//! Headless screenshot mode (`transit-flow --play carrefour-ecole --frames 120 --screenshot
//! out.png`): it opens a level, renders N frames at a fixed 1/60 s step (so the result is
//! deterministic), saves the window as a PNG and exits. macOS can't render without a
//! window, so a real 1280x720 window opens for a moment.
//!
//! `--play <id>` shows play mode on a level; with `--run` it presses Enter once the window
//! is up, so the screenshot shows the run. `--level <id>` shows the level without play
//! mode; with `--run` a street run of the level starts before the first frame, on the
//! inline `StreetRunner`, and `--seed` and `--population` set its options.

use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use tf_sim::street::run::agent::Population;
use tf_sim::street::run::{RunOptions, StreetRun};
use tf_sim::street::{Design, LevelFile};

use crate::camera::{ZOOM_LEVELS, nearest_level};
use crate::level::LoadedLevel;
use crate::play::{self, PlayMode, PlayPhase, PlayState};
use crate::street_runner::StreetRunner;

/// Window size in this mode (logical px, scale factor forced to 1) so PNGs compare.
pub const WINDOW_SIZE: (u32, u32) = (1280, 720);
/// Frames to wait for the GPU read-back before giving up.
pub const CAPTURE_TIMEOUT_FRAMES: u32 = 600;
pub const FRAME_DT: Duration = Duration::from_micros(16_667);

pub const USAGE: &str = "usage: transit-flow [--play <id> [--frames N --screenshot PATH [--run] [--zoom Z]]] [--level <id> --frames N --screenshot PATH [--zoom Z] [--run [--seed N] [--population <careful|mixed|careless>]]]
  no arguments: play the first level
  --play        open a level of the street game to design, run and read the report (carrefour-ecole);
                with --frames and --screenshot it is a screenshot of that screen, and --run starts the run first
  --level       show a level without play mode (carrefour-ecole)
  --frames      frames to run before the screenshot (>= 1, at a fixed 1/60 s each)
  --screenshot  PNG file to write; the program then exits
  --zoom        camera zoom (nearest of 0.25 0.375 0.5 0.75 1 1.5 2 3 4); default: framed on the level
  --run         with --level: start a run of the level (empty design) before the frames
  --seed        with --level --run: the run's seed (default: the level's)
  --population  with --level --run: careful, mixed (default) or careless drivers";

