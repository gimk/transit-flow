//! The street game's level, drawn from its data (pivot plan Phase 1): building blocks,
//! every strip by its use, separators, the junction area and its corners, lane
//! directions, the movements through the junction (thin lines, a debug look until the
//! art direction is chosen), obstacles and green areas. Rebuilt whole when
//! `LoadedLevel` changes; a level is small.

use bevy::prelude::*;
use tf_sim::street::Street;
use tf_sim::street::level::{AreaKind, ObstacleKind, Shape};
use tf_sim::street::space::{Separator, StripUse};

use super::{ViewMaterial, spawn_shape, z};
use crate::camera::sim_to_bevy;
use crate::draw::ShapeBuilder;
use crate::level::{LoadedLevel, PX_PER_M, level_to_world};

pub const OUTSIDE_COLOR: Color = Color::srgb(0.30, 0.33, 0.30);
pub const BLOCK_COLOR: Color = Color::srgb(0.52, 0.47, 0.42);
pub const ASPHALT_COLOR: Color = Color::srgb(0.27, 0.28, 0.30);
pub const SIDEWALK_COLOR: Color = Color::srgb(0.70, 0.69, 0.66);
pub const PAINT_COLOR: Color = Color::srgb(0.93, 0.93, 0.90);
pub const KERB_COLOR: Color = Color::srgb(0.84, 0.84, 0.82);
pub const BOLLARD_COLOR: Color = Color::srgb(0.15, 0.15, 0.17);
pub const GREEN_COLOR: Color = Color::srgb(0.27, 0.48, 0.25);
pub const ARROW_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.45);
pub const MOVEMENT_COLOR: Color = Color::srgba(1.0, 0.82, 0.25, 0.6);

/// Metres.
pub const PAINT_WIDTH: f32 = 0.15;
pub const KERB_WIDTH: f32 = 0.3;
pub const MOVEMENT_WIDTH: f32 = 0.15;
pub const ARROW_SPACING: f32 = 15.0;
pub const ARROW_SIZE: f32 = 1.0;

pub fn strip_color(use_: StripUse) -> Color {
    match use_ {
        StripUse::Sidewalk => SIDEWALK_COLOR,
        StripUse::CarLane => ASPHALT_COLOR,
        StripUse::BusLane => Color::srgb(0.55, 0.24, 0.22),
        StripUse::BikeLane => Color::srgb(0.24, 0.47, 0.35),
        StripUse::BikeTrack => Color::srgb(0.20, 0.52, 0.38),
        StripUse::Parking => Color::srgb(0.33, 0.34, 0.36),
        StripUse::DeliveryBay => Color::srgb(0.42, 0.37, 0.25),
        StripUse::Median => Color::srgb(0.55, 0.55, 0.53),
        StripUse::Planting => GREEN_COLOR,
        StripUse::Terrace => Color::srgb(0.62, 0.48, 0.35),
    }
}

/// Street metres (y down) to the Bevy world.
fn to_bevy(p: Vec2) -> Vec2 {
    sim_to_bevy(level_to_world(p))
}

fn points(points: &[Vec2]) -> Vec<Vec2> {
    points.iter().map(|p| to_bevy(*p)).collect()
}

/// Every shape of the level view.
#[derive(Component)]
pub struct StreetShape;

