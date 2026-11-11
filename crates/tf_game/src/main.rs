//! Transit Flow: the Bevy app. Renders the simulation (`tf_sim`) with simple shapes and
//! turns keyboard and mouse input into sim commands. So far (M8.2) it opens a window
//! with the map camera and runs the simulation in the background; the views come in M9.
//! See `docs/BEVY_MIGRATION_PLAN.md`.

use bevy::prelude::*;
use tf_game::camera::MapCamera;
use tf_game::plugin::GamePlugin;

/// Behind everything the map doesn't cover.
const BACKGROUND: Color = Color::srgb(0.11, 0.12, 0.14);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Transit Flow".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(GamePlugin::default())
        .insert_resource(ClearColor(BACKGROUND))
        .add_systems(Startup, spawn_camera)
        .run();
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn(MapCamera::default());
}
