//! The threaded simulation runner (`core/game.gd`). `SimHandle` owns the running
//! `Simulation` and drives it at a fixed tick.
//!
//! Pausing is speed 0, not a paused Bevy schedule, so the camera, tools and UI keep
//! working while the sim is frozen.
//!
//! **Threading.** The `Simulation` moves *by value* between the main thread and one
//! long-lived worker thread over channels (no locks anywhere in the sim). The worker steps
//! a batch of up to `MAX_TICKS_PER_BATCH` ticks, keeping the sim `LEAD_TICKS` ahead of
//! what is on screen, captures a `ViewSnapshot` after each tick, and sends the sim and
//! the snapshots back. Per-frame views draw from `view_from` / `view_to` / `view_alpha`
//! (the snapshots around the render clock, interpolating moving things). Rules:
//!
//! - `get_mut` waits for the batch in flight and hands the sim over: fine for tools,
//!   saves and commands, a hitch if called every frame (read the view instead).
//! - `roads()` returns the road geometry as an `Arc<RoadsView>`, published after every
//!   road edit, so views never wait. Compare `RoadsView::version` to notice a new one.
//! - Sim signals the views need are relayed as `RunnerEvent`s from `advance`, on the main
//!   thread, while the sim is home (the Bevy plugin turns them into messages).
//!
//! If the worker can't keep up the render clock waits for it: the sim runs slower than
//! asked (`sim_lagging`) instead of frames stalling. Without a worker (`threaded: false`,
//! headless tests, one-core machines) batches run inline inside `advance`.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use bevy::prelude::Resource;
use tf_sim::Simulation;
use tf_sim::events::SimEvent;
use tf_sim::ids::{NodeId, SegmentId};
use tf_sim::roads::RoadNetwork;

use crate::snapshot::ViewSnapshot;

/// Sim speeds: index 0 is paused.
pub const SPEED_MULTIPLIERS: [u32; 4] = [0, 1, 2, 4];
/// The simulation runs this far ahead of the screen (in ticks): the time a tick has to
/// compute in the background before it's needed on screen.
pub const LEAD_TICKS: u32 = 2;
/// Most ticks the worker runs in one go.
pub const MAX_TICKS_PER_BATCH: usize = 4;
/// `sim_lagging`: the simulation runs below this fraction of the chosen speed (smoothed).
pub const LAG_THRESHOLD: f64 = 0.9;
pub const SPEED_SMOOTHING: f64 = 0.02;

/// The road network as the views read it: a copy taken after every road edit.
#[derive(Debug)]
pub struct RoadsView {
    /// Goes up by one with every publication.
    pub version: u64,
    pub roads: RoadNetwork,
}

/// What the runner tells the rest of the game (Godot's `Events` signals that the runner
/// emitted: see `events.rs` for the messages they become).
#[derive(Clone, Debug, PartialEq)]
pub enum RunnerEvent {
    /// A new `Simulation` replaced the old one.
    Reset,
    SpeedChanged {
        speed_index: usize,
        multiplier: u32,
    },
    HourChanged(i32),
    DayStarted(i32),
    RoadNetworkChanged {
        node_ids: Vec<NodeId>,
        segment_ids: Vec<SegmentId>,
    },
    LotsChanged,
    ZonesChanged,
    ExternalsChanged,
}

struct Job {
    sim: Box<Simulation>,
    count: usize,
}

struct Done {
    sim: Box<Simulation>,
    snapshots: Vec<Arc<ViewSnapshot>>,
    /// Milliseconds each tick took.
    tick_ms: Vec<f64>,
    /// Milliseconds each snapshot capture took (not counted in `tick_ms`).
    capture_ms: Vec<f64>,
}

/// Steps `count` ticks, snapshotting after each (on the worker if threaded).
fn run_batch(job: Job) -> Done {
    let Job { mut sim, count } = job;
    let mut snapshots = Vec::with_capacity(count);
    let mut tick_ms = Vec::with_capacity(count);
    let mut capture_ms = Vec::with_capacity(count);
    for _ in 0..count {
        let started = Instant::now();
        sim.step();
        tick_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        let captured = Instant::now();
        snapshots.push(Arc::new(ViewSnapshot::capture(&mut sim)));
        capture_ms.push(captured.elapsed().as_secs_f64() * 1000.0);
    }
    Done {
        sim,
        snapshots,
        tick_ms,
        capture_ms,
    }
}

