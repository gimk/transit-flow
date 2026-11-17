//! Transit Flow: the Bevy app. Renders the simulation (`tf_sim`) with simple shapes and
//! turns keyboard and mouse input into sim commands. So far (M8.3) it opens a window
//! with the map camera and the static map views (M9.1), and runs the simulation in the background.
//! With `--scenario ... --screenshot ...` it is the headless screenshot tool (`capture`).
//! See `docs/BEVY_MIGRATION_PLAN.md`.

use bevy::prelude::*;
use bevy::window::WindowResolution;
use tf_game::bench::{self, BenchPlugin};
use tf_game::camera::MapCamera;
use tf_game::capture::{self, Cli, WINDOW_SIZE};
use tf_game::play::PlayPlugin;
use tf_game::plugin::GamePlugin;
use tf_game::runner::SimHandle;

/// Behind everything the map doesn't cover.
const BACKGROUND: Color = Color::srgb(0.11, 0.12, 0.14);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bench = match bench::parse_args(&args) {
        Some(Ok(options)) => Some(options),
        Some(Err(message)) => {
            eprintln!("error: {message}\n{}\n{}", capture::USAGE, bench::USAGE);
            std::process::exit(2);
        }
        None => None,
    };
    let cli = if bench.is_some() {
        Cli::Play
    } else {
        match capture::parse_args(args.iter().cloned()) {
            Ok(cli) => cli,
            Err(message) => {
                eprintln!("error: {message}\n{}", capture::USAGE);
                std::process::exit(2);
            }
        }
    };
    let mut window = Window {
        title: "Transit Flow".into(),
        ..default()
    };
    let mut camera = MapCamera::default();
    let mut game = GamePlugin::default();
    if let Cli::Capture(options) = &cli {
        // Fixed size and scale so screenshots compare; inline sim so runs are repeatable.
        window.resolution =
            WindowResolution::new(WINDOW_SIZE.0, WINDOW_SIZE.1).with_scale_factor_override(1.0);
        window.resizable = false;
        game.threaded = false;
        if let Some(zoom) = options.zoom {
            camera.zoom = zoom;
        }
    }
    let play_level = match &cli {
        Cli::PlayLevel(id) => Some((id.clone(), SimHandle::default_threaded())),
        Cli::Capture(options) if options.play => match &options.target {
            capture::Target::Level(id) => Some((id.clone(), false)),
            _ => None,
        },
        _ => None,
    };
    if bench.is_some() {
        window.present_mode = bevy::window::PresentMode::AutoNoVsync;
    }
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(window),
        ..default()
    }))
    .add_plugins(game)
    .add_plugins(tf_game::views::ViewsPlugin)
    .insert_resource(ClearColor(BACKGROUND))
    .add_systems(Startup, move |mut commands: Commands| {
        commands.spawn(camera.clone());
    });
    if let Some((level, threaded)) = play_level {
        app.add_plugins(PlayPlugin { level, threaded });
    }
    if let Some(options) = bench {
        app.add_plugins(BenchPlugin(options));
    }
    if let Cli::Capture(options) = cli {
        app.add_plugins(capture::CapturePlugin(options));
    }
    app.run();
}
