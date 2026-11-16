//! The threaded runner of a street run (pivot plan Phase 2, section 7.4). `StreetRunner`
//! owns the running `StreetRun` and steps it at its fixed tick; the views draw from the
//! snapshots around its render clock. It copies the design of `runner::SimHandle`: the run
//! moves *by value* to one long-lived worker thread over channels (no locks in the sim),
//! the worker steps a batch of at most `MAX_TICKS_PER_BATCH` ticks to stay `LEAD_TICKS`
//! ahead of the screen and captures a `StreetSnapshot` after each tick.
//!
//! **Rule: nothing from the frame enters the sim.** The frame delta and the speed only move
//! the render clock, which decides *how many* whole `StreetRun::step()` calls are made; the
//! batch size and the thread decide nothing else. So a run steps the same sequence of ticks
//! however the frames fall, and its event log hash equals a headless run's.
//!
//! Pausing is speed 0. When the run has ended no more ticks are asked for. Without a worker
//! (`threaded: false`: tests, screenshot mode) batches run inline inside `advance`.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use bevy::prelude::*;
use tf_sim::street::ids::AgentId;
use tf_sim::street::level::Mode;
use tf_sim::street::run::{StreetRun, TICK_RATE};

use crate::runner::{LEAD_TICKS, MAX_TICKS_PER_BATCH, SPEED_MULTIPLIERS};

/// Speed index a run starts at: x1.
pub const DEFAULT_SPEED_INDEX: usize = 1;

/// One agent as the views need it, copied out of the run so the views never touch it.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentPose {
    pub id: AgentId,
    pub mode: Mode,
    pub archetype: &'static str,
    /// Middle of the body, metres, y down.
    pub pos: Vec2,
    pub dir: Vec2,
    pub prev_pos: Vec2,
    pub prev_dir: Vec2,
    pub length: f32,
    pub width: f32,
    pub speed: f32,
    pub frustration: f32,
    pub waiting: bool,
    pub crashed: bool,
    pub distracted: bool,
}

/// One pedestrian as the views need it (pivot plan Phase 3): `pos` and `prev_pos` are the
/// body as the run has it, `dir` the way it walks (its velocity, or its last step while it
/// stands), so the view can put the mark of the side it goes to.
#[derive(Clone, Debug, PartialEq)]
pub struct WalkerPose {
    pub id: AgentId,
    pub archetype: &'static str,
    /// The first member of its group; its own id when it walks alone.
    pub group: AgentId,
    /// Middle of the body, metres, y down.
    pub pos: Vec2,
    pub prev_pos: Vec2,
    /// Unit vector of the way it walks; zero when it has not moved lately.
    pub dir: Vec2,
    pub radius: f32,
    /// It stands at a kerb, waiting for its gap.
    pub waiting: bool,
    /// On a carriageway (a crossing or out of one).
    pub on_carriageway: bool,
}

/// The agents after one tick, in id order (so a pose is found by binary search).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreetSnapshot {
    pub tick: u64,
    pub agents: Vec<AgentPose>,
    pub walkers: Vec<WalkerPose>,
    pub finished: bool,
}

impl StreetSnapshot {
    pub fn capture(run: &StreetRun) -> Self {
        let mut agents: Vec<AgentPose> = run
            .agents
            .values()
            .map(|agent| AgentPose {
                id: agent.id,
                mode: agent.mode,
                archetype: agent.archetype,
                pos: agent.pos,
                dir: agent.dir,
                prev_pos: agent.prev_pos,
                prev_dir: agent.prev_dir,
                length: agent.length as f32,
                width: agent.width as f32,
                speed: agent.speed as f32,
                frustration: agent.frustration as f32,
                waiting: agent.wait_ticks > 0,
                crashed: agent.crashed(),
                distracted: run.tick < agent.distracted_until,
            })
            .collect();
        agents.sort_by_key(|pose| pose.id);
        let mut walkers: Vec<WalkerPose> = run
            .walkers
            .values()
            .map(|walker| {
                let area = run.walk_paths[walker.path]
                    .areas
                    .get(walker.area_index)
                    .map(|&index| run.walk_space.areas[index].kind);
                WalkerPose {
                    id: walker.id,
                    archetype: walker.archetype,
                    group: walker.group,
                    pos: walker.pos,
                    prev_pos: walker.prev_pos,
                    dir: walk_direction(walker.vel, walker.pos - walker.prev_pos),
                    radius: walker.radius,
                    waiting: walker.kerb_since.is_some(),
                    on_carriageway: area.is_some_and(|kind| kind.is_road()),
                }
            })
            .collect();
        walkers.sort_by_key(|pose| pose.id);
        Self {
            tick: run.tick,
            agents,
            walkers,
            finished: run.finished(),
        }
    }

    /// Where agent `id` is in `agents`.
    pub fn index_of(&self, id: AgentId) -> Option<usize> {
        self.agents.binary_search_by_key(&id, |pose| pose.id).ok()
    }

