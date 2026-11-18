//! The threaded runner of a street run (pivot plan Phase 2, section 7.4). `StreetRunner`
//! owns the running `StreetRun` and steps it at its fixed tick; the views draw from the
//! snapshots around its render clock. The run
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

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use bevy::prelude::*;
use tf_sim::street::ids::AgentId;
use tf_sim::street::level::Mode;
use tf_sim::street::run::{
    CauseKind, Grade, LoggedEvent, Outcome, StreetEvent, StreetRun, TICK_RATE,
};
use tf_sim::street::signals::Light;

/// The run steps this far ahead of the screen (in ticks): the time a tick has to compute in
/// the background before it is needed on screen.
pub const LEAD_TICKS: u32 = 2;
/// Most ticks the worker runs in one go.
pub const MAX_TICKS_PER_BATCH: usize = 4;

/// Whether a run steps on a worker thread: yes unless the machine has a single core. Tests
/// and screenshot mode pass `false` and step inline.
pub fn default_threaded() -> bool {
    std::thread::available_parallelism().is_ok_and(|n| n.get() > 1)
}

/// Run speeds: index 0 is paused. The first four are the old runner's `SPEED_MULTIPLIERS`
/// (so keys 1 to 3 mean the same); x8 is new, to watch a long slice of the day.
pub const STREET_SPEED_MULTIPLIERS: [u32; 5] = [0, 1, 2, 4, 8];

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
    /// Metres from the middle of its lane, to the right of its direction of travel
    /// (`Agent::lateral`). `pos` already holds it; the inspector card reads it.
    pub lateral: f32,
    pub speed: f32,
    pub frustration: f32,
    /// Stopped for others: at its own line, or held by people on foot.
    pub waiting: bool,
    pub crashed: bool,
    pub distracted: bool,
    /// A driver stopped at its junction line by pedestrians (stage C).
    pub held_by_walkers: bool,
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
    /// Its way crosses a carriageway out of a crossing (stage C: it jaywalks).
    pub off_crossing: bool,
    /// It stepped off on red within the run's recent-cause window.
    pub walked_on_red: bool,
    /// Hit by a vehicle: it lies where it fell until the run takes it away.
    pub hit: bool,
}

/// The traits of a driver, fixed at spawn (pivot plan §5.3). Sent once, with the snapshot
/// of the tick the agent appeared, and kept by the runner (`StreetRunner::info`), so the
/// per-tick snapshots stay small. For the inspector card.
#[derive(Clone, Debug, PartialEq)]
pub struct CarInfo {
    pub id: AgentId,
    pub mode: Mode,
    pub archetype: &'static str,
    /// Desired speed as a factor of the limit.
    pub speed_factor: f64,
    pub patience_s: f64,
    pub compliance: f64,
    pub attention: f64,
    pub reaction_s: f64,
    pub risk: f64,
    pub courtesy: f64,
    /// Riders only: their own habits.
    pub bike: Option<BikeInfo>,
}

/// The habits of a rider, fixed at spawn (`BikeHabits`): where it rides, and how likely it
/// is to break a rule. The `*_u` numbers are the rider's fixed draw: the rule is broken
/// when the draw is under the chance of the situation, so a low number means a rider that
/// breaks it easily.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BikeInfo {
    /// Metres from the right edge of a car lane.
    pub edge_m: f64,
    pub red_u: f64,
    pub sidewalk_u: f64,
    pub filter_u: f64,
}

/// The same for a pedestrian.
#[derive(Clone, Debug, PartialEq)]
pub struct WalkerInfo {
    pub id: AgentId,
    pub archetype: &'static str,
    pub group_size: usize,
    /// The speed it walks at on a free sidewalk, m/s.
    pub desired_speed: f64,
    pub patience_s: f64,
    pub compliance: f64,
    pub attention: f64,
    pub reaction_s: f64,
    pub risk: f64,
    pub courtesy: f64,
}

