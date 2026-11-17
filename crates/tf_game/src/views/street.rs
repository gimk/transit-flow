//! The street game's level, drawn from its data (pivot plan Phase 1): building blocks,
//! every strip by its use, separators, the junction area and its corners, lane
//! directions, the movements through the junction (thin lines, a debug look until the
//! art direction is chosen), obstacles, green areas and the zebra crossings (Phase 3).
//! Rebuilt whole when `LoadedLevel` changes; a level is small.

use bevy::prelude::*;
use tf_sim::roads::lane_builder;
use tf_sim::street::Street;
use tf_sim::street::level::{AreaKind, ObstacleKind, Shape};
use tf_sim::street::model::Crossing;
use tf_sim::street::space::{Profile, Separator, Strip, StripUse};

use super::{ViewMaterial, palette, spawn_shape, z};
use crate::camera::sim_to_bevy;
use crate::draw::ShapeBuilder;
use crate::level::{LoadedLevel, PX_PER_M, level_to_world};
use crate::views::speed_signs::{distance_to_outline, lane_point, marking_span};

pub const OUTSIDE_COLOR: Color = palette::OUTSIDE;
pub const BLOCK_COLOR: Color = palette::BLOCK;
pub const ASPHALT_COLOR: Color = palette::CARRIAGEWAY;
pub const SIDEWALK_COLOR: Color = palette::SIDEWALK;
pub const PAINT_COLOR: Color = palette::PAINT;
pub const KERB_COLOR: Color = palette::KERB;
pub const BOLLARD_COLOR: Color = palette::BOLLARD;
pub const GREEN_COLOR: Color = palette::PLANTING;
pub const ARROW_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.7);
/// The thin lines of the movements through a junction: faint white on the carriageway.
pub const MOVEMENT_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.35);

/// Metres.
pub const BLOCK_OUTLINE_WIDTH: f32 = 0.4;
pub const PAINT_WIDTH: f32 = 0.15;
pub const KERB_WIDTH: f32 = 0.3;
pub const MOVEMENT_WIDTH: f32 = 0.15;
pub const ARROW_SPACING: f32 = 15.0;
pub const ARROW_SIZE: f32 = 1.0;
/// A zebra bar's width across the road (and the gap to the next bar), metres. A bar is as
/// long as the crossing is wide, along the traffic direction.
pub const ZEBRA_BAR_WIDTH_M: f32 = 0.5;
pub const ZEBRA_GAP_M: f32 = 0.5;
/// A bollard dot is drawn at least this big, metres (its real radius is 0.1 m).
pub const MIN_BOLLARD_DRAW_M: f32 = 0.25;

pub fn strip_color(use_: StripUse) -> Color {
    match use_ {
        StripUse::Sidewalk => SIDEWALK_COLOR,
        StripUse::CarLane => ASPHALT_COLOR,
        StripUse::BusLane => palette::BUS_LANE,
        StripUse::BikeLane => palette::BIKE_LANE,
        StripUse::BikeTrack => palette::BIKE_TRACK,
        StripUse::Parking => palette::PARKING,
        StripUse::DeliveryBay => palette::DELIVERY_BAY,
        StripUse::Median => palette::MEDIAN,
        StripUse::Planting => GREEN_COLOR,
        StripUse::Terrace => palette::TERRACE,
    }
}

/// Street metres (y down) to the Bevy world.
pub fn to_bevy(p: Vec2) -> Vec2 {
    sim_to_bevy(level_to_world(p))
}