    /// Where walker `id` is in `walkers`.
    pub fn walker_index_of(&self, id: AgentId) -> Option<usize> {
        self.walkers.binary_search_by_key(&id, |pose| pose.id).ok()
    }
}

/// The unit direction a walking body shows: its velocity, or its last step while the
/// crowd holds it in place (a standing body keeps the heading it came with).
fn walk_direction(vel: Vec2, step: Vec2) -> Vec2 {
    if vel.length_squared() > 1e-6 {
        vel.normalize()
    } else if step.length_squared() > 1e-8 {
        step.normalize()
    } else {
        Vec2::ZERO
    }
}

struct Job {
    run: Box<StreetRun>,
    count: usize,
}

struct Done {
    run: Box<StreetRun>,
    snapshots: Vec<Arc<StreetSnapshot>>,
}

/// Steps `count` ticks, snapshotting after each (on the worker if threaded).
fn run_batch(job: Job) -> Done {
    let Job { mut run, count } = job;
    let mut snapshots = Vec::with_capacity(count);
    for _ in 0..count {
        run.step();
        snapshots.push(Arc::new(StreetSnapshot::capture(&run)));
    }
    Done { run, snapshots }
}

/// One long-lived worker thread. The receiver sits in a `Mutex` only to make the resource
/// `Sync`; only the main thread touches it.
struct Worker {
    jobs: Option<Sender<Job>>,
    done: Mutex<Receiver<Done>>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    fn start() -> Self {
        let (job_tx, job_rx) = channel::<Job>();
        let (done_tx, done_rx) = channel::<Done>();
        let thread = std::thread::Builder::new()
            .name("street-worker".into())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    if done_tx.send(run_batch(job)).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn the street run worker");
        Self {
            jobs: Some(job_tx),
            done: Mutex::new(done_rx),
            thread: Some(thread),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing the job channel ends the worker's loop.
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Resource)]
pub struct StreetRunner {
    /// `None` while a batch has it on the worker, or when no run is started.
    run: Option<Box<StreetRun>>,
    active: bool,
    worker: Option<Worker>,
    busy: bool,
    speed_index: usize,
    /// Snapshots ready for the views, oldest first.
    snapshots: VecDeque<Arc<StreetSnapshot>>,
    from: Arc<StreetSnapshot>,
    to: Arc<StreetSnapshot>,
    alpha: f32,
    /// Play time on screen, in ticks.
    render_tick: f64,
}

impl Default for StreetRunner {
    fn default() -> Self {
        Self {
            run: None,
            active: false,
            worker: None,
            busy: false,
            speed_index: DEFAULT_SPEED_INDEX,
            snapshots: VecDeque::new(),
            from: Arc::default(),
            to: Arc::default(),
            alpha: 0.0,
            render_tick: 0.0,
        }
    }
}

impl StreetRunner {
    /// Starts `run`, replacing any run in progress. `threaded`: step it on a worker thread;
    /// otherwise inline.
    pub fn start(&mut self, run: StreetRun, threaded: bool) {
        self.stop();
        let first = Arc::new(StreetSnapshot::capture(&run));
        self.render_tick = first.tick as f64;
        self.run = Some(Box::new(run));
        self.worker = threaded.then(Worker::start);
        self.active = true;
        self.snapshots = VecDeque::from([first]);
        self.update_view();
    }

    /// Drops the run (after the batch in flight) and clears the views' snapshots.
    pub fn stop(&mut self) {
        self.wait_for_worker();
        self.run = None;
        self.worker = None;
        self.active = false;
        self.snapshots.clear();
        self.from = Arc::default();
        self.to = Arc::default();
        self.alpha = 0.0;
        self.render_tick = 0.0;
    }

    pub fn is_running(&self) -> bool {
        self.active
    }

    pub fn speed_index(&self) -> usize {
        self.speed_index
    }

    pub fn set_speed(&mut self, index: usize) {
        self.speed_index = index.min(SPEED_MULTIPLIERS.len() - 1);
    }

    pub fn view_from(&self) -> &StreetSnapshot {
        &self.from
    }

    pub fn view_to(&self) -> &StreetSnapshot {
        &self.to
    }

    /// How far (0-1) the render clock is from `view_from` to `view_to`.
    pub fn view_alpha(&self) -> f32 {
        self.alpha
    }

    /// The run, for tests and the end-of-run report: waits for the batch in flight.
    pub fn get_mut(&mut self) -> Option<&mut StreetRun> {
        self.wait_for_worker();
        self.run.as_deref_mut()
    }

    /// One frame: moves the render clock by `real_dt_s` real seconds at the chosen speed
    /// (the only thing the frame decides), and keeps the run `LEAD_TICKS` ahead of it.
    pub fn advance(&mut self, real_dt_s: f64) {
        if !self.active {
            return;
        }
        if self.busy {
            self.try_collect();
        }
        let multiplier = f64::from(SPEED_MULTIPLIERS[self.speed_index]);
        self.render_tick += real_dt_s.max(0.0) * multiplier * f64::from(TICK_RATE);
        let newest = self.snapshots.back().expect("a snapshot").tick as f64;
        if self.render_tick > newest {
            // The run is behind: wait for it rather than run ahead of it.
            self.render_tick = newest;
        }
        if multiplier > 0.0 {
            self.start_batch();
        }
        self.update_view();
    }