/// What never changes about an agent.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentInfo {
    Car(CarInfo),
    Walker(WalkerInfo),
}

impl AgentInfo {
    pub fn id(&self) -> AgentId {
        match self {
            AgentInfo::Car(car) => car.id,
            AgentInfo::Walker(walker) => walker.id,
        }
    }
}

/// The agents after one tick, in id order (so a pose is found by binary search).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreetSnapshot {
    pub tick: u64,
    pub agents: Vec<AgentPose>,
    pub walkers: Vec<WalkerPose>,
    pub finished: bool,
    /// The light every approach of a signalled junction shows now (empty without signals).
    pub lights: Vec<ApproachLight>,
    /// What the run logged in this tick. `capture` leaves it empty; the batch fills it,
    /// so the runner can count events and mark incidents without ever reading the run's
    /// log from the main thread.
    pub new_events: Vec<LoggedEvent>,
    /// The traits of the agents that appeared in this tick. Like `new_events`, filled by
    /// the batch (from the spawn events), not by `capture`.
    pub new_info: Vec<AgentInfo>,
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
                lateral: agent.lateral as f32,
                speed: agent.speed as f32,
                frustration: agent.frustration as f32,
                waiting: agent.wait_ticks > 0 || agent.held_by_walkers,
                crashed: agent.crashed(),
                distracted: run.tick < agent.distracted_until,
                held_by_walkers: agent.held_by_walkers,
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
                    off_crossing: walker.off_crossing,
                    walked_on_red: walker
                        .recent
                        .iter()
                        .any(|(_, cause)| matches!(cause, CauseKind::WalkedOnRed)),
                    hit: walker.hit(),
                }
            })
            .collect();
        walkers.sort_by_key(|pose| pose.id);
        Self {
            tick: run.tick,
            agents,
            walkers,
            finished: run.finished(),
            new_events: Vec::new(),
            new_info: Vec::new(),
            lights: approach_lights(run),
        }
    }

    /// The traits of the agents named by `events` (the spawns of one tick), read from the
    /// run as it is after that tick.
    pub fn info_for(run: &StreetRun, events: &[LoggedEvent]) -> Vec<AgentInfo> {
        let mut info = Vec::new();
        for logged in events {
            match &logged.event {
                StreetEvent::Spawned { agent, .. } => {
                    if let Some(car) = run.agents.get(agent) {
                        info.push(AgentInfo::Car(CarInfo {
                            id: car.id,
                            mode: car.mode,
                            archetype: car.archetype,
                            speed_factor: car.traits.speed_factor,
                            patience_s: car.traits.patience_s,
                            compliance: car.traits.compliance,
                            attention: car.traits.attention,
                            reaction_s: car.traits.reaction_s,
                            risk: car.traits.risk,
                            courtesy: car.traits.courtesy,
                            bike: car.bike.as_ref().map(|habits| BikeInfo {
                                edge_m: habits.edge_m,
                                red_u: habits.red_u,
                                sidewalk_u: habits.sidewalk_u,
                                filter_u: habits.filter_u,
                            }),
                        }));
                    }
                }
                StreetEvent::WalkerSpawned { agent, .. } => {
                    if let Some(walker) = run.walkers.get(agent) {
                        info.push(AgentInfo::Walker(WalkerInfo {
                            id: walker.id,
                            archetype: walker.archetype,
                            group_size: walker.members.len(),
                            desired_speed: walker.desired_speed,
                            patience_s: walker.traits.patience_s,
                            compliance: walker.traits.compliance,
                            attention: walker.traits.attention,
                            reaction_s: walker.traits.reaction_s,
                            risk: walker.traits.risk,
                            courtesy: walker.traits.courtesy,
                        }));
                    }
                }
                _ => {}
            }
        }
        info
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

/// What a signal shows an approach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalColor {
    Green,
    Amber,
    Red,
}

