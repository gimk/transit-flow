//! Headless screenshot mode (`transit-flow --scenario town --frames 120 --screenshot out.png`):
//! the counterpart of Godot's `--write-movie`. It builds a scenario, optionally runs the
//! sim ahead to an hour of the day, renders N frames at a fixed 1/60 s step (so the result
//! is deterministic), saves the window as a PNG and exits. macOS can't render without a
//! window, so a real 1280x720 window opens for a moment.
//!
//! `--overlay heatmap|lanes` sends `DebugOverlayToggleRequested` (once, on frame 1); the
//! overlays arrive in M9 and only have to listen for it.

use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use tf_sim::Simulation;
use tf_sim::clock::SimClock;

use crate::camera::{ZOOM_LEVELS, nearest_level};
use crate::events::DebugOverlayToggleRequested;
use crate::game::{self, Announce};
use crate::runner::SimHandle;

/// Window size in this mode (logical px, scale factor forced to 1) so PNGs compare.
pub const WINDOW_SIZE: (u32, u32) = (1280, 720);
/// Frames to wait for the GPU read-back before giving up.
pub const CAPTURE_TIMEOUT_FRAMES: u32 = 600;
pub const FRAME_DT: Duration = Duration::from_micros(16_667);

pub const USAGE: &str = "usage: transit-flow [--scenario <empty|bottleneck|bottleneck-bus|town> --frames N --screenshot PATH [--hour H] [--zoom Z] [--overlay <heatmap|lanes>]]
  no arguments: run the game
  --scenario    map to load (empty = the start map)
  --frames      frames to run before the screenshot (>= 1, at a fixed 1/60 s each)
  --screenshot  PNG file to write; the program then exits
  --hour        advance the sim to this hour of the day (0-24) before the first frame
  --zoom        camera zoom (nearest of 0.25 0.375 0.5 0.75 1 1.5 2 3 4); default: as the F-key frames it
  --overlay     toggle a debug overlay on (heatmap, lanes)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Empty,
    Bottleneck,
    BottleneckBus,
    Town,
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct CaptureOptions {
    pub scenario: Scenario,
    pub frames: u32,
    pub screenshot: PathBuf,
    pub hour: Option<f64>,
    /// Already snapped to one of `ZOOM_LEVELS`.
    pub zoom: Option<f32>,
    pub overlay: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cli {
    /// Normal game.
    Play,
    Capture(CaptureOptions),
}

/// Parses the arguments after the program name.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut args = args.into_iter();
    let (mut scenario, mut frames, mut screenshot) = (None, None, None);
    let (mut hour, mut zoom, mut overlay) = (None, None, None);
    let mut any = false;
    while let Some(flag) = args.next() {
        any = true;
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match flag.as_str() {
            "--scenario" => {
                scenario = Some(match value("--scenario")?.as_str() {
                    "empty" => Scenario::Empty,
                    "bottleneck" => Scenario::Bottleneck,
                    "bottleneck-bus" => Scenario::BottleneckBus,
                    "town" => Scenario::Town,
                    other => return Err(format!("unknown scenario '{other}'")),
                });
            }
            "--frames" => {
                let text = value("--frames")?;
                match text.parse::<u32>() {
                    Ok(n) if n >= 1 => frames = Some(n),
                    _ => {
                        return Err(format!(
                            "--frames must be a whole number >= 1, got '{text}'"
                        ));
                    }
                }
            }
            "--screenshot" => screenshot = Some(PathBuf::from(value("--screenshot")?)),
            "--hour" => {
                let text = value("--hour")?;
                match text.parse::<f64>() {
                    Ok(h) if (0.0..=24.0).contains(&h) => hour = Some(h),
                    _ => return Err(format!("--hour must be between 0 and 24, got '{text}'")),
                }
            }
            "--zoom" => {
                let text = value("--zoom")?;
                match text.parse::<f32>() {
                    Ok(z) if z.is_finite() && z > 0.0 => {
                        zoom = Some(ZOOM_LEVELS[nearest_level(z)]);
                    }
                    _ => return Err(format!("--zoom must be a positive number, got '{text}'")),
                }
            }
            "--overlay" => {
                overlay = Some(match value("--overlay")?.as_str() {
                    "heatmap" => "heatmap",
                    "lanes" => "lanes",
                    other => return Err(format!("unknown overlay '{other}'")),
                });
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    if !any {
        return Ok(Cli::Play);
    }
    Ok(Cli::Capture(CaptureOptions {
        scenario: scenario.ok_or("--scenario is required")?,
        frames: frames.ok_or("--frames is required")?,
        screenshot: screenshot.ok_or("--screenshot is required")?,
        hour,
        zoom,
        overlay,
    }))
}

/// What to do on a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Run,
    /// Take the screenshot now.
    Capture,
    /// The screenshot never arrived.
    GiveUp,
}

/// Counts frames: `frames` of running, then the capture, then a wait for it.
#[derive(Resource, Debug)]
pub struct FrameCounter {
    pub frames: u32,
    pub done: u32,
    pub captured: bool,
}

impl FrameCounter {
    pub fn new(frames: u32) -> Self {
        Self {
            frames,
            done: 0,
            captured: false,
        }
    }

    /// Called once per frame.
    pub fn tick(&mut self) -> Step {
        self.done += 1;
        if self.done < self.frames {
            Step::Run
        } else if !self.captured {
            self.captured = true;
            Step::Capture
        } else if self.done - self.frames > CAPTURE_TIMEOUT_FRAMES {
            Step::GiveUp
        } else {
            Step::Run
        }
    }
}

/// Steps `sim` until the clock reads `hour` (forwards, wrapping past midnight).
pub fn advance_to_hour(sim: &mut Simulation, hour: f64) {
    let now = sim.clock.minute_of_day();
    let wait = (hour * 60.0 - now).rem_euclid(f64::from(SimClock::MINUTES_PER_DAY));
    let target = sim.clock.total_minutes + wait;
    while sim.clock.total_minutes < target {
        sim.step();
    }
}

pub struct CapturePlugin(pub CaptureOptions);

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone())
            .insert_resource(FrameCounter::new(self.0.frames))
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME_DT))
            .add_systems(Startup, setup_scenario)
            .add_systems(Update, count_frames);
    }
}

