//! Lanes on strips (pivot plan §3.1, §3.2): one lane per travel strip, offset from the
//! centerline by the widths of the strips before it, and the junction movements between
//! them. The geometry helpers of `roads::lane_builder` are reused as they are (offset
//! polylines, connector curves, conflict zones); the old road network is not involved.

use glam::Vec2;
use indexmap::IndexMap;

use super::ids::StreetLaneId;
use super::space::{Access, Direction, Profile, StripUse};
use crate::geom::{Curve2D, GdVec2};
use crate::roads::lane::Turn;
use crate::roads::lane_builder;

/// Curves are baked every this many metres (Godot's default of 5 is for pixels and would
/// cut the corners of a 5 m turn).
pub const BAKE_INTERVAL: f32 = 0.25;
/// A turn closer to straight than this is "straight" (the old `STRAIGHT_MAX_ANGLE`).
pub const STRAIGHT_MAX_ANGLE: f64 = std::f64::consts::PI / 6.0;
/// Conflict zones are sampled every this many metres.
pub const CONFLICT_STEP: f64 = 0.5;
/// Two movements that merge into one lane share at least its last this many metres.
pub const MERGE_ZONE_LENGTH: f64 = 5.0;

/// A path agents follow: along a travel strip, or through a junction (a movement).
#[derive(Clone, Debug, PartialEq)]
pub struct StreetLane {
    pub id: StreetLaneId,
    /// `None` for a movement.
    pub segment_id: Option<i32>,
    /// The strip it runs along; `None` for a movement.
    pub strip: Option<usize>,
    /// For movements both are the junction node.
    pub from_node: i32,
    pub to_node: i32,
    pub curve: Curve2D,
    pub length: f64,
    pub width: f32,
    pub access: Access,
    /// Offset of the lane's middle to the right of its own direction of travel: lower
    /// is further left, which orders the lanes of one approach from left to right.
    pub lateral: f32,
    /// Metres per second.
    pub speed_limit: f64,
    pub turn: Turn,
    /// Movements only: the lane that feeds it.
    pub feeder: Option<StreetLaneId>,
    /// Road lanes: the movements leaving its end; movements: the lane they lead into.
    pub outgoing: Vec<StreetLaneId>,
    /// Movements only: for each conflicting movement, the stretch of this one (x = from,
    /// y = to, distances along it) that comes too close to it.
    pub conflict_zones: IndexMap<StreetLaneId, Vec2>,
}

impl StreetLane {
    pub fn is_movement(&self) -> bool {
        self.segment_id.is_none()
    }

    pub fn polyline(&self) -> Vec<Vec2> {
        lane_builder::get_polyline(&self.curve)
    }
}

/// The road lane of one travel strip, before it gets an id.
#[derive(Clone, Debug, PartialEq)]
pub struct StripLaneSpec {
    pub strip: usize,
    pub forward: bool,
    pub points: Vec<Vec2>,
    pub width: f32,
    pub access: Access,
    pub lateral: f32,
}

/// One spec per travel strip, in strip order. `start_trim` / `end_trim` cut the lanes
/// back from the centerline's first / last point (the junction edges).
pub fn strip_lanes(
    centerline: &[Vec2],
    profile: &Profile,
    start_trim: f32,
    end_trim: f32,
) -> Vec<StripLaneSpec> {
    let mut specs = Vec::new();
    for (index, strip) in profile.strips.iter().enumerate() {
        if !strip.use_.is_travel() || strip.direction == Direction::None {
            continue;
        }
        let (left, right) = profile.strip_span(index);
        let offset = (left + right) * 0.5;
        let mut points = lane_builder::offset_polyline(centerline, f64::from(offset));
        let forward = strip.direction == Direction::Forward;
        let (from_trim, to_trim) = if forward {
            (start_trim, end_trim)
        } else {
            points.reverse();
            (end_trim, start_trim)
        };
        let length = lane_builder::polyline_length(&points);
        let points =
            lane_builder::sub_polyline(&points, f64::from(from_trim), length - f64::from(to_trim));
        specs.push(StripLaneSpec {
            strip: index,
            forward,
            points,
            width: strip.width,
            access: strip.access(),
            lateral: if forward { offset } else { -offset },
        });
    }
    specs
}

