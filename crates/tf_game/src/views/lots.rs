//! Lots (`view/lot_renderer.gd`): a building at the back of its plot in the zone's colour
//! and a marked parking strip in front, on the side facing the road the lot is reached
//! from. Lots with no road access are dimmed with a red outline. One entity per lot; a lot
//! is redrawn only when its plot, its zone or its door changed.
//!
//! Rebuilt on `LotsChanged`, `RoadNetworkChanged` (plots and doors move with the roads) and
//! `SimulationReset`: `LotsChanged` doesn't say which lots, so each lot's layout is
//! compared with what is on screen. This reads the sim (`SimHandle::get_mut`), once per
//! change message, not per frame.
//!
//! Not drawn here: the parked cars (M9.2 draws them from `ViewSnapshot::parked`, in the
//! stalls `lot_layout` returns), building styles.

use std::collections::HashMap;

use bevy::prelude::*;
use tf_sim::city::citizen_system::RESIDENTS_PER_HOME;
use tf_sim::city::{LotType, Parking};
use tf_sim::geom::Rect2;
use tf_sim::ids::LotId;

use super::{ViewMaterial, spawn_shape, z};
use crate::camera::sim_to_bevy;
use crate::draw::ShapeBuilder;
use crate::events::{LotsChanged, RoadNetworkChanged, SimulationReset};
use crate::runner::SimHandle;

/// Zone colours (the GDScript's `LotRenderer.COLORS`), shared with the zone view.
pub fn zone_color(lot_type: LotType) -> Color {
    match lot_type {
        LotType::Residential => Color::srgb(0.298, 0.686, 0.314),
        LotType::Commercial => Color::srgb(0.259, 0.647, 0.961),
        LotType::Industrial => Color::srgb(0.976, 0.659, 0.145),
    }
}

pub const NO_ACCESS_COLOR: Color = Color::srgba(1.0, 0.3, 0.25, 0.9);
pub const NO_ACCESS_DIM: Color = Color::srgba(0.0, 0.0, 0.0, 0.5);
pub const PAVEMENT_COLOR: Color = Color::srgb(0.24, 0.25, 0.27);
pub const STALL_LINE_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.4);
/// Depth of the parking strip (a car is 11 px long) and the least a building keeps.
pub const PARKING_DEPTH: f32 = 13.0;
pub const MIN_BUILDING: f32 = 8.0;
/// Width of a stall (a car is 6 px wide), and the gap around buildings.
pub const STALL_WIDTH: f32 = 8.0;
pub const BUILDING_MARGIN: f32 = 2.0;

/// Where things go on a plot (sim coordinates, y down, like the sim's own).
#[derive(Clone, Debug, PartialEq)]
pub struct LotLayout {
    /// Unit vector from the building toward the road.
    pub front: Vec2,
    pub parking: Rect2,
    pub building: Rect2,
    /// One rectangle per stall that fits.
    pub stalls: Vec<Rect2>,
}

