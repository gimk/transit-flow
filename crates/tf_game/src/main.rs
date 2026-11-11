//! Transit Flow: the Bevy app. Renders the simulation (`tf_sim`) with simple shapes and
//! turns keyboard and mouse input into sim commands. For now (migration work package
//! M0.1) it only opens a window with a 2D camera; see `docs/BEVY_MIGRATION_PLAN.md`.

use bevy::prelude::*;

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
        .insert_resource(ClearColor(BACKGROUND))
        .add_systems(Startup, spawn_camera)
        .run();
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}