fn setup_scenario(
    options: Res<CaptureOptions>,
    mut handle: ResMut<SimHandle>,
    mut announce: Announce,
) {
    match options.scenario {
        Scenario::Empty => game::new_empty_game(&mut handle, &mut announce),
        Scenario::Bottleneck => game::load_bottleneck_scenario(&mut handle, &mut announce),
        Scenario::BottleneckBus => {
            game::load_bottleneck_bus_lanes_scenario(&mut handle, &mut announce)
        }
        Scenario::Town => game::load_town_scenario(&mut handle, &mut announce),
    }
    if let Some(hour) = options.hour {
        // Stepped directly (no rendering): take the sim out, run it, put it back, which
        // also resyncs the snapshots and tells the views to reset.
        let mut sim = std::mem::replace(handle.get_mut(), Simulation::new());
        advance_to_hour(&mut sim, hour);
        handle.replace(sim);
    }
}

fn count_frames(
    options: Res<CaptureOptions>,
    mut counter: ResMut<FrameCounter>,
    mut commands: Commands,
    mut overlays: MessageWriter<DebugOverlayToggleRequested>,
    mut exit: MessageWriter<AppExit>,
) {
    if counter.done == 1
        && let Some(overlay) = options.overlay
    {
        overlays.write(DebugOverlayToggleRequested(overlay.into()));
    }
    match counter.tick() {
        Step::Run => {}
        Step::Capture => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(options.screenshot.clone()))
                .observe(
                    |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                        info!("screenshot saved");
                        exit.write(AppExit::Success);
                    },
                );
        }
        Step::GiveUp => {
            error!("the screenshot never arrived");
            exit.write(AppExit::error());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_plays() {
        assert_eq!(parse(&[]), Ok(Cli::Play));
    }

    #[test]
    fn full_command_parses() {
        let cli = parse(&[
            "--scenario",
            "bottleneck-bus",
            "--frames",
            "30",
            "--screenshot",
            "a.png",
            "--hour",
            "8.5",
            "--zoom",
            "0.6",
            "--overlay",
            "lanes",
        ])
        .unwrap();
        let Cli::Capture(o) = cli else { panic!() };
        assert_eq!(o.scenario, Scenario::BottleneckBus);
        assert_eq!(o.frames, 30);
        assert_eq!(o.screenshot, PathBuf::from("a.png"));
        assert_eq!(o.hour, Some(8.5));
        assert_eq!(o.zoom, Some(0.5));
        assert_eq!(o.overlay, Some("lanes"));
    }

    #[test]
    fn minimal_command_leaves_options_unset() {
        let cli = parse(&[
            "--scenario",
            "town",
            "--frames",
            "1",
            "--screenshot",
            "t.png",
        ]);
        let Ok(Cli::Capture(o)) = cli else { panic!() };
        assert_eq!((o.hour, o.zoom, o.overlay), (None, None, None));
    }

    #[test]
    fn bad_arguments_are_errors() {
        for args in [
            &["--scenario", "town"][..],
            &["--frames", "5", "--screenshot", "a.png"],
            &[
                "--scenario",
                "mars",
                "--frames",
                "5",
                "--screenshot",
                "a.png",
            ],
            &[
                "--scenario",
                "town",
                "--frames",
                "0",
                "--screenshot",
                "a.png",
            ],
            &[
                "--scenario",
                "town",
                "--frames",
                "x",
                "--screenshot",
                "a.png",
            ],
            &["--scenario", "town", "--frames", "5", "--screenshot"],
            &[
                "--scenario",
                "town",
                "--frames",
                "5",
                "--screenshot",
                "a",
                "--hour",
                "25",
            ],
            &[
                "--scenario",
                "town",
                "--frames",
                "5",
                "--screenshot",
                "a",
                "--overlay",
                "x",
            ],
            &["--bogus"],
            &["--help"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn counter_captures_once_after_n_frames_then_gives_up() {
        let mut counter = FrameCounter::new(3);
        assert_eq!(counter.tick(), Step::Run);
        assert_eq!(counter.tick(), Step::Run);
        assert_eq!(counter.tick(), Step::Capture);
        for _ in 0..CAPTURE_TIMEOUT_FRAMES {
            assert_eq!(counter.tick(), Step::Run);
        }
        assert_eq!(counter.tick(), Step::GiveUp);
    }

    #[test]
    fn single_frame_captures_at_once() {
        assert_eq!(FrameCounter::new(1).tick(), Step::Capture);
    }

    #[test]
    fn advancing_to_an_hour_reaches_it_and_wraps_past_midnight() {
        let mut sim = Simulation::new();
        advance_to_hour(&mut sim, 8.0);
        assert!((sim.clock.time_of_day_hours() - 8.0).abs() < 0.01);
        advance_to_hour(&mut sim, 2.0);
        assert!((sim.clock.time_of_day_hours() - 2.0).abs() < 0.01);
        assert_eq!(sim.clock.day(), 2);
    }
}