/// The light of one approach (a segment id) of a signalled junction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApproachLight {
    pub node: i32,
    pub segment: i32,
    pub color: SignalColor,
    /// The light of the bicycle lane (`SignalPlan::bicycle_light`): the same as `color`,
    /// but green first when the plan has a leading green.
    pub bicycle: SignalColor,
}

fn signal_color(light: Light) -> SignalColor {
    match light {
        Light::Green(_) => SignalColor::Green,
        Light::Amber(_) => SignalColor::Amber,
        Light::Red(_) => SignalColor::Red,
    }
}

/// The lights of every signalled junction at the run's time: a few entries, cheap to
/// copy into each snapshot.
fn approach_lights(run: &StreetRun) -> Vec<ApproachLight> {
    let time_s = run.tick as f64 / f64::from(TICK_RATE);
    let mut lights = Vec::new();
    for (node, plan) in run.signal_plans() {
        let Some(junction) = run.street.nodes.get(node) else {
            continue;
        };
        for segment in &junction.segment_ids {
            lights.push(ApproachLight {
                node: *node,
                segment: *segment,
                color: signal_color(plan.light(*segment, time_s)),
                bicycle: signal_color(plan.bicycle_light(*segment, time_s)),
            });
        }
    }
    lights
}

/// What happened on the street that the player should see where it happened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IncidentKind {
    /// A near miss (post-encroachment time or a hard brake).
    LightConflict,
    SeriousConflict,
    /// A vehicle touched a vehicle or a walker, with how it ended.
    Contact(Outcome),
}

/// A mark the play view keeps on the map until the next run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Incident {
    pub tick: u64,
    /// Metres, y down.
    pub position: Vec2,
    pub kind: IncidentKind,
}

/// Running counts of a run, for the HUD, added up from the events as snapshots arrive.
/// The final figures are the run's `summary()`; this is the live view of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunTally {
    /// The slice of the day now running.
    pub slice: Option<String>,
    pub spawned: u32,
    pub walkers_spawned: u32,
    pub arrived: u32,
    pub walkers_arrived: u32,
    pub gave_up: u32,
    pub conflicts_light: u32,
    pub conflicts_serious: u32,
    pub contacts: u32,
    /// Light or serious injuries (vehicles and walkers), and deaths.
    pub injuries: u32,
    pub deaths: u32,
}

impl RunTally {
    pub fn conflicts(&self) -> u32 {
        self.conflicts_light + self.conflicts_serious
    }