/// The layers of a level, each one mesh: ground and blocks, surfaces, markings, things.
pub fn build_layers(street: &Street) -> [(ShapeBuilder, f32); 4] {
    let mut ground = ShapeBuilder::new();
    let (a, b) = (to_bevy(street.bounds.min), to_bevy(street.bounds.max));
    ground.rect(a.min(b), a.max(b), OUTSIDE_COLOR);
    for block in &street.blocks {
        ground.polygon(&points(&block.points), BLOCK_COLOR);
    }

    let mut surfaces = ShapeBuilder::new();
    for junction in street.junctions.values() {
        surfaces.polygon(&points(&junction.polygon.points), ASPHALT_COLOR);
    }
    for segment in street.segments.values() {
        for (index, strip) in segment.profile.strips.iter().enumerate() {
            surfaces.polygon(
                &points(&segment.strip_polygon(index)),
                strip_color(strip.use_),
            );
        }
    }
    for plaza in &street.plazas {
        surfaces.polygon(&points(&plaza.points), SIDEWALK_COLOR);
    }
    for area in street.areas.values() {
        let color = match area.kind {
            AreaKind::Green => GREEN_COLOR,
            AreaKind::Rest => Color::srgb(0.60, 0.55, 0.40),
        };
        surfaces.polygon(&points(&area.polygon.points), color);
    }

    let mut markings = ShapeBuilder::new();
    for segment in street.segments.values() {
        let strips = &segment.profile.strips;
        for (index, strip) in strips.iter().enumerate() {
            let polygon = segment.strip_polygon(index);
            let half = polygon.len() / 2;
            // The strip polygon is its left edge, then its right edge backwards.
            let mut right: Vec<Vec2> = polygon[half..].to_vec();
            right.reverse();
            for (separator, edge) in [(strip.left, &polygon[..half]), (strip.right, &right[..])] {
                draw_separator(&mut markings, separator, edge);
            }
        }
        for lane in street.segment_lanes(segment.id) {
            draw_arrows(&mut markings, &lane.polyline());
        }
    }
    for junction in street.junctions.values() {
        for movement in street.movements(junction.node) {
            markings.polyline(
                &points(&movement.polyline()),
                MOVEMENT_WIDTH * PX_PER_M,
                MOVEMENT_COLOR,
            );
        }
    }

    let mut things = ShapeBuilder::new();
    for obstacle in street.obstacles.values() {
        let color = match obstacle.kind {
            ObstacleKind::Tree | ObstacleKind::Planter => Color::srgb(0.18, 0.40, 0.18),
            ObstacleKind::Bench => Color::srgb(0.45, 0.30, 0.18),
            _ => BOLLARD_COLOR,
        };
        match &obstacle.shape {
            Shape::Circle(circle) => {
                things.circle(to_bevy(circle.center), circle.radius * PX_PER_M, 16, color);
            }
            Shape::Polygon(polygon) => {
                things.polygon(&points(&polygon.points), color);
            }
        }
    }
    [
        (ground, z::STREET_GROUND),
        (surfaces, z::STREET_SURFACE),
        (markings, z::STREET_MARKINGS),
        (things, z::STREET_THINGS),
    ]
}

fn draw_separator(shapes: &mut ShapeBuilder, separator: Separator, edge: &[Vec2]) {
    let edge = points(edge);
    match separator {
        Separator::None => {}
        Separator::Paint => {
            shapes.polyline(&edge, PAINT_WIDTH * PX_PER_M, PAINT_COLOR);
        }
        Separator::Kerb => {
            shapes.polyline(&edge, KERB_WIDTH * PX_PER_M, KERB_COLOR);
        }
        Separator::Bollards | Separator::Planter => {
            let (color, radius) = if separator == Separator::Bollards {
                (BOLLARD_COLOR, 0.15)
            } else {
                (GREEN_COLOR, 0.4)
            };
            for pair in edge.windows(2) {
                let length = pair[0].distance(pair[1]) / PX_PER_M;
                let count = (length / 2.0).floor() as i32;
                for i in 0..=count {
                    let at = pair[0].lerp(pair[1], i as f32 / count.max(1) as f32);
                    shapes.circle(at, radius * PX_PER_M, 8, color);
                }
            }
        }
    }
}

/// A small chevron every `ARROW_SPACING` metres along a lane, pointing its way.
fn draw_arrows(shapes: &mut ShapeBuilder, lane: &[Vec2]) {
    let mut walked = 0.0;
    let mut next = ARROW_SPACING * 0.5;
    for pair in lane.windows(2) {
        let length = pair[0].distance(pair[1]);
        let direction = (pair[1] - pair[0]).normalize_or_zero();
        let side = Vec2::new(-direction.y, direction.x);
        while next <= walked + length {
            let tip = pair[0] + direction * (next - walked);
            let back = tip - direction * ARROW_SIZE;
            let width = 0.12 * PX_PER_M;
            shapes.polyline(
                &[to_bevy(back + side * ARROW_SIZE * 0.6), to_bevy(tip)],
                width,
                ARROW_COLOR,
            );
            shapes.polyline(
                &[to_bevy(back - side * ARROW_SIZE * 0.6), to_bevy(tip)],
                width,
                ARROW_COLOR,
            );
            next += ARROW_SPACING;
        }
        walked += length;
    }
}

fn rebuild_street_view(
    level: Res<LoadedLevel>,
    shapes: Query<Entity, With<StreetShape>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
) {
    if !level.is_changed() {
        return;
    }
    for entity in &shapes {
        commands.entity(entity).despawn();
    }
    let Some(street) = &level.street else { return };
    for (shape, depth) in build_layers(street) {
        if shape.is_empty() {
            continue;
        }
        let entity = spawn_shape(&mut commands, &mut meshes, &material, shape, depth);
        commands.entity(entity).insert(StreetShape);
    }
}

pub struct StreetViewPlugin;

impl Plugin for StreetViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, rebuild_street_view);
    }
}
