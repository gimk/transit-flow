//! The speed limit of every segment, painted on the carriageway as in French streets: the
//! number only, in worn white (`palette::SPEED_MARK`), on the motor lane nearest the kerb,
//! a few metres past the start of the lane (that is, where the traffic enters the segment,
//! at a junction or at the level edge). One marking for each way of travel.
//!
//! The marking is stretched along the traffic, as real road markings are, so that a driver
//! reads it at a low angle: the bottom of the digits faces the approaching driver and the
//! top points the way the traffic goes.
//!
//! The digits are drawn with seven segments, so the view needs no font. The markings are
//! rebuilt with the street (`LoadedLevel` changes), so the speed tool shows at once.

use bevy::color::Alpha;
use bevy::prelude::*;
use tf_sim::street::Street;
use tf_sim::street::space::Access;

use super::{ViewMaterial, palette, spawn_shape, z};
use crate::draw::ShapeBuilder;
use crate::level::{LoadedLevel, PX_PER_M};
use crate::views::street::to_bevy;

/// Height of a digit along the lane, metres (the traffic direction). Real markings are
/// about this tall so that a driver reads them at a low angle.
pub const DIGIT_HEIGHT_M: f32 = 2.5;
/// Width of a digit across the lane, metres.
pub const DIGIT_WIDTH_M: f32 = 0.8;
/// Thickness of a digit's segments, metres (real markings: 0.15 to 0.2 m).
pub const STROKE_M: f32 = 0.18;
/// Space between two digits across the lane, metres.
pub const DIGIT_GAP_M: f32 = 0.3;
/// The centre of a marking stands this far from the start of its lane, metres (less on a
/// short lane).
pub const SIGN_INSET_M: f32 = 4.0;

/// A marking to paint: where (street metres, y down), which way the traffic goes there
/// (a unit vector in street metres) and the limit it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SignSpot {
    pub segment: i32,
    pub position: Vec2,
    pub direction: Vec2,
    pub kmh: f32,
}

/// The point `along` metres down `line`, clamped to its ends, and the unit direction of
/// travel there.
fn lane_point(line: &[Vec2], along: f32) -> Option<(Vec2, Vec2)> {
    let mut walked = 0.0;
    let mut last = None;
    for pair in line.windows(2) {
        let length = pair[0].distance(pair[1]);
        if length <= 1e-6 {
            continue;
        }
        let direction = (pair[1] - pair[0]) / length;
        if along <= walked + length {
            return Some((pair[0] + direction * (along - walked), direction));
        }
        walked += length;
        last = Some((pair[1], direction));
    }
    last
}

/// The markings of one segment: for each way, the motor lane with the greatest `lateral`
/// (the right-most one: the one nearest the kerb of its own way).
pub fn segment_signs(street: &Street, segment: i32) -> Vec<SignSpot> {
    let mut spots = Vec::new();
    let Some(segment) = street.segments.get(&segment) else {
        return spots;
    };
    for forward in [true, false] {
        let lane = street
            .segment_lanes(segment.id)
            .filter(|lane| lane.access.intersects(Access::MOTOR))
            .filter(|lane| (lane.from_node == segment.from) == forward)
            .max_by(|a, b| a.lateral.total_cmp(&b.lateral));
        let Some(lane) = lane else { continue };
        let line = lane.polyline();
        let inset = SIGN_INSET_M.min(lane.length as f32 * 0.5);
        if let Some((position, direction)) = lane_point(&line, inset) {
            spots.push(SignSpot {
                segment: segment.id,
                position,
                direction,
                kmh: segment.speed_limit_kmh,
            });
        }
    }
    spots
}

/// Every marking of the street.
pub fn sign_spots(street: &Street) -> Vec<SignSpot> {
    street
        .segments
        .keys()
        .flat_map(|id| segment_signs(street, *id))
        .collect()
}

/// Which of the seven segments (top, upper right, lower right, bottom, lower left, upper
/// left, middle) draw a digit.
const SEGMENTS: [[bool; 7]; 10] = [
    [true, true, true, true, true, true, false],
    [false, true, true, false, false, false, false],
    [true, true, false, true, true, false, true],
    [true, true, true, true, false, false, true],
    [false, true, true, false, false, true, true],
    [true, false, true, true, false, true, true],
    [true, false, true, true, true, true, true],
    [true, true, true, false, false, false, false],
    [true, true, true, true, true, true, true],
    [true, true, true, true, false, true, true],
];

