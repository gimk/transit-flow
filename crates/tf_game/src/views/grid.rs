//! The building grid (`view/grid_renderer.gd`): lines every cell, heavier every
//! `MAJOR_EVERY`, shown while a tool that snaps to the grid is active. M10's tools set the
//! `ShowGrid` flag (off by default). Minor lines hide when zoomed out far enough that they
//! would only be noise.

use bevy::prelude::*;
use tf_sim::grid;

use super::{ViewMaterial, spawn_shape, z};
use crate::camera::MapCamera;
use crate::draw::ShapeBuilder;

pub const MAJOR_EVERY: i32 = 8;
pub const MIN_MINOR_SPACING_PX: f32 = 12.0;
pub const MINOR_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
pub const MAJOR_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.26);

/// Whether the grid shows. A snapping tool sets it while it is active (M10).
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShowGrid(pub bool);

#[derive(Component)]
pub struct GridLines {
    pub major: bool,
}

pub struct GridViewPlugin;

impl Plugin for GridViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowGrid>()
            .add_systems(Startup, spawn_grid)
            .add_systems(Update, update_grid_visibility);
    }
}

fn spawn_grid(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
) {
    let size = grid::map_rect().size;
    let cell = grid::CELL_SIZE as f32;
    let mut minor = ShapeBuilder::new();
    let mut major = ShapeBuilder::new();
    for i in 0..=grid::MAP_SIZE_CELLS.x {
        let x = i as f32 * cell;
        let (shape, color, width) = if i % MAJOR_EVERY == 0 {
            (&mut major, MAJOR_COLOR, 1.5)
        } else {
            (&mut minor, MINOR_COLOR, 1.0)
        };
        shape.line(Vec2::new(x, 0.0), Vec2::new(x, -size.y), width, color);
    }
    for j in 0..=grid::MAP_SIZE_CELLS.y {
        let y = -(j as f32) * cell;
        let (shape, color, width) = if j % MAJOR_EVERY == 0 {
            (&mut major, MAJOR_COLOR, 1.5)
        } else {
            (&mut minor, MINOR_COLOR, 1.0)
        };
        shape.line(Vec2::new(0.0, y), Vec2::new(size.x, y), width, color);
    }
    for (shape, is_major) in [(minor, false), (major, true)] {
        let entity = spawn_shape(&mut commands, &mut meshes, &material, shape, z::GRID);
        commands
            .entity(entity)
            .insert((GridLines { major: is_major }, Visibility::Hidden));
    }
}

fn update_grid_visibility(
    show: Res<ShowGrid>,
    camera: Query<&MapCamera>,
    mut lines: Query<(&GridLines, &mut Visibility)>,
) {
    let zoom = camera.single().map_or(1.0, |c| c.zoom);
    let minor_ok = grid::CELL_SIZE as f32 * zoom >= MIN_MINOR_SPACING_PX;
    for (lines, mut visibility) in &mut lines {
        let on = show.0 && (lines.major || minor_ok);
        let wanted = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}