/// The polygon a strip covers between the two trims (drawing and walkable space).
pub fn strip_polygon(
    centerline: &[Vec2],
    profile: &Profile,
    index: usize,
    start_trim: f32,
    end_trim: f32,
) -> Vec<Vec2> {
    let (left, right) = profile.strip_span(index);
    let length = lane_builder::polyline_length(centerline);
    let cut = |offset: f32| {
        let side = lane_builder::offset_polyline(centerline, f64::from(offset));
        lane_builder::sub_polyline(&side, f64::from(start_trim), length - f64::from(end_trim))
    };
    let mut points = cut(left);
    let mut other = cut(right);
    other.reverse();
    points.extend(other);
    points
}

pub fn curve_from_points(points: &[Vec2]) -> Curve2D {
    let curve = Curve2D::from_points(points);
    Curve2D::with_bake_interval(curve.points().to_vec(), BAKE_INTERVAL)
}

pub fn connector_curve(from: &Curve2D, to: &Curve2D) -> Curve2D {
    let curve = lane_builder::build_connector_curve(from, to);
    Curve2D::with_bake_interval(curve.points().to_vec(), BAKE_INTERVAL)
}

/// The movement from a lane ending in direction `d0` into one starting in direction `d1`.
pub fn classify_turn(d0: Vec2, d1: Vec2) -> Turn {
    if f64::from(d0.gd_angle_to(d1).abs()) < STRAIGHT_MAX_ANGLE {
        Turn::Straight
    } else if d0.gd_cross(d1) > 0.0 {
        // y down: a positive cross product turns clockwise, i.e. right.
        Turn::Right
    } else {
        Turn::Left
    }
}

/// Lanes that take the same movements: bicycle-only lanes, bus lanes, and the general
/// lanes everyone else uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneGroup {
    Bicycle,
    Bus,
    General,
}

impl LaneGroup {
    pub fn of(use_: StripUse, access: Access) -> LaneGroup {
        if !access.intersects(Access::MOTOR) {
            LaneGroup::Bicycle
        } else if use_ == StripUse::BusLane || !access.intersects(Access::CAR) {
            LaneGroup::Bus
        } else {
            LaneGroup::General
        }
    }

    /// Where a lane of this group may lead, in order of preference, when the next arm has
    /// no lane of its own group: a bicycle lane continues into the general lanes (French
    /// cyclists ride in car lanes) or a bus lane, a bus lane into the general lanes, and
    /// a general lane into a bicycle lane (for its cyclists: the movement's access is
    /// what both lanes allow).
    pub fn targets(self) -> &'static [LaneGroup] {
        match self {
            LaneGroup::Bicycle => &[LaneGroup::Bicycle, LaneGroup::General, LaneGroup::Bus],
            LaneGroup::Bus => &[LaneGroup::Bus, LaneGroup::General],
            LaneGroup::General => &[LaneGroup::General, LaneGroup::Bicycle],
        }
    }
}

/// Which lane of the target group a lane at `index` of `count` (left to right in its
/// own group) leads into, for a turn, among `target_count` lanes (left to right); `None`
/// when that lane makes no such movement. Straight keeps its position (the extra lanes
/// of a wider approach all go to the last lane); right turns go from the rightmost lane
/// to the rightmost lane, left turns from the leftmost to the leftmost. A lane that falls
/// back to another group (`same_group` false) keeps to the right except to turn left.
pub fn target_index(
    turn: Turn,
    index: usize,
    count: usize,
    target_count: usize,
    same_group: bool,
) -> Option<usize> {
    let last = target_count - 1;
    match turn {
        Turn::Straight if same_group => Some(index.min(last)),
        Turn::Straight => (index == count - 1).then_some(last),
        Turn::Right => (index == count - 1).then_some(last),
        Turn::Left => (index == 0).then_some(0),
        Turn::None | Turn::UTurn => None,
    }
}

