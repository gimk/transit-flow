//! The ground: one flat rectangle for the whole map (`view/grid_renderer.gd`'s `_draw`),
//! with a thin border. Built once; it doesn't depend on the sim.

use bevy::prelude::*;
use tf_sim::grid;

use super::{ViewMaterial, spawn_shape, z};
use crate::camera::sim_to_bevy;
use crate::draw::ShapeBuilder;

pub const GROUND_COLOR: Color = Color::srgb(0.17, 0.22, 0.18);
pub const BORDER_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.35);
pub const BORDER_WIDTH: f32 = 3.0;

#[derive(Component)]
pub struct Ground;

/// The thin border of the map (play mode removes it with the ground).
#[derive(Component)]
pub struct MapBorder;

pub struct GroundViewPlugin;

impl Plugin for GroundViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_ground);
    }
}

fn spawn_ground(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
) {
    let map = grid::map_rect();
    let (a, b) = (sim_to_bevy(map.position), sim_to_bevy(map.end()));
    let (min, max) = (a.min(b), a.max(b));
    let mut ground = ShapeBuilder::new();
    ground.rect(min, max, GROUND_COLOR);
    let entity = spawn_shape(&mut commands, &mut meshes, &material, ground, z::GROUND);
    commands.entity(entity).insert(Ground);
    // Drawn just inside the map edge so the camera limits show it.
    let mut border = ShapeBuilder::new();
    border.rect_outline(min, max, BORDER_WIDTH, BORDER_COLOR);
    let entity = spawn_shape(&mut commands, &mut meshes, &material, border, z::GRID);
    commands.entity(entity).insert(MapBorder);
}