/// One digit, centred on `center` (Bevy world px). `up` is the unit vector the reader's
/// top points to (the traffic direction); the digit's right is `up` turned clockwise.
fn digit(shape: &mut ShapeBuilder, center: Vec2, up: Vec2, digit: usize, color: Color) {
    let right = Vec2::new(up.y, -up.x);
    // The angle that turns the quads' own x axis onto `right` and their y axis onto `up`.
    let angle = (-up.x).atan2(up.y);
    let (w, h, t) = (
        DIGIT_WIDTH_M * PX_PER_M,
        DIGIT_HEIGHT_M * PX_PER_M,
        STROKE_M * PX_PER_M,
    );
    // (offset of the segment's centre, size), for each of the seven: `offset.x` across the
    // lane, `offset.y` along it.
    let parts: [(Vec2, Vec2); 7] = [
        (Vec2::new(0.0, h * 0.5 - t * 0.5), Vec2::new(w, t)),
        (
            Vec2::new(w * 0.5 - t * 0.5, h * 0.25),
            Vec2::new(t, h * 0.5),
        ),
        (
            Vec2::new(w * 0.5 - t * 0.5, -h * 0.25),
            Vec2::new(t, h * 0.5),
        ),
        (Vec2::new(0.0, -h * 0.5 + t * 0.5), Vec2::new(w, t)),
        (
            Vec2::new(-w * 0.5 + t * 0.5, -h * 0.25),
            Vec2::new(t, h * 0.5),
        ),
        (
            Vec2::new(-w * 0.5 + t * 0.5, h * 0.25),
            Vec2::new(t, h * 0.5),
        ),
        (Vec2::ZERO, Vec2::new(w, t)),
    ];
    for (on, (offset, size)) in SEGMENTS[digit % 10].iter().zip(parts) {
        if *on {
            let at = center + right * offset.x + up * offset.y;
            shape.quad(at, size, angle, color);
        }
    }
}

/// The limit `kmh` painted at `spot`, as on the carriageway. `alpha` fades it (a preview).
pub fn sign_shape(shape: &mut ShapeBuilder, spot: &SignSpot, kmh: f32, alpha: f32) {
    let center = to_bevy(spot.position);
    let up = (to_bevy(spot.position + spot.direction) - center).normalize_or_zero();
    let right = Vec2::new(up.y, -up.x);
    let value = kmh.round().max(0.0) as usize;
    let digits: Vec<usize> = value
        .to_string()
        .bytes()
        .map(|b| (b - b'0') as usize)
        .collect();
    let pitch = (DIGIT_WIDTH_M + DIGIT_GAP_M) * PX_PER_M;
    let first = -pitch * (digits.len() as f32 - 1.0) * 0.5;
    for (i, d) in digits.iter().enumerate() {
        let at = center + right * (first + pitch * i as f32);
        digit(shape, at, up, *d, palette::SPEED_MARK.with_alpha(alpha));
    }
}

/// The shape of every marking of the street.
pub fn build_signs(street: &Street) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new();
    for spot in sign_spots(street) {
        sign_shape(&mut shape, &spot, spot.kmh, 1.0);
    }
    shape
}

/// The one entity that holds every marking.
#[derive(Component)]
pub struct SpeedSigns;

fn rebuild_signs(
    level: Res<LoadedLevel>,
    existing: Query<Entity, With<SpeedSigns>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
) {
    if !level.is_changed() {
        return;
    }
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let Some(street) = &level.street else { return };
    let shape = build_signs(street);
    if !shape.is_empty() {
        let entity = spawn_shape(
            &mut commands,
            &mut meshes,
            &material,
            shape,
            z::STREET_MARKINGS,
        );
        commands.entity(entity).insert(SpeedSigns);
    }
}

pub struct SpeedSignPlugin;

impl Plugin for SpeedSignPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, rebuild_signs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tf_sim::street::{Design, LevelFile};

    fn street() -> Street {
        let level = LevelFile::load("carrefour-ecole").expect("loads");
        Street::from_level_and_design(&level, &Design::new(&level.id)).expect("builds")
    }

    #[test]
    fn every_segment_has_a_sign_for_each_way() {
        let street = street();
        let spots = sign_spots(&street);
        for segment in street.segments.values() {
            let count = spots.iter().filter(|s| s.segment == segment.id).count();
            assert!(
                (1..=2).contains(&count),
                "segment {} has {count} signs",
                segment.id
            );
            assert!(
                spots
                    .iter()
                    .filter(|s| s.segment == segment.id)
                    .all(|s| s.kmh == segment.speed_limit_kmh)
            );
        }
    }

    #[test]
    fn a_new_limit_changes_the_sign_shape() {
        let spot = SignSpot {
            segment: 0,
            position: Vec2::ZERO,
            direction: Vec2::X,
            kmh: 20.0,
        };
        let mut one = ShapeBuilder::new();
        let mut two = ShapeBuilder::new();
        sign_shape(&mut one, &spot, 20.0, 1.0);
        sign_shape(&mut two, &spot, 50.0, 1.0);
        assert_ne!(one.positions(), two.positions());
        assert!(!one.is_empty());
    }

    #[test]
    fn the_marking_is_stretched_along_the_traffic() {
        // "30" going right on the street: its height runs along x, its two digits across y.
        let spot = SignSpot {
            segment: 0,
            position: Vec2::ZERO,
            direction: Vec2::X,
            kmh: 30.0,
        };
        let mut shape = ShapeBuilder::new();
        sign_shape(&mut shape, &spot, 30.0, 1.0);
        let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in shape.positions() {
            min = min.min(Vec2::new(p[0], p[1]));
            max = max.max(Vec2::new(p[0], p[1]));
        }
        let size = max - min;
        assert!((size.x - DIGIT_HEIGHT_M * PX_PER_M).abs() < 1e-3, "{size}");
        assert!(
            (size.y - (2.0 * DIGIT_WIDTH_M + DIGIT_GAP_M) * PX_PER_M).abs() < 1e-3,
            "{size}"
        );
    }
}
