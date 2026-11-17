//! The speed limit of every segment, painted on the carriageway: a white disc with a red
//! ring and the limit in black digits, as French streets show it. One sign for each way of
//! travel, on the motor lane nearest the kerb, a few metres past the start of the lane
//! (that is, where the traffic enters the segment, at a junction or at the level edge).
//!
//! The digits are drawn with seven segments, so the view needs no font. The signs are
//! rebuilt with the street (`LoadedLevel` changes), so the speed tool shows at once.

use bevy::color::Alpha;
use bevy::prelude::*;
use tf_sim::street::Street;
use tf_sim::street::space::Access;

use super::{ViewMaterial, palette, spawn_shape, z};
use crate::draw::ShapeBuilder;
use crate::level::{LoadedLevel, PX_PER_M};
use crate::views::street::to_bevy;

/// Outer radius of a sign, metres: a bit under the width of a lane, so it reads at the
/// default framing (about 4 px per metre) without covering the lane.
pub const SIGN_RADIUS_M: f32 = 2.0;
/// Width of the red ring, as a share of the radius.
pub const RING_SHARE: f32 = 0.2;
/// A sign stands this far from the start of its lane, metres (less on a short lane).
pub const SIGN_INSET_M: f32 = 4.0;
const SIDES: u32 = 20;

/// A sign to paint: where (street metres, y down) and what it says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SignSpot {
    pub segment: i32,
    pub position: Vec2,
    pub kmh: f32,
}

/// The point `along` metres down `line`, clamped to its ends.
fn lane_point(line: &[Vec2], along: f32) -> Option<Vec2> {
    let mut walked = 0.0;
    for pair in line.windows(2) {
        let length = pair[0].distance(pair[1]);
        if length > 1e-6 && along <= walked + length {
            return Some(pair[0].lerp(pair[1], (along - walked) / length));
        }
        walked += length;
    }
    line.last().copied()
}

/// Every sign of the street: for each segment and each way, the motor lane with the
/// greatest `lateral` (the right-most one: the one nearest the kerb of its own way).
pub fn sign_spots(street: &Street) -> Vec<SignSpot> {
    let mut spots = Vec::new();
    for segment in street.segments.values() {
        for forward in [true, false] {
            let lane = street
                .segment_lanes(segment.id)
                .filter(|lane| lane.access.intersects(Access::MOTOR))
                .filter(|lane| (lane.from_node == segment.from) == forward)
                .max_by(|a, b| a.lateral.total_cmp(&b.lateral));
            let Some(lane) = lane else { continue };
            let line = lane.polyline();
            let inset = SIGN_INSET_M.min(lane.length as f32 * 0.5);
            if let Some(position) = lane_point(&line, inset) {
                spots.push(SignSpot {
                    segment: segment.id,
                    position,
                    kmh: segment.speed_limit_kmh,
                });
            }
        }
    }
    spots
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

/// One digit, upright, centred on `center` (Bevy world px); `height` is its height in px.
fn digit(shape: &mut ShapeBuilder, center: Vec2, height: f32, digit: usize, color: Color) {
    let (w, h, t) = (height * 0.5, height, height * 0.16);
    // (offset of the segment's centre, size), for each of the seven, y up.
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
            shape.quad(center + offset, size, 0.0, color);
        }
    }
}

/// A sign of `kmh` at `center` (Bevy world px). `alpha` fades the whole sign (a preview).
pub fn sign_shape(shape: &mut ShapeBuilder, center: Vec2, kmh: f32, alpha: f32) {
    let radius = SIGN_RADIUS_M * PX_PER_M;
    shape.circle(center, radius, SIDES, palette::SIGN_RING.with_alpha(alpha));
    shape.circle(
        center,
        radius * (1.0 - RING_SHARE),
        SIDES,
        palette::SIGN_FACE.with_alpha(alpha),
    );
    let value = kmh.round().max(0.0) as usize;
    let digits: Vec<usize> = value
        .to_string()
        .bytes()
        .map(|b| (b - b'0') as usize)
        .collect();
    let height = radius * 0.85;
    let pitch = height * 0.7;
    let first = -pitch * (digits.len() as f32 - 1.0) * 0.5;
    for (i, d) in digits.iter().enumerate() {
        let at = center + Vec2::new(first + pitch * i as f32, 0.0);
        digit(shape, at, height, *d, palette::SIGN_DIGIT.with_alpha(alpha));
    }
}

/// The shape of every sign of the street.
pub fn build_signs(street: &Street) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new();
    for spot in sign_spots(street) {
        sign_shape(&mut shape, to_bevy(spot.position), spot.kmh, 1.0);
    }
    shape
}

/// The one entity that holds every sign.
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
        let entity = spawn_shape(&mut commands, &mut meshes, &material, shape, z::SPEED_SIGNS);
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
        let mut one = ShapeBuilder::new();
        let mut two = ShapeBuilder::new();
        sign_shape(&mut one, Vec2::ZERO, 20.0, 1.0);
        sign_shape(&mut two, Vec2::ZERO, 50.0, 1.0);
        assert_ne!(one.positions(), two.positions());
        assert!(!one.is_empty());
    }
}