pub fn points(points: &[Vec2]) -> Vec<Vec2> {
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
        let outline = points(&block.points);
        ground.polygon(&outline, BLOCK_COLOR);
        ground.closed_polyline(
            &outline,
            BLOCK_OUTLINE_WIDTH * PX_PER_M,
            palette::BLOCK_OUTLINE,
        );
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
            AreaKind::Rest => palette::REST_AREA,
        };
        surfaces.polygon(&points(&area.polygon.points), color);
    }

    let mut markings = ShapeBuilder::new();
    let bars = zebra_bars(street);
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
            draw_arrows(
                &mut markings,
                &lane.polyline(),
                lane.length as f32,
                marking_span(street, lane),
                &bars,
            );
        }
    }
    for bar in &bars {
        markings.polygon(&points(bar), PAINT_COLOR);
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
            ObstacleKind::Tree | ObstacleKind::Planter => palette::TREE,
            ObstacleKind::Bench => palette::BENCH,
            _ => BOLLARD_COLOR,
        };
        match &obstacle.shape {
            Shape::Circle(circle) => {
                // A bollard is 0.1 m of radius: drawn at least this big to be seen at all.
                let radius = match obstacle.kind {
                    ObstacleKind::Bollard => circle.radius.max(MIN_BOLLARD_DRAW_M),
                    _ => circle.radius,
                };
                things.circle(to_bevy(circle.center), radius * PX_PER_M, 16, color);
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
    street
        .crossings
        .values()
        .flat_map(|crossing| crossing_bars(street, crossing))
        .collect()
}

/// The bars of one crossing (see `zebra_bars`). Also draws the preview of a crossing that
/// is not in the street yet.
pub fn crossing_bars(street: &Street, crossing: &Crossing) -> Vec<Vec<Vec2>> {
    let mut bars = Vec::new();
    let period = ZEBRA_BAR_WIDTH_M + ZEBRA_GAP_M;
    let Some(segment) = street.segments.get(&crossing.segment) else {
        return bars;
    };
    let Some((left, right)) = carriageway(&segment.profile) else {
        return bars;
    };
    let across = right - left;
    let count = ((across + ZEBRA_GAP_M) / period).floor() as usize;
    if count == 0 {
        return bars;
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
    bars
}

/// The span of a profile's carriageway, as offsets from its centerline: from the kerb of
/// the leftmost strip that is not a foot strip to the kerb of the rightmost one. A level's
/// profiles put the sidewalks at both ends (a median or a bike lane in the middle is road).
pub fn carriageway(profile: &Profile) -> Option<(f32, f32)> {
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

/// Metres kept between an arrow and the speed marking on its lane: an arrow whose shape
/// comes closer is not drawn.
pub const ARROW_CLEARANCE_M: f32 = 1.0;

/// The distances along a lane of length `length` metres where its arrow tips are, in order.
pub fn arrow_tips(length: f32) -> impl Iterator<Item = f32> {
    (0..)
        .map(|k| ARROW_SPACING * 0.5 + k as f32 * ARROW_SPACING)
        .take_while(move |tip| *tip <= length)
}

/// Whether the arrow with its tip `tip` metres down a lane stays clear of the lane's speed
/// marking `span` (see `speed_signs::marking_span`). The arrow covers `ARROW_SIZE` metres
/// back from its tip. The arrow and the marking are both on the lane line, so their
/// distance is the gap along the lane.
pub fn arrow_clear_of(tip: f32, span: Option<(f32, f32)>) -> bool {
    match span {
        None => true,
        Some((from, to)) => {
            tip + ARROW_CLEARANCE_M <= from || tip - ARROW_SIZE >= to + ARROW_CLEARANCE_M
        }
    }
}

/// Whether the arrow with its tip `tip` metres down `line` keeps `ARROW_CLEARANCE_M` clear of
/// every zebra bar (`zebra_bars`). The arrow's footprint is `ARROW_SIZE` long and 0.6 m either
/// side of the lane line (as drawn in `draw_arrows`); it is sampled along and across, as the
/// speed marking's is (`speed_signs::clear_of_bars`).
pub fn arrow_clear_of_bars(line: &[Vec2], tip: f32, bars: &[Vec<Vec2>]) -> bool {
    for k in 0..=20 {
        let t = tip - ARROW_SIZE + k as f32 * ARROW_SIZE / 20.0;
        let Some((p, direction)) = lane_point(line, t) else {
            return false;
        };
        let normal = Vec2::new(-direction.y, direction.x);
        for side in [-0.6, 0.0, 0.6] {
            let q = p + normal * side;
            if bars
                .iter()
                .any(|bar| distance_to_outline(q, bar) < ARROW_CLEARANCE_M)
            {
                return false;
            }
        }
    }
    true
}

/// A small chevron every `ARROW_SPACING` metres along a lane, pointing its way. Arrows that
/// would touch the lane's speed marking or a zebra bar are left out.
fn draw_arrows(
    shapes: &mut ShapeBuilder,
    lane: &[Vec2],
    length: f32,
    span: Option<(f32, f32)>,
    bars: &[Vec<Vec2>],
) {
    for tip in arrow_tips(length)
        .filter(|tip| arrow_clear_of(*tip, span) && arrow_clear_of_bars(lane, *tip, bars))
    {
        let Some((at, direction)) = lane_point(lane, tip) else {
            continue;
        };
        let side = Vec2::new(-direction.y, direction.x);
        let back = at - direction * ARROW_SIZE;
        let width = 0.12 * PX_PER_M;
        shapes.polyline(
            &[to_bevy(back + side * ARROW_SIZE * 0.6), to_bevy(at)],
            width,
            ARROW_COLOR,
        );
        shapes.polyline(
            &[to_bevy(back - side * ARROW_SIZE * 0.6), to_bevy(at)],
            width,
            ARROW_COLOR,
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::speed_signs::{DIGIT_GAP_M, DIGIT_HEIGHT_M, DIGIT_WIDTH_M};
    use tf_sim::street::{Design, LevelFile};

    fn street() -> Street {
        let level = LevelFile::load("carrefour-ecole").expect("loads");
        Street::from_level_and_design(&level, &Design::new(&level.id)).expect("builds")
    }

    #[test]
    fn the_clearance_rule_at_its_edges() {
        // The arrow with its tip at 7.5 m covers 6.5 m to 7.5 m.
        let tip = 7.5;
        // Clear: the marking ends 1 m before the arrow's back, or starts 1 m after its tip.
        assert!(arrow_clear_of(tip, None));
        assert!(arrow_clear_of(tip, Some((2.75, 5.25))));
        assert!(arrow_clear_of(tip, Some((0.0, 5.5))));
        assert!(arrow_clear_of(tip, Some((8.5, 9.0))));
        // Not clear: less than 1 m between them, or the marking under the arrow.
        assert!(!arrow_clear_of(tip, Some((5.6, 6.4))));
        assert!(!arrow_clear_of(tip, Some((6.0, 8.0))));
        assert!(!arrow_clear_of(tip, Some((8.0, 9.0))));
    }

    #[test]
    fn no_drawn_arrow_comes_within_1_m_of_a_speed_marking() {
        let street = street();
        let across = (2.0 * DIGIT_WIDTH_M + DIGIT_GAP_M) * 0.5;
        let half = DIGIT_HEIGHT_M * 0.5;
        let mut markings = 0;
        for segment in street.segments.keys() {
            for lane in street.segment_lanes(*segment) {
                let Some((from, to)) = marking_span(&street, lane) else {
                    continue;
                };
                markings += 1;
                let line = lane.polyline();
                // The marking's footprint, sampled on its edges and its centre line.
                let mut marking = Vec::new();
                for k in 0..=40 {
                    let t = from + k as f32 * (to - from) / 40.0;
                    let (p, d) = lane_point(&line, t).expect("on the lane");
                    let n = Vec2::new(-d.y, d.x);
                    for side in [-across, 0.0, across] {
                        marking.push(p + n * side);
                    }
                }
                assert!((to - from - 2.0 * half).abs() < 1e-3);
                for tip in arrow_tips(lane.length as f32) {
                    if !arrow_clear_of(tip, Some((from, to))) {
                        continue;
                    }
                    // A drawn arrow: its footprint (0.6 m either side of the line, from
                    // ARROW_SIZE back to the tip) must stay 1 m from the marking.
                    let mut closest = f32::MAX;
                    for k in 0..=20 {
                        let t = tip - ARROW_SIZE + k as f32 * ARROW_SIZE / 20.0;
                        let (p, d) = lane_point(&line, t).expect("on the lane");
                        let n = Vec2::new(-d.y, d.x);
                        for side in [-0.6, 0.0, 0.6] {
                            let q = p + n * side;
                            for m in &marking {
                                closest = closest.min(q.distance(*m));
                            }
                        }
                    }
                    // Sampling can miss the closest pair by about 0.1 m.
                    assert!(
                        closest >= ARROW_CLEARANCE_M - 0.1,
                        "arrow at {tip} m is {closest} m from a marking (segment {})",
                        lane.segment_id.unwrap_or(-1)
                    );
                }
            }
        }
        assert!(markings > 0, "the level has speed markings");
    }

    #[test]
    fn no_drawn_arrow_comes_within_1_m_of_a_zebra() {
        let street = street();
        let bars = zebra_bars(&street);
        assert!(!bars.is_empty(), "the level has zebras");
        let (mut drawn, mut dropped) = (0, 0);
        for segment in street.segments.keys() {
            for lane in street.segment_lanes(*segment) {
                let line = lane.polyline();
                for tip in arrow_tips(lane.length as f32) {
                    if !arrow_clear_of_bars(&line, tip, &bars) {
                        // Only the arrows that really come close to a bar are left out.
                        dropped += 1;
                        continue;
                    }
                    drawn += 1;
                    // A drawn arrow: its footprint, sampled finer than the rule does.
                    let mut closest = f32::MAX;
                    for k in 0..=40 {
                        let t = tip - ARROW_SIZE + k as f32 * ARROW_SIZE / 40.0;
                        let (p, d) = lane_point(&line, t).expect("on the lane");
                        let n = Vec2::new(-d.y, d.x);
                        for side in [-0.6, 0.0, 0.6] {
                            let q = p + n * side;
                            for bar in &bars {
                                closest = closest.min(distance_to_outline(q, bar));
                            }
                        }
                    }
                    assert!(
                        closest >= ARROW_CLEARANCE_M - 0.1,
                        "arrow at {tip} m is {closest} m from a zebra (segment {})",
                        lane.segment_id.unwrap_or(-1)
                    );
                }
            }
        }
        assert!(drawn > 0, "some arrows are drawn");
        assert!(dropped > 0, "the zebras take some arrows away");
    }
}
