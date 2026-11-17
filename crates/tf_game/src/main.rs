//! Transit Flow: the Bevy app. Opens a level of the street game (`play`): the player
//! designs the street, starts the run and reads the report. With `--frames N --screenshot
//! out.png` it is the headless screenshot tool (`capture`).

use bevy::prelude::*;
use bevy::window::WindowResolution;
use tf_game::camera::MapCamera;
use tf_game::capture::{self, Cli, WINDOW_SIZE};
use tf_game::play::PlayPlugin;
use tf_game::plugin::GamePlugin;
use tf_game::street_runner::default_threaded;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match capture::parse_args(args.iter().cloned()) {
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
    if let Cli::Capture(options) = &cli {
        // Fixed size and scale so screenshots compare; the run steps inline so it repeats.
        window.resolution =
            WindowResolution::new(WINDOW_SIZE.0, WINDOW_SIZE.1).with_scale_factor_override(1.0);
        window.resizable = false;
        if let Some(zoom) = options.zoom {
            camera.zoom = zoom;
        }
    }
    let play_level = match &cli {
        Cli::Play => {
            let first = tf_sim::street::level::level_ids().next();
            Some((
                first.expect("the game ships a level").to_string(),
                default_threaded(),
            ))
        }
        Cli::PlayLevel(id) => Some((id.clone(), default_threaded())),
        Cli::Capture(options) if options.play => {
            let capture::Target::Level(id) = &options.target;
            Some((id.clone(), false))
        }
        Cli::Capture(_) => None,
    };
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(window),
        ..default()
    }))
    .insert_resource(ClearColor(tf_game::views::palette::OUTSIDE))
    .add_plugins(GamePlugin)
    .add_plugins(tf_game::views::ViewsPlugin)
    .add_systems(Startup, move |mut commands: Commands| {
        commands.spawn(camera.clone());
    });
    if let Some((level, threaded)) = play_level {
        app.add_plugins(PlayPlugin { level, threaded });
    }
    if let Cli::Capture(options) = cli {
        app.add_plugins(capture::CapturePlugin(options));
    }
    app.run();
}
