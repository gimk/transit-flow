//! Frame-time benchmark (`transit-flow --bench-frames 600 [--bench-vehicles 2000]`): loads
//! the Town, sets the debug stress traffic to N vehicles (at the fastest speed), waits until
//! they are up, then runs at normal speed and logs the average and worst frame time over the
//! given number of frames, and exits. Meant for a release build with the threaded runner;
//! `main` turns vsync off in this mode so the numbers are the real cost, not the display's
//! refresh interval.

use bevy::prelude::*;

use crate::events::DebugOverlayToggleRequested;
use crate::game::{self, Announce};
use crate::runner::SimHandle;

pub const USAGE: &str = "       transit-flow --bench-frames N [--bench-vehicles V] [--bench-overlay heatmap|lanes]
  --bench-frames    Town + stress traffic, no vsync: log the frame time over N frames (release build)
  --bench-vehicles  stress vehicles to wait for (default 2000)
  --bench-overlay   switch on a debug overlay first (heatmap, lanes)";
/// Real seconds to wait for the stress traffic to fill up before measuring anyway.
const WARMUP_TIMEOUT: f64 = 120.0;
/// Frames dropped after the warm-up (the first frames after a speed change).
const SETTLE_FRAMES: u32 = 60;

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct BenchOptions {
    pub frames: u32,
    pub vehicles: usize,
    /// A debug overlay to switch on first (`heatmap` or `lanes`), to measure its cost.
    pub overlay: Option<&'static str>,
}

/// `Some` if the arguments ask for the benchmark (`Err` if they are malformed).
pub fn parse_args(args: &[String]) -> Option<Result<BenchOptions, String>> {
    if !args.iter().any(|a| a.starts_with("--bench-")) {
        return None;
    }
    let mut options = BenchOptions {
        frames: 0,
        vehicles: 2000usize,
        overlay: None,
    };
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        if flag == "--bench-overlay" {
            options.overlay = match it.next().map(String::as_str) {
                Some("heatmap") => Some("heatmap"),
                Some("lanes") => Some("lanes"),
                _ => return Some(Err("--bench-overlay takes heatmap or lanes".into())),
            };
            continue;
        }
        let value = it.next().and_then(|v| v.parse::<u32>().ok());
        match (flag.as_str(), value) {
            ("--bench-frames", Some(n)) if n >= 1 => options.frames = n,
            ("--bench-vehicles", Some(n)) => options.vehicles = n as usize,
            _ => return Some(Err(format!("bad benchmark argument '{flag}'"))),
        }
    }
    (options.frames > 0)
        .then_some(Ok(options))
        .or(Some(Err("--bench-frames is required".into())))
}

#[derive(Resource, Default)]
struct Run {
    started: Option<f64>,
    settled: u32,
    frames: Vec<f64>,
    measuring: bool,
    /// When this frame's update began (main-thread work: everything between First and Last).
    update_start: Option<std::time::Instant>,
    updates: Vec<f64>,
}

pub struct BenchPlugin(pub BenchOptions);

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0)
            .init_resource::<Run>()
            .add_systems(Startup, setup)
            .add_systems(First, |mut run: ResMut<Run>| {
                run.update_start = Some(std::time::Instant::now())
            })
            .add_systems(Last, measure);
    }
}

fn setup(
    options: Res<BenchOptions>,
    mut handle: ResMut<SimHandle>,
    mut announce: Announce,
    mut overlays: MessageWriter<DebugOverlayToggleRequested>,
) {
    if let Some(overlay) = options.overlay {
        overlays.write(DebugOverlayToggleRequested(overlay.into()));
    }
    game::load_town_scenario(&mut handle, &mut announce);
    handle.get_mut().stress.target_count = options.vehicles;
    handle.set_speed(3);
}

fn measure(
    options: Res<BenchOptions>,
    time: Res<Time<Real>>,
    mut handle: ResMut<SimHandle>,
    mut run: ResMut<Run>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs_f64();
    let started = *run.started.get_or_insert(now);
    if !run.measuring {
        let up = handle.view_to.vehicle_count as f64 >= options.vehicles as f64 * 0.95;
        if up || now - started > WARMUP_TIMEOUT {
            run.settled += 1;
            if run.settled == 1 {
                handle.set_speed(1);
            }
            run.measuring = run.settled > SETTLE_FRAMES;
        }
        return;
    }
    run.frames.push(time.delta_secs_f64() * 1000.0);
    if let Some(start) = run.update_start {
        run.updates.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    if run.frames.len() as u32 >= options.frames {
        let n = run.frames.len() as f64;
        let average = run.frames.iter().sum::<f64>() / n;
        let worst = run.frames.iter().copied().fold(0.0, f64::max);
        let mut sorted = run.frames.clone();
        sorted.sort_by(f64::total_cmp);
        let update_average = run.updates.iter().sum::<f64>() / run.updates.len().max(1) as f64;
        let update_worst = run.updates.iter().copied().fold(0.0, f64::max);
        let p99 = sorted[((sorted.len() as f64 * 0.99) as usize).min(sorted.len() - 1)];
        println!(
            "bench: {} frames, {} vehicles (+{} waiting), average {average:.2} ms ({:.0} FPS), p99 {p99:.2} ms, worst {worst:.2} ms; main-thread update (First to Last) average {update_average:.2} ms, worst {update_worst:.2} ms; sim tick {:.2} ms + snapshot capture {:.2} ms, lagging: {}",
            run.frames.len(),
            handle.view_to.vehicle_count,
            handle.view_to.waiting_count,
            1000.0 / average,
            handle.average_tick_ms,
            handle.average_capture_ms,
            handle.sim_lagging,
        );
        exit.write(AppExit::Success);
    }
}
