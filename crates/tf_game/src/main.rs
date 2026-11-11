//! Transit Flow: the Bevy app. Renders the simulation (`tf_sim`) with simple shapes and
//! turns keyboard and mouse input into sim commands. So far (M8.3) it opens a window
//! with the map camera and the static map views (M9.1), and runs the simulation in the background.
//! With `--scenario ... --screenshot ...` it is the headless screenshot tool (`capture`).
//! See `docs/BEVY_MIGRATION_PLAN.md`.

use bevy::prelude::*;
use bevy::window::WindowResolution;
use tf_game::camera::MapCamera;
use tf_game::capture::{self, Cli, WINDOW_SIZE};
use tf_game::plugin::GamePlugin;

/// Behind everything the map doesn't cover.
const BACKGROUND: Color = Color::srgb(0.11, 0.12, 0.14);

fn main() {
    let cli = match capture::parse_args(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("error: {message}\n{}", capture::USAGE);
            std::process::exit(2);
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
    if let Cli::Capture(options) = cli {
        app.add_plugins(capture::CapturePlugin(options));
    }
    app.run();
}