/// One long-lived worker thread (the OS learns it is busy and gives it a fast core, which
/// a fresh thread per batch doesn't get), running a batch whenever sent one. The
/// receiver sits in a `Mutex` only to make the resource `Sync`; only the main thread
/// touches it.
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
            .name("sim-worker".into())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    if done_tx.send(run_batch(job)).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn the simulation worker");
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
pub struct SimHandle {
    /// `None` while a batch has it on the worker.
    sim: Option<Box<Simulation>>,
    worker: Option<Worker>,
    busy: bool,
    speed_index: usize,
    last_running_speed_index: usize,
    /// How fast the simulation actually runs (sim seconds per real second, smoothed), and
    /// whether that is well below the chosen speed (it couldn't keep up, so it ran
    /// slower).
    pub effective_speed: f64,
    pub sim_lagging: bool,
    /// Smoothed cost of one simulation tick, for the debug readout.
    pub average_tick_ms: f64,
    /// Smoothed cost of capturing the snapshot after a tick (on the worker, on top of
    /// `average_tick_ms`).
    pub average_capture_ms: f64,
    /// Snapshots ready for the views, oldest first.
    snapshots: VecDeque<Arc<ViewSnapshot>>,
    /// The snapshots just before and after the render clock, and how far (0-1) between
    /// them.
    pub view_from: Arc<ViewSnapshot>,
    pub view_to: Arc<ViewSnapshot>,
    pub view_alpha: f32,
    /// Sim time on screen.
    render_time: f64,
    roads: Arc<RoadsView>,
    roads_version: u64,
    /// Events queued for the next `advance` (reset, speed changes).
    pending: Vec<RunnerEvent>,
}

impl SimHandle {
    /// Runs `sim`. `threaded`: step it on a worker thread; otherwise inline (headless
    /// tests, machines with one core).
    pub fn new(sim: Simulation, threaded: bool) -> Self {
        let mut sim = Box::new(sim);
        let first = Arc::new(ViewSnapshot::capture(&mut sim));
        let roads = Arc::new(RoadsView {
            version: 0,
            roads: sim.roads.clone(),
        });
        Self {
            sim: Some(sim),
            worker: threaded.then(Worker::start),
            busy: false,
            speed_index: 1,
            last_running_speed_index: 1,
            effective_speed: 1.0,
            sim_lagging: false,
            average_tick_ms: 0.0,
            average_capture_ms: 0.0,
            render_time: first.time,
            view_from: first.clone(),
            view_to: first.clone(),
            view_alpha: 0.0,
            snapshots: VecDeque::from([first]),
            roads,
            roads_version: 0,
            pending: vec![RunnerEvent::Reset],
        }
    }

    /// Threaded unless the machine has a single core (Godot's rule, minus "headless":
    /// tests that want the thread ask for it, the others pass `false`).
    pub fn default_threaded() -> bool {
        std::thread::available_parallelism().is_ok_and(|n| n.get() > 1)
    }

    pub fn is_threaded(&self) -> bool {
        self.worker.is_some()
    }

    /// The simulation, for tools, commands and saves (`Game.simulation`): waits for the
    /// batch in flight first. Edits made through it reach the views as `RunnerEvent`s at
    /// the next `advance`.
    pub fn get_mut(&mut self) -> &mut Simulation {
        self.wait_for_worker();
        self.sim.as_mut().expect("the simulation is home")
    }

    /// The road geometry, readable without waiting for the worker.
    pub fn roads(&self) -> Arc<RoadsView> {
        self.roads.clone()
    }

    /// Replaces the running simulation (a fresh one is `Simulation::new()`) and tells
    /// everyone to resync (`Game.new_simulation`).
    pub fn replace(&mut self, sim: Simulation) {
        self.wait_for_worker();
        let mut sim = Box::new(sim);
        let first = Arc::new(ViewSnapshot::capture(&mut sim));
        self.render_time = first.time;
        self.snapshots = VecDeque::from([first]);
        self.sim = Some(sim);
        self.publish_roads();
        self.update_view();
        self.pending.push(RunnerEvent::Reset);
    }

    pub fn speed_index(&self) -> usize {
        self.speed_index
    }

    pub fn multiplier(&self) -> u32 {
        SPEED_MULTIPLIERS[self.speed_index]
    }

    pub fn is_paused(&self) -> bool {
        self.speed_index == 0
    }

    pub fn set_speed(&mut self, index: usize) {
        self.speed_index = index.min(SPEED_MULTIPLIERS.len() - 1);
        self.effective_speed = f64::from(self.multiplier());
        if self.speed_index > 0 {
            self.last_running_speed_index = self.speed_index;
        }
        self.pending.push(RunnerEvent::SpeedChanged {
            speed_index: self.speed_index,
            multiplier: self.multiplier(),
        });
    }

    pub fn toggle_pause(&mut self) {
        self.set_speed(if self.is_paused() {
            self.last_running_speed_index
        } else {
            0
        });
    }

    /// Sim time on screen (sim seconds).
    pub fn render_time(&self) -> f64 {
        self.render_time
    }