/// What the screenshot shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A level of the street game, by id.
    Level(String),
}

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct CaptureOptions {
    pub target: Target,
    pub frames: u32,
    pub screenshot: PathBuf,
    /// Already snapped to one of `ZOOM_LEVELS`.
    pub zoom: Option<f32>,
    /// Start a street run of the level.
    pub run: bool,
    /// Play mode on the level: `play::PlayPlugin` loads it, and `run` presses Enter once
    /// the window is up.
    pub play: bool,
    /// Only with `run`.
    pub seed: Option<u64>,
    pub population: Option<Population>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cli {
    /// Normal game: play mode on the first level.
    Play,
    /// Play mode on this level (`--play <id>`).
    PlayLevel(String),
    Capture(CaptureOptions),
}

fn known_level(id: String) -> Result<String, String> {
    if tf_sim::street::level::level_ids().any(|known| known == id) {
        Ok(id)
    } else {
        Err(format!("unknown level '{id}'"))
    }
}

/// Parses the arguments after the program name.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut args = args.into_iter();
    let (mut level, mut frames, mut screenshot) = (None, None, None);
    let mut zoom = None;
    let (mut run, mut seed, mut population) = (false, None, None);
    let mut play = None;
    let mut any = false;
    while let Some(flag) = args.next() {
        any = true;
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match flag.as_str() {
            "--level" => level = Some(known_level(value("--level")?)?),
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
            "--zoom" => {
                let text = value("--zoom")?;
                match text.parse::<f32>() {
                    Ok(z) if z.is_finite() && z > 0.0 => {
                        zoom = Some(ZOOM_LEVELS[nearest_level(z)]);
                    }
                    _ => return Err(format!("--zoom must be a positive number, got '{text}'")),
                }
            }
            "--play" => play = Some(known_level(value("--play")?)?),
            "--run" => run = true,
            "--seed" => {
                let text = value("--seed")?;
                seed = Some(
                    text.parse::<u64>()
                        .map_err(|_| format!("--seed must be a whole number, got '{text}'"))?,
                );
            }
            "--population" => {
                population = Some(match value("--population")?.as_str() {
                    "careful" => Population::Careful,
                    "mixed" => Population::Mixed,
                    "careless" => Population::Careless,
                    other => return Err(format!("unknown population '{other}'")),
                });
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    if !any {
        return Ok(Cli::Play);
    }
    if let Some(id) = play {
        if level.is_some() || seed.is_some() || population.is_some() {
            return Err("--play goes only with --frames, --screenshot, --run and --zoom".into());
        }
        if frames.is_none() && screenshot.is_none() {
            if run {
                return Err("--run needs --frames and --screenshot".into());
            }
            if zoom.is_some() {
                return Err("--zoom needs --frames and --screenshot".into());
            }
            return Ok(Cli::PlayLevel(id));
        }
        return Ok(Cli::Capture(CaptureOptions {
            target: Target::Level(id),
            frames: frames.ok_or("--frames is required")?,
            screenshot: screenshot.ok_or("--screenshot is required")?,
            zoom,
            run,
            seed: None,
            population: None,
            play: true,
        }));
    }
    let Some(level) = level else {
        return Err("--play or --level is required".into());
    };
    if !run && (seed.is_some() || population.is_some()) {
        return Err("--seed and --population need --run".into());
    }
    Ok(Cli::Capture(CaptureOptions {
        target: Target::Level(level),
        frames: frames.ok_or("--frames is required")?,
        screenshot: screenshot.ok_or("--screenshot is required")?,
        zoom,
        run,
        seed,
        population,
        play: false,
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

pub struct CapturePlugin(pub CaptureOptions);

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone())
            .insert_resource(FrameCounter::new(self.0.frames))
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME_DT))
            .add_systems(Startup, setup_level)
            .add_systems(Update, count_frames)
            .add_systems(Update, start_play_run.run_if(resource_exists::<PlayState>));
    }
}

fn setup_level(
    options: Res<CaptureOptions>,
    mut level: ResMut<LoadedLevel>,
    mut runner: ResMut<StreetRunner>,
) {
    let Target::Level(id) = &options.target;
    if options.play {
        // `play::PlayPlugin` loads the level and starts the run; the framing zoom is
        // the one thing it leaves to us.
        level.zoom = options.zoom;
        return;
    }
    level.street = Some(LoadedLevel::load(id).expect("level ids are checked on parsing"));
    level.zoom = options.zoom;
    if options.run {
        start_street_run(&mut runner, id, options.seed, options.population);
    }
}

/// Starts a run of level `id` with the empty design, inline (screenshots are fixed-step).
fn start_street_run(
    runner: &mut StreetRunner,
    id: &str,
    seed: Option<u64>,
    population: Option<Population>,
) {
    let file = LevelFile::load(id).expect("level ids are checked on parsing");
    let mut run_options = RunOptions::for_level(&file);
    if let Some(seed) = seed {
        run_options.seed = seed;
    }
    if let Some(population) = population {
        run_options.population = population;
    }
    let run = StreetRun::new(&file, &Design::new(&file.id), run_options)
        .expect("the empty design builds");
    runner.start(run, false);
}

/// `--play <id> --run`: presses Enter once, on the second frame, so the screenshot shows the run.
fn start_play_run(
    options: Res<CaptureOptions>,
    counter: Res<FrameCounter>,
    mode: Res<PlayMode>,
    mut state: ResMut<PlayState>,
    mut runner: ResMut<StreetRunner>,
    mut next: ResMut<NextState<PlayPhase>>,
    mut started: Local<bool>,
) {
    if !options.play || !options.run || *started || counter.done < 2 {
        return;
    }
    *started = true;
    play::begin_run(&mut state, &mode, &mut runner, &mut next);
}

fn count_frames(
    options: Res<CaptureOptions>,
    mut counter: ResMut<FrameCounter>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
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
    fn level_command_parses() {
        let cli = parse(&[
            "--level",
            "carrefour-ecole",
            "--frames",
            "3",
            "--screenshot",
            "l.png",
            "--zoom",
            "0.6",
        ]);
        let Ok(Cli::Capture(o)) = cli else { panic!() };
        assert_eq!(o.target, Target::Level("carrefour-ecole".into()));
        assert_eq!(o.frames, 3);
        assert_eq!(o.screenshot, PathBuf::from("l.png"));
        assert_eq!(o.zoom, Some(0.5));
        for args in [
            &[
                "--level",
                "nowhere",
                "--frames",
                "3",
                "--screenshot",
                "l.png",
            ][..],
            &["--level", "carrefour-ecole", "--scenario", "town"],
            &["--level", "carrefour-ecole", "--hour", "8"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn run_flags_parse_with_a_level_only() {
        let base = ["--frames", "3", "--screenshot", "l.png"];
        let with = |extra: &[&str]| {
            let mut args: Vec<&str> = extra.to_vec();
            args.extend(base);
            parse(&args)
        };
        let Ok(Cli::Capture(o)) = with(&[
            "--level",
            "carrefour-ecole",
            "--run",
            "--seed",
            "7",
            "--population",
            "careless",
        ]) else {
            panic!()
        };
        assert!(o.run);
        assert_eq!(o.seed, Some(7));
        assert_eq!(o.population, Some(Population::Careless));
        let Ok(Cli::Capture(o)) = with(&["--level", "carrefour-ecole", "--run"]) else {
            panic!()
        };
        assert_eq!((o.run, o.seed, o.population), (true, None, None));
        for extra in [
            &["--level", "carrefour-ecole", "--seed", "7"][..],
            &["--level", "carrefour-ecole", "--population", "mixed"],
            &["--level", "carrefour-ecole", "--run", "--seed", "x"],
            &["--level", "carrefour-ecole", "--run", "--population", "bad"],
        ] {
            assert!(with(extra).is_err(), "{extra:?}");
        }
    }

    #[test]
    fn bad_arguments_are_errors() {
        for args in [
            &["--level", "carrefour-ecole"][..],
            &["--frames", "5", "--screenshot", "a.png"],
            &[
                "--level",
                "carrefour-ecole",
                "--frames",
                "0",
                "--screenshot",
                "a.png",
            ],
            &[
                "--level",
                "carrefour-ecole",
                "--frames",
                "x",
                "--screenshot",
                "a.png",
            ],
            &[
                "--level",
                "carrefour-ecole",
                "--frames",
                "5",
                "--screenshot",
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
}