    /// Counts one logged event; returns the mark it leaves on the map, if any.
    pub fn add(&mut self, logged: &LoggedEvent) -> Option<Incident> {
        let tick = logged.tick;
        match &logged.event {
            StreetEvent::SliceStarted { name, .. } => self.slice = Some(name.clone()),
            StreetEvent::Spawned { .. } => self.spawned += 1,
            StreetEvent::WalkerSpawned { .. } => self.walkers_spawned += 1,
            StreetEvent::TripEnded { .. } => self.arrived += 1,
            StreetEvent::WalkEnded { .. } => self.walkers_arrived += 1,
            StreetEvent::GaveUp { .. } => self.gave_up += 1,
            StreetEvent::Conflict {
                grade, position, ..
            } => {
                let kind = match grade {
                    Grade::Light => {
                        self.conflicts_light += 1;
                        IncidentKind::LightConflict
                    }
                    Grade::Serious => {
                        self.conflicts_serious += 1;
                        IncidentKind::SeriousConflict
                    }
                };
                return Some(Incident {
                    tick,
                    position: *position,
                    kind,
                });
            }
            StreetEvent::Contact {
                position, outcome, ..
            } => {
                self.contacts += 1;
                match outcome {
                    Outcome::LightInjury | Outcome::SeriousInjury => self.injuries += 1,
                    Outcome::Death => self.deaths += 1,
                    Outcome::NoInjury => {}
                }
                return Some(Incident {
                    tick,
                    position: *position,
                    kind: IncidentKind::Contact(*outcome),
                });
            }
            _ => {}
        }
        None
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
    // Every earlier tick was snapshotted, so the log up to here is already reported.
    let mut seen = run.events().len();
    for _ in 0..count {
        run.step();
        let mut snapshot = StreetSnapshot::capture(&run);
        snapshot.new_events = run.events()[seen..].to_vec();
        snapshot.new_info = StreetSnapshot::info_for(&run, &snapshot.new_events);
        seen = run.events().len();
        snapshots.push(Arc::new(snapshot));
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
    /// The speed to go back to when a paused run is resumed.
    resume_index: usize,
    tally: RunTally,
    incidents: Vec<Incident>,
    /// Bumped whenever `incidents` changes, so a view redraws only then.
    incident_version: u64,
    /// The traits of every agent of the run, copied once from the snapshot of its spawn.
    /// Only looked up by id (for the inspector), never walked.
    info: HashMap<AgentId, AgentInfo>,
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
            resume_index: DEFAULT_SPEED_INDEX,
            tally: RunTally::default(),
            incidents: Vec::new(),
            incident_version: 0,
            info: HashMap::new(),
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
        // A new run starts with a clean tally and map: the marks of the last run go now.
        self.tally = RunTally::default();
        self.info.clear();
        self.clear_incidents();
        let first = Arc::new(StreetSnapshot::capture(&run));
        self.render_tick = first.tick as f64;
        self.run = Some(Box::new(run));
        self.worker = threaded.then(Worker::start);
        self.active = true;
        self.snapshots = VecDeque::from([first]);
        self.update_view();
    }

    /// Drops the run (after the batch in flight) and clears the views' snapshots. The
    /// incident marks stay until the next `start`, so the player can study them.
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
        self.speed_index = index.min(STREET_SPEED_MULTIPLIERS.len() - 1);
        if self.speed_index > 0 {
            self.resume_index = self.speed_index;
        }
    }

    /// Space: pause, or go on at the speed before the pause.
    pub fn toggle_pause(&mut self) {
        if self.speed_index == 0 {
            self.set_speed(self.resume_index);
        } else {
            self.speed_index = 0;
        }
    }

    pub fn tally(&self) -> &RunTally {
        &self.tally
    }

    /// The marks of the conflicts and contacts of the last run, in the order they happened.
    pub fn incidents(&self) -> &[Incident] {
        &self.incidents
    }

    /// The traits of agent `id` (a car or a walker), known from its spawn on.
    pub fn info(&self, id: AgentId) -> Option<&AgentInfo> {
        self.info.get(&id)
    }

    pub fn incident_version(&self) -> u64 {
        self.incident_version
    }

    fn clear_incidents(&mut self) {
        if !self.incidents.is_empty() {
            self.incidents.clear();
            self.incident_version += 1;
        }
    }

    /// The run has ended (a death, or the day is over and the street is empty). Looks at
    /// the run itself when the main thread has it, else at the newest snapshot it got.
    pub fn run_ended(&self) -> bool {
        match &self.run {
            Some(run) => run.finished(),
            None => self.snapshots.back().is_some_and(|s| s.finished),
        }
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
        let multiplier = f64::from(STREET_SPEED_MULTIPLIERS[self.speed_index]);
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
        for snapshot in &done.snapshots {
            for info in &snapshot.new_info {
                self.info.insert(info.id(), info.clone());
            }
            for logged in &snapshot.new_events {
                if let Some(incident) = self.tally.add(logged) {
                    self.incidents.push(incident);
                    self.incident_version += 1;
                }
            }
        }
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
            assert_eq!(pose.waiting, agent.wait_ticks > 0 || agent.held_by_walkers);
            assert_eq!(pose.held_by_walkers, agent.held_by_walkers);
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