    /// One frame (`Game._process`): collects a finished batch, moves the render clock by
    /// `delta` real seconds at the chosen speed, relays the sim's signals, starts the next
    /// batch and picks the snapshots to draw. Returns the events for the rest of the game.
    pub fn advance(&mut self, delta: f64) -> Vec<RunnerEvent> {
        if self.busy {
            self.try_collect();
        }
        let multiplier = f64::from(self.multiplier());
        let before = self.render_time;
        self.render_time += delta * multiplier;
        let newest = self.snapshots.back().expect("a snapshot").time;
        if self.render_time > newest {
            // The simulation is behind: wait for it rather than run ahead of it.
            self.render_time = newest;
        }
        if multiplier > 0.0 && delta > 0.0 {
            let rate = (self.render_time - before).max(0.0) / delta;
            self.effective_speed += (rate - self.effective_speed) * SPEED_SMOOTHING;
            self.sim_lagging = self.effective_speed < multiplier * LAG_THRESHOLD;
        } else {
            self.sim_lagging = false;
        }
        // Relay the signals of the ticks just collected while the worker is idle (once
        // the next batch starts, the worker may be adding to them).
        let mut events = std::mem::take(&mut self.pending);
        if !self.busy {
            self.relay(&mut events);
        }
        if multiplier > 0.0 {
            self.start_batch();
        }
        self.update_view();
        events
    }

    /// Takes the sim's view events and turns the ones Godot's `Game` relayed into
    /// `RunnerEvent`s: hours, days and road changes as they come, lots and zones once
    /// each (Godot kept a flag per signal). Republishes the roads if they changed.
    fn relay(&mut self, events: &mut Vec<RunnerEvent>) {
        let sim_events = self.sim.as_mut().expect("idle").take_view_events();
        let (mut lots, mut zones, mut roads_changed) = (false, false, false);
        for event in sim_events {
            match event {
                SimEvent::HourChanged(hour) => events.push(RunnerEvent::HourChanged(hour)),
                SimEvent::DayStarted(day) => events.push(RunnerEvent::DayStarted(day)),
                SimEvent::RoadsChanged(change) => {
                    roads_changed = true;
                    events.push(RunnerEvent::RoadNetworkChanged {
                        node_ids: change.node_ids,
                        segment_ids: change.segment_ids,
                    });
                }
                SimEvent::ExternalsChanged => events.push(RunnerEvent::ExternalsChanged),
                SimEvent::LotsChanged => lots = true,
                SimEvent::ZonesChanged => zones = true,
                // Not relayed: the sim's own systems listen to these.
                SimEvent::VehicleArrived(_)
                | SimEvent::VehicleFailed(_)
                | SimEvent::PedestrianArrived(_)
                | SimEvent::RiderArrived { .. }
                | SimEvent::RiderStranded(_)
                | SimEvent::TripEnded { .. } => {}
            }
        }
        if lots {
            events.push(RunnerEvent::LotsChanged);
        }
        if zones {
            events.push(RunnerEvent::ZonesChanged);
        }
        if roads_changed {
            self.publish_roads();
        }
    }

    fn publish_roads(&mut self) {
        self.roads_version += 1;
        self.roads = Arc::new(RoadsView {
            version: self.roads_version,
            roads: self.sim.as_ref().expect("idle").roads.clone(),
        });
    }

    /// Runs (or, threaded, starts running) the ticks that keep the simulation
    /// `LEAD_TICKS` ahead of the screen.
    fn start_batch(&mut self) {
        if self.busy {
            return;
        }
        let sim = self.sim.as_ref().expect("idle");
        let behind =
            self.render_time + f64::from(LEAD_TICKS) * Simulation::TICK_DT - sim.sim_time();
        let count = ((behind / Simulation::TICK_DT - 0.001).ceil().max(0.0) as usize)
            .min(MAX_TICKS_PER_BATCH);
        if count == 0 {
            return;
        }
        let sim = self.sim.take().expect("idle");
        match &self.worker {
            Some(worker) => {
                worker
                    .jobs
                    .as_ref()
                    .expect("worker running")
                    .send(Job { sim, count })
                    .expect("the simulation worker is alive");
                self.busy = true;
            }
            None => self.collect(run_batch(Job { sim, count })),
        }
    }

    /// The batch is over: its snapshots become available to the views.
    fn collect(&mut self, done: Done) {
        self.busy = false;
        self.sim = Some(done.sim);
        self.snapshots.extend(done.snapshots);
        for ms in done.tick_ms {
            self.average_tick_ms += (ms - self.average_tick_ms) * 0.05;
        }
        for ms in done.capture_ms {
            self.average_capture_ms += (ms - self.average_capture_ms) * 0.05;
        }
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
                panic!("the simulation worker died")
            }
        }
    }

    /// Lets a running batch finish (so the simulation can be used from the main thread).
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
            .expect("the simulation worker died");
        self.collect(done);
    }

    /// Picks the snapshots on either side of the render clock, dropping older ones.
    fn update_view(&mut self) {
        while self.snapshots.len() > 2 && self.snapshots[1].time <= self.render_time {
            self.snapshots.pop_front();
        }
        self.view_from = self.snapshots[0].clone();
        self.view_to = self.snapshots.get(1).unwrap_or(&self.snapshots[0]).clone();
        let span = self.view_to.time - self.view_from.time;
        self.view_alpha = if span > 0.0 {
            ((self.render_time - self.view_from.time) / span).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
    }
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        // A batch in flight finishes first, so the worker isn't left holding the sim.
        if self.busy {
            let _ = self
                .worker
                .as_ref()
                .and_then(|w| w.done.lock().ok()?.recv().ok());
        }
    }
}