/// `LotRenderer.layout`: the parking strip along the plot's side facing `door` (a point on
/// the road lane the lot is reached from), the building in the rest, stalls along the strip.
pub fn lot_layout(plot: Rect2, door: Vec2, stall_count: usize) -> LotLayout {
    let to_door = door - plot.center();
    let mut front = if to_door.x.abs() > to_door.y.abs() {
        Vec2::new(to_door.x.signum(), 0.0)
    } else {
        Vec2::new(0.0, to_door.y.signum())
    };
    if front == Vec2::ZERO || front.x.is_nan() || front.y.is_nan() {
        front = Vec2::Y; // down, as the GDScript does
    }
    let horizontal = front.x != 0.0;
    let depth = if horizontal { plot.size.x } else { plot.size.y };
    let strip = PARKING_DEPTH.min(depth - MIN_BUILDING);
    let mut parking = Rect2::default();
    let mut building = plot;
    if strip > 0.0 {
        let end = plot.end();
        if front == Vec2::NEG_Y {
            parking = Rect2::new(plot.position, Vec2::new(plot.size.x, strip));
            building = Rect2::new(
                plot.position + Vec2::new(0.0, strip),
                plot.size - Vec2::new(0.0, strip),
            );
        } else if front == Vec2::Y {
            parking = Rect2::new(
                Vec2::new(plot.position.x, end.y - strip),
                Vec2::new(plot.size.x, strip),
            );
            building = Rect2::new(plot.position, plot.size - Vec2::new(0.0, strip));
        } else if front == Vec2::NEG_X {
            parking = Rect2::new(plot.position, Vec2::new(strip, plot.size.y));
            building = Rect2::new(
                plot.position + Vec2::new(strip, 0.0),
                plot.size - Vec2::new(strip, 0.0),
            );
        } else {
            parking = Rect2::new(
                Vec2::new(end.x - strip, plot.position.y),
                Vec2::new(strip, plot.size.y),
            );
            building = Rect2::new(plot.position, plot.size - Vec2::new(strip, 0.0));
        }
    }
    let mut stalls = Vec::new();
    if parking.has_area() {
        let length = if horizontal {
            parking.size.y
        } else {
            parking.size.x
        };
        let count = stall_count.min((length / STALL_WIDTH).floor() as usize);
        let start = (length - count as f32 * STALL_WIDTH) * 0.5;
        for i in 0..count {
            let along = start + i as f32 * STALL_WIDTH;
            stalls.push(if horizontal {
                Rect2::new(
                    parking.position + Vec2::new(0.0, along),
                    Vec2::new(parking.size.x, STALL_WIDTH),
                )
            } else {
                Rect2::new(
                    parking.position + Vec2::new(along, 0.0),
                    Vec2::new(STALL_WIDTH, parking.size.y),
                )
            });
        }
    }
    LotLayout {
        front,
        parking,
        building: building.grow(-BUILDING_MARGIN),
        stalls,
    }
}

/// Stalls a lot has: the lot's spaces at shops and factories, one per resident's car at homes.
pub fn stall_count(lot_type: LotType) -> usize {
    Parking::capacity_of(lot_type).unwrap_or(RESIDENTS_PER_HOME) as usize
}

/// A sim rectangle as Bevy corners.
pub fn bevy_rect(rect: Rect2) -> (Vec2, Vec2) {
    let (a, b) = (sim_to_bevy(rect.position), sim_to_bevy(rect.end()));
    (a.min(b), a.max(b))
}

/// What decides how a lot looks; two equal ones draw the same.
#[derive(Clone, Debug, PartialEq)]
pub struct LotShape {
    pub lot_type: LotType,
    pub plot: Rect2,
    /// A point of the lane in front of the lot, `None` without road access.
    pub door: Option<Vec2>,
}