/// The conflict zones of movements `a` and `b` (on each), or `None` if they don't come
/// closer than the sum of their half widths. `merge`: they lead into the same lane.
pub fn conflict_zones(
    a: &[Vec2],
    a_width: f32,
    a_length: f64,
    b: &[Vec2],
    b_width: f32,
    b_length: f64,
    merge: bool,
) -> Option<(Vec2, Vec2)> {
    let threshold = f64::from(a_width + b_width) * 0.5;
    if !merge && lane_builder::polyline_distance(a, b) >= threshold {
        return None;
    }
    let mut zone_a = lane_builder::conflict_zone(a, b, threshold, CONFLICT_STEP);
    let mut zone_b = lane_builder::conflict_zone(b, a, threshold, CONFLICT_STEP);
    if merge {
        zone_a = Some(with_end_zone(zone_a, a_length));
        zone_b = Some(with_end_zone(zone_b, b_length));
    }
    Some((zone_a?, zone_b?))
}

fn with_end_zone(zone: Option<Vec2>, length: f64) -> Vec2 {
    let start = (length - MERGE_ZONE_LENGTH).max(0.0) as f32;
    match zone {
        None => Vec2::new(start, length as f32),
        Some(zone) => Vec2::new(zone.x.min(start), length as f32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::street::space::{Separator, Strip};

    fn strip(use_: StripUse, width: f32, direction: Direction) -> Strip {
        Strip {
            use_,
            width,
            direction,
            access: None,
            left: Separator::None,
            right: Separator::None,
            locked: false,
        }
    }

    #[test]
    fn one_lane_per_travel_strip_with_its_own_width() {
        let profile = Profile {
            strips: vec![
                strip(StripUse::Sidewalk, 3.0, Direction::None),
                strip(StripUse::CarLane, 3.0, Direction::Backward),
                strip(StripUse::CarLane, 3.0, Direction::Forward),
                strip(StripUse::BikeLane, 1.5, Direction::Forward),
                strip(StripUse::Sidewalk, 2.5, Direction::None),
            ],
        };
        // Centerline along +x; y down, so "right" of travel is +y.
        let centerline = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)];
        let specs = strip_lanes(&centerline, &profile, 10.0, 5.0);
        assert_eq!(specs.len(), 3);
        // Total 13 m: the backward lane spans -3.5..-0.5, middle -2.0.
        let back = &specs[0];
        assert!(!back.forward);
        assert_eq!(back.points[0], Vec2::new(95.0, -2.0));
        assert_eq!(*back.points.last().unwrap(), Vec2::new(10.0, -2.0));
        assert_eq!(back.lateral, 2.0);
        let bike = &specs[2];
        assert_eq!(bike.width, 1.5);
        assert_eq!(bike.access, Access::BICYCLE);
        assert_eq!(bike.points[0], Vec2::new(10.0, 3.25));
        assert_eq!(bike.lateral, 3.25);
    }

    #[test]
    fn turns_are_classified_y_down() {
        assert_eq!(classify_turn(Vec2::X, Vec2::X), Turn::Straight);
        assert_eq!(classify_turn(Vec2::X, Vec2::Y), Turn::Right);
        assert_eq!(classify_turn(Vec2::X, -Vec2::Y), Turn::Left);
    }

    #[test]
    fn lane_targets_follow_the_old_rules() {
        assert_eq!(target_index(Turn::Straight, 1, 2, 1, true), Some(0));
        assert_eq!(target_index(Turn::Right, 0, 2, 2, true), None);
        assert_eq!(target_index(Turn::Right, 1, 2, 2, true), Some(1));
        assert_eq!(target_index(Turn::Left, 0, 2, 2, true), Some(0));
        assert_eq!(target_index(Turn::Straight, 0, 1, 2, false), Some(1));
    }

    #[test]
    fn crossing_paths_conflict_parallel_ones_do_not() {
        let a = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)];
        let b = [Vec2::new(5.0, -5.0), Vec2::new(5.0, 5.0)];
        let (zone_a, zone_b) = conflict_zones(&a, 3.0, 10.0, &b, 3.0, 10.0, false).unwrap();
        assert!(zone_a.x < 5.0 && zone_a.y > 5.0);
        assert!(zone_b.x < 5.0 && zone_b.y > 5.0);
        let c = [Vec2::new(0.0, 3.0), Vec2::new(10.0, 3.0)];
        assert_eq!(conflict_zones(&a, 3.0, 10.0, &c, 3.0, 10.0, false), None);
        let (merge_a, _) = conflict_zones(&a, 3.0, 10.0, &c, 3.0, 10.0, true).unwrap();
        assert_eq!(merge_a, Vec2::new(5.0, 10.0));
    }
}
