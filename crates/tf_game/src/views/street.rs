//! The street game's level, drawn from its data (pivot plan Phase 1): building blocks,
//! every strip by its use, separators, the junction area and its corners, lane
//! directions, the movements through the junction (thin lines, a debug look until the
//! art direction is chosen), obstacles, green areas and the zebra crossings (Phase 3).
//! Rebuilt whole when `LoadedLevel` changes; a level is small.

use bevy::prelude::*;
use tf_sim::roads::lane_builder;
use tf_sim::street::Street;
use tf_sim::street::level::{AreaKind, ObstacleKind, Shape};
use tf_sim::street::space::{Profile, Separator, Strip, StripUse};

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
/// A zebra bar's width across the road (and the gap to the next bar), metres. A bar is as
/// long as the crossing is wide, along the traffic direction.
pub const ZEBRA_BAR_WIDTH_M: f32 = 0.5;
pub const ZEBRA_GAP_M: f32 = 0.5;

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
    for bar in zebra_bars(street) {
        markings.polygon(&points(&bar), PAINT_COLOR);
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

/// One polygon per zebra bar of every crossing, in street metres (y down), for the
/// markings layer. A French zebra is bars long along the traffic direction (as long as the
/// crossing is wide) and 0.5 m wide across the road, with 0.5 m gaps, from one kerb to the
/// other, so a walker steps over each bar in turn. The pattern is centred on the
/// carriageway, which on these profiles leaves the first bar 0.25 m inside the kerb
/// (8 m of carriageway: 8 bars; 7 m: 7 bars). The bars span the road only, because a zebra
/// paints the carriageway, not the sidewalks.
pub fn zebra_bars(street: &Street) -> Vec<Vec<Vec2>> {
    let mut bars = Vec::new();
    let period = ZEBRA_BAR_WIDTH_M + ZEBRA_GAP_M;
    for crossing in street.crossings.values() {
        let Some(segment) = street.segments.get(&crossing.segment) else {
            continue;
        };
        let Some((left, right)) = carriageway(&segment.profile) else {
            continue;
        };
        let across = right - left;
        let count = ((across + ZEBRA_GAP_M) / period).floor() as usize;
        if count == 0 {
            continue;
        }
        let used = count as f32 * ZEBRA_BAR_WIDTH_M + (count - 1) as f32 * ZEBRA_GAP_M;
        let margin = (across - used) * 0.5;
        let start = crossing.at - crossing.width * 0.5;
        let end = crossing.at + crossing.width * 0.5;
        for k in 0..count {
            let from = left + margin + k as f32 * period;
            let to = from + ZEBRA_BAR_WIDTH_M;
            let from_edge = lane_builder::offset_polyline(&segment.centerline, f64::from(from));
            let to_edge = lane_builder::offset_polyline(&segment.centerline, f64::from(to));
            let mut polygon: Vec<Vec2> =
                lane_builder::sub_polyline(&from_edge, f64::from(start), f64::from(end));
            let mut other: Vec<Vec2> =
                lane_builder::sub_polyline(&to_edge, f64::from(start), f64::from(end));
            other.reverse();
            polygon.extend(other);
            if polygon.len() >= 3 {
                bars.push(polygon);
            }
        }
    }
    bars
}

/// The span of a profile's carriageway, as offsets from its centerline: from the kerb of
/// the leftmost strip that is not a foot strip to the kerb of the rightmost one. A level's
/// profiles put the sidewalks at both ends (a median or a bike lane in the middle is road).
fn carriageway(profile: &Profile) -> Option<(f32, f32)> {
    let foot = |strip: &Strip| matches!(strip.use_, StripUse::Sidewalk | StripUse::Terrace);
    let first = profile.strips.iter().position(|strip| !foot(strip))?;
    let last = profile.strips.iter().rposition(|strip| !foot(strip))?;
    let (left, _) = profile.strip_span(first);
    let (_, right) = profile.strip_span(last);
    Some((left, right))
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