/// The lot's shapes.
pub fn lot_shapes(shape: &LotShape) -> ShapeBuilder {
    let mut out = ShapeBuilder::new();
    let color = zone_color(shape.lot_type);
    let layout = lot_layout(
        shape.plot,
        shape.door.unwrap_or(shape.plot.center() + Vec2::Y),
        stall_count(shape.lot_type),
    );
    let outline_color = color.darker(0.45);
    let draw_building = |out: &mut ShapeBuilder, rect: Rect2| {
        let (min, max) = bevy_rect(rect);
        out.rect(min, max, color)
            .rect_outline(min, max, 1.0, outline_color);
    };
    if shape.door.is_none() {
        draw_building(&mut out, layout.building);
        let (min, max) = bevy_rect(layout.building);
        out.rect(min, max, NO_ACCESS_DIM)
            .rect_outline(min, max, 2.0, NO_ACCESS_COLOR);
        return out;
    }
    if layout.parking.has_area() {
        let (min, max) = bevy_rect(layout.parking);
        out.rect(min, max, PAVEMENT_COLOR);
    }
    for stall in &layout.stalls {
        // Dividers run front to back, along each stall's near edge.
        let (a, b) = if layout.front.x != 0.0 {
            (
                stall.position,
                stall.position + Vec2::new(stall.size.x, 0.0),
            )
        } else {
            (
                stall.position,
                stall.position + Vec2::new(0.0, stall.size.y),
            )
        };
        out.line(sim_to_bevy(a), sim_to_bevy(b), 0.8, STALL_LINE_COLOR);
    }
    draw_building(&mut out, layout.building);
    out
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LotView(pub LotId);

/// What is on screen.
#[derive(Resource, Default, Debug)]
pub struct LotViews {
    lots: HashMap<LotId, (LotShape, Entity)>,
    /// Goes up whenever a lot is drawn, redrawn or removed (parked cars follow it).
    pub revision: u64,
    /// Lots drawn anew so far (tests check an edit redraws only what changed).
    pub built: usize,
}

impl LotViews {
    /// Every lot on screen with the shape it was drawn from.
    pub fn shapes(&self) -> impl Iterator<Item = (LotId, &LotShape)> {
        self.lots.iter().map(|(id, (shape, _))| (*id, shape))
    }

    pub fn entity(&self, id: LotId) -> Option<Entity> {
        self.lots.get(&id).map(|(_, e)| *e)
    }

    pub fn len(&self) -> usize {
        self.lots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lots.is_empty()
    }
}

pub struct LotViewPlugin;

impl Plugin for LotViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LotViews>()
            .add_systems(Update, update_lots);
    }
}

#[allow(clippy::too_many_arguments)]
fn update_lots(
    mut resets: MessageReader<SimulationReset>,
    mut lot_changes: MessageReader<LotsChanged>,
    mut road_changes: MessageReader<RoadNetworkChanged>,
    mut handle: ResMut<SimHandle>,
    mut views: ResMut<LotViews>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
    mut built: Local<bool>,
) {
    let reset = resets.read().count() > 0 || !*built;
    let changed = lot_changes.read().count() + road_changes.read().count() > 0;
    *built = true;
    if !reset && !changed {
        return;
    }
    if reset {
        for (_, (_, entity)) in views.lots.drain() {
            commands.entity(entity).despawn();
        }
        views.revision += 1;
    }
    let sim = handle.get_mut();
    let roads = &sim.roads;
    let ids: Vec<LotId> = sim.lots.lots.keys().copied().collect();
    let mut wanted: HashMap<LotId, LotShape> = HashMap::new();
    for &id in &ids {
        let lot = &sim.lots.lots[&id];
        let (lot_type, plot) = (lot.lot_type, lot.plot);
        let door = sim.lots.access(roads, id).and_then(|point| {
            roads
                .lanes
                .get(&point.lane_id)
                .map(|lane| lane.sample_pose(point.distance).0)
        });
        wanted.insert(
            id,
            LotShape {
                lot_type,
                plot,
                door,
            },
        );
    }
    let gone: Vec<LotId> = views
        .lots
        .keys()
        .filter(|id| !wanted.contains_key(id))
        .copied()
        .collect();
    for id in gone {
        if let Some((_, entity)) = views.lots.remove(&id) {
            commands.entity(entity).despawn();
        }
        views.revision += 1;
    }
    for id in ids {
        let shape = &wanted[&id];
        if views.lots.get(&id).is_some_and(|(old, _)| old == shape) {
            continue;
        }
        if let Some((_, old)) = views.lots.remove(&id) {
            commands.entity(old).despawn();
        }
        let entity = spawn_shape(
            &mut commands,
            &mut meshes,
            &material,
            lot_shapes(shape),
            z::LOTS,
        );
        commands.entity(entity).insert(LotView(id));
        views.built += 1;
        views.revision += 1;
        views.lots.insert(id, (shape.clone(), entity));
    }
}
