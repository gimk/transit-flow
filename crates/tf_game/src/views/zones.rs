//! Zoned cells that have no building yet (`view/zone_renderer.gd`): a light tint of the
//! zone colour with an outline, on what the roads leave of the cell (`compute_plot`).
//! Cells waiting for a road, or for room, get a red outline round the whole cell so the
//! player sees why nothing grows there. One entity per cell, redrawn only when it changed.
//!
//! Rebuilt on `ZonesChanged`, `LotsChanged` (a lot covers its cell's zone), `RoadNetworkChanged`
//! (plots move) and `SimulationReset`; cells are compared with what is on screen.

use std::collections::HashMap;

use bevy::prelude::*;
use glam::IVec2;
use tf_sim::city::{LotSystem, LotType};
use tf_sim::geom::Rect2;
use tf_sim::grid;

use super::lots::{bevy_rect, zone_color};
use super::{ViewMaterial, spawn_shape, z};
use crate::draw::ShapeBuilder;
use crate::events::{LotsChanged, RoadNetworkChanged, SimulationReset, ZonesChanged};
use crate::runner::SimHandle;

pub const FILL_ALPHA: f32 = 0.22;
pub const OUTLINE_ALPHA: f32 = 0.7;
pub const NO_ROAD_COLOR: Color = Color::srgba(1.0, 0.35, 0.3, 0.8);

#[derive(Clone, Debug, PartialEq)]
pub struct ZoneShape {
    pub zone: LotType,
    /// The free land of the cell, or the whole cell when the roads leave none.
    pub rect: Rect2,
    pub waiting_for_road: bool,
}

pub fn zone_shapes(shape: &ZoneShape, cell: IVec2) -> ShapeBuilder {
    let color = zone_color(shape.zone);
    let (min, max) = bevy_rect(shape.rect);
    let mut out = ShapeBuilder::new();
    out.rect(min, max, color.with_alpha(FILL_ALPHA))
        .rect_outline(min, max, 1.0, color.with_alpha(OUTLINE_ALPHA));
    if shape.waiting_for_road {
        let cell_rect = Rect2::new(
            cell.as_vec2() * grid::CELL_SIZE as f32,
            Vec2::splat(grid::CELL_SIZE as f32),
        )
        .grow(-6.0);
        let (min, max) = bevy_rect(cell_rect);
        out.rect_outline(min, max, 1.5, NO_ROAD_COLOR);
    }
    out
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoneView(pub IVec2);

#[derive(Resource, Default, Debug)]
pub struct ZoneViews {
    cells: HashMap<IVec2, (ZoneShape, Entity)>,
    pub built: usize,
}

impl ZoneViews {
    pub fn entity(&self, cell: IVec2) -> Option<Entity> {
        self.cells.get(&cell).map(|(_, e)| *e)
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

pub struct ZoneViewPlugin;

impl Plugin for ZoneViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ZoneViews>()
            .add_systems(Update, update_zones);
    }
}

#[allow(clippy::too_many_arguments)]
fn update_zones(
    mut resets: MessageReader<SimulationReset>,
    mut zone_changes: MessageReader<ZonesChanged>,
    mut lot_changes: MessageReader<LotsChanged>,
    mut road_changes: MessageReader<RoadNetworkChanged>,
    mut handle: ResMut<SimHandle>,
    mut views: ResMut<ZoneViews>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
    mut built: Local<bool>,
) {
    let reset = resets.read().count() > 0 || !*built;
    let changed =
        zone_changes.read().count() + lot_changes.read().count() + road_changes.read().count() > 0;
    *built = true;
    if !reset && !changed {
        return;
    }
    if reset {
        for (_, (_, entity)) in views.cells.drain() {
            commands.entity(entity).despawn();
        }
    }
    let sim = handle.get_mut();
    let mut wanted: HashMap<IVec2, ZoneShape> = HashMap::new();
    for (&cell, &zone) in &sim.zones.zones {
        if sim.lots.lot_at_cell(cell).is_some() {
            continue;
        }
        let plot = LotSystem::compute_plot(&sim.roads, cell);
        let rect = if plot.has_area() {
            plot
        } else {
            Rect2::new(
                cell.as_vec2() * grid::CELL_SIZE as f32,
                Vec2::splat(grid::CELL_SIZE as f32),
            )
        };
        wanted.insert(
            cell,
            ZoneShape {
                zone,
                rect,
                waiting_for_road: sim.zones.is_waiting_for_road(cell),
            },
        );
    }
    let gone: Vec<IVec2> = views
        .cells
        .keys()
        .filter(|c| !wanted.contains_key(c))
        .copied()
        .collect();
    for cell in gone {
        if let Some((_, entity)) = views.cells.remove(&cell) {
            commands.entity(entity).despawn();
        }
    }
    for (cell, shape) in wanted {
        if views.cells.get(&cell).is_some_and(|(old, _)| *old == shape) {
            continue;
        }
        if let Some((_, old)) = views.cells.remove(&cell) {
            commands.entity(old).despawn();
        }
        let entity = spawn_shape(
            &mut commands,
            &mut meshes,
            &material,
            zone_shapes(&shape, cell),
            z::ZONES,
        );
        commands.entity(entity).insert(ZoneView(cell));
        views.built += 1;
        views.cells.insert(cell, (shape, entity));
    }
}