    /// Runs (or, threaded, starts running) the ticks that keep the run ahead of the screen.
    /// None once the run has ended.
    fn start_batch(&mut self) {
        if self.busy || self.snapshots.back().is_some_and(|s| s.finished) {
            return;
        }
        let run = self.run.as_ref().expect("idle");
        let behind = self.render_tick + f64::from(LEAD_TICKS) - run.tick as f64;
        let count = ((behind - 0.001).ceil().max(0.0) as usize).min(MAX_TICKS_PER_BATCH);
        if count == 0 {
            return;
        }
        let run = self.run.take().expect("idle");
        match &self.worker {
            Some(worker) => {
                worker
                    .jobs
                    .as_ref()
                    .expect("worker running")
                    .send(Job { run, count })
                    .expect("the street worker is alive");
                self.busy = true;
            }
            None => self.collect(run_batch(Job { run, count })),
        }
    }

    fn collect(&mut self, done: Done) {
        self.busy = false;
        self.run = Some(done.run);
        self.snapshots.extend(done.snapshots);
    }

    fn try_collect(&mut self) {
        let received = self
            .worker
            .as_ref()
            .expect("busy means threaded")
            .done
            .lock()
            .expect("only the main thread locks")
            .try_recv();
        match received {
            Ok(done) => self.collect(done),
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                panic!("the street worker died")
            }
        }
    }

    /// Lets a running batch finish (so the run can be used from the main thread).
    fn wait_for_worker(&mut self) {
        if !self.busy {
            return;
        }
        let done = self
            .worker
            .as_ref()
            .expect("busy means threaded")
            .done
            .lock()
            .expect("only the main thread locks")
            .recv()
            .expect("the street worker died");
        self.collect(done);
    }

    /// Picks the snapshots on either side of the render clock, dropping older ones.
    fn update_view(&mut self) {
        if self.snapshots.is_empty() {
            return;
        }
        while self.snapshots.len() > 2 && self.snapshots[1].tick as f64 <= self.render_tick {
            self.snapshots.pop_front();
        }
        self.from = self.snapshots[0].clone();
        self.to = self.snapshots.get(1).unwrap_or(&self.snapshots[0]).clone();
        let span = self.to.tick as f64 - self.from.tick as f64;
        self.alpha = if span > 0.0 {
            ((self.render_tick - self.from.tick as f64) / span).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
    }
}

impl Drop for StreetRunner {
    fn drop(&mut self) {
        // A batch in flight finishes first, so the worker isn't left holding the run.
        self.wait_for_worker();
    }
}

/// Advances the runner by the frame's real time, in `PreUpdate` like the old runner.
fn advance_street_run(time: Res<Time<Real>>, mut runner: ResMut<StreetRunner>) {
    runner.advance(time.delta_secs_f64());
}

pub struct StreetRunPlugin;

impl Plugin for StreetRunPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StreetRunner>()
            .add_systems(PreUpdate, advance_street_run);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tf_sim::street::run::RunOptions;
    use tf_sim::street::{Design, LevelFile};

    fn new_run() -> StreetRun {
        let level = LevelFile::load("carrefour-ecole").expect("loads");
        let options = RunOptions::for_level(&level);
        StreetRun::new(&level, &Design::new(&level.id), options).expect("builds")
    }

    #[test]
    fn a_snapshot_copies_the_agents_in_id_order() {
        let mut run = new_run();
        let mut steps = 0;
        while run.agents.is_empty() {
            run.step();
            steps += 1;
            assert!(steps < 20_000, "no agent ever came in");
        }
        let snapshot = StreetSnapshot::capture(&run);
        assert_eq!(snapshot.tick, run.tick);
        assert_eq!(snapshot.agents.len(), run.agents.len());
        assert!(snapshot.agents.windows(2).all(|w| w[0].id < w[1].id));
        for agent in run.agents.values() {
            let pose = &snapshot.agents[snapshot.index_of(agent.id).expect("found")];
            assert_eq!(pose.pos, agent.pos);
            assert_eq!(pose.archetype, agent.archetype);
            assert_eq!(pose.waiting, agent.wait_ticks > 0);
            assert_eq!(pose.crashed, agent.crashed());
        }
        assert_eq!(snapshot.index_of(AgentId(-1)), None);
    }

    #[test]
    fn an_idle_runner_does_nothing() {
        let mut runner = StreetRunner::default();
        assert!(!runner.is_running());
        runner.advance(1.0);
        assert!(runner.get_mut().is_none());
        assert!(runner.view_to().agents.is_empty());
    }
}
