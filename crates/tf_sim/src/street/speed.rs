//! The speed a street asks for by its look (pivot plan §5.7, addition 1). Real drivers
//! pick their speed mostly from what they see, not from the sign: a narrow lane, parked
//! cars, kerbs, trees and bollards close to the lane and a short view all slow them; a
//! wide, empty lane speeds them up ("self-explaining roads"; Fitzpatrick et al. 2000 /
//! NCHRP 504 for lane width and operating speed; Ewing & Dumbaugh 2009 for side friction
//! and trees). The sign has a strong effect on compliant drivers only (`drive::desired_speed`
//! mixes the two by compliance).
//!
//! The result is one speed per road lane, in m/s, from the geometry alone (never from the
//! sign). The street does not change during a run, so it is computed once.
//!
//! All figures are start values of our own, not fitted to data: Phase 8 calibration. The
//! formula uses only `+ - * /`, `min`, `max` and `clamp`, so every
//! platform gives the same bits.

use glam::Vec2;
use indexmap::IndexMap;

use super::ids::StreetLaneId;
use super::lanes::StreetLane;
use super::level::Shape;
use super::model::Street;
use super::space::{Profile, Separator, StripUse};
use crate::geom::segment::closest_point_to_segment;

/// The speed on a lane of the reference width with nothing at its sides, m/s (54 km/h).
/// Own start value, in the range NCHRP 504 reports for 3.3 to 3.6 m urban lanes.
pub const FREE_SPEED: f64 = 15.0;
/// The lane width from which width stops costing speed, metres.
pub const REFERENCE_WIDTH: f64 = 3.5;
/// Speed lost per metre a lane is narrower than the reference, m/s (own start value).
pub const NARROW_SLOPE: f64 = 4.0;
/// Speed gained per metre a lane is wider than the reference, m/s. Smaller than the
/// loss: wide lanes add less than narrow ones take away (own start value).
pub const WIDE_SLOPE: f64 = 2.0;
/// Width above which a wider lane adds nothing more, metres.
pub const WIDE_CAP: f64 = 4.5;
/// Speed lost per unit of side friction, m/s. Friction runs from 0 (nothing) to about 1
/// per side (a parking strip or a lined edge); own start value.
pub const FRICTION_SLOPE: f64 = 2.0;
/// No driver wants more than this times the street speed: the street speed is already what
/// drivers choose, and the spread of free-flow speeds around it is about +-15-20 %
/// (85th percentile about 1.15-1.2 times the mean). Own start value, Phase 8 calibration.
pub const STREET_TOLERANCE: f64 = 1.2;
/// Never slower than this, m/s (15 km/h: a walking-pace shared street still flows).
pub const MIN_SPEED: f64 = 15.0 / 3.6;
/// Never faster than this, m/s (59 km/h).
pub const MAX_SPEED: f64 = 16.5;

/// Friction of what lies beside a lane (own start values).
pub const FRICTION_PARKING: f64 = 1.0;
/// A kerb with a sidewalk right next to the lane: people, doors, things stepping out.
pub const FRICTION_SIDEWALK: f64 = 0.6;
pub const FRICTION_TERRACE: f64 = 0.7;
/// Another car lane behind a painted line: a low friction.
pub const FRICTION_CAR_LANE: f64 = 0.1;
pub const FRICTION_BIKE_LANE: f64 = 0.3;
/// A separated strip (bicycle track, planting, median): medium.
pub const FRICTION_SEPARATED: f64 = 0.5;
/// Added on a side by a physical separator (bollards, planters) on top of the strip.
pub const FRICTION_PHYSICAL_EDGE: f64 = 0.6;

/// An obstacle counts when its nearest point is this close to the lane edge, metres.
pub const OBSTACLE_REACH: f64 = 1.0;
/// Weight of a tall obstacle (`ObstacleKind::blocks_sight`) next to the lane, against 1
/// for a low one: it also cuts the view to the side, which makes drivers slow down.
pub const SIGHT_BLOCKER_WEIGHT: f64 = 1.5;
/// Weighted obstacles per 10 m of lane (both sides) that make the lane fully lined.
pub const FULL_LINE_DENSITY: f64 = 3.0;
/// The friction of a fully lined lane, both sides together.
pub const FRICTION_LINED: f64 = 1.0;

/// Friction of what is next to strip `index` of `profile`, on both sides, from the strips
/// there and their separators. The edge of the profile counts as a kerb with a sidewalk.
pub fn strip_friction(profile: &Profile, index: usize) -> f64 {
    let strip = &profile.strips[index];
    let left = index.checked_sub(1).map(|i| profile.strips[i].use_);
    let right = profile.strips.get(index + 1).map(|s| s.use_);
    side_friction(left, strip.left) + side_friction(right, strip.right)
}

fn side_friction(neighbour: Option<StripUse>, separator: Separator) -> f64 {
    let base = match neighbour {
        None | Some(StripUse::Sidewalk) => FRICTION_SIDEWALK,
        Some(StripUse::Terrace) => FRICTION_TERRACE,
        Some(StripUse::Parking | StripUse::DeliveryBay) => FRICTION_PARKING,
        Some(StripUse::CarLane | StripUse::BusLane) => FRICTION_CAR_LANE,
        Some(StripUse::BikeLane) => FRICTION_BIKE_LANE,
        Some(StripUse::BikeTrack | StripUse::Planting | StripUse::Median) => FRICTION_SEPARATED,
    };
    match separator {
        Separator::Bollards | Separator::Planter => base + FRICTION_PHYSICAL_EDGE,
        Separator::None | Separator::Paint | Separator::Kerb => base,
    }
}

/// The friction of a lane lined by obstacles: `weighted` is the sum of their weights,
/// `length` the lane length in metres.
pub fn obstacle_friction(weighted: f64, length: f64) -> f64 {
    let density = weighted * 10.0 / length.max(1.0);
    (density / FULL_LINE_DENSITY).min(1.0) * FRICTION_LINED
}

/// The street speed of a lane of `width` metres with `side` friction from its strips and
/// `lined` friction from obstacles, m/s.
pub fn lane_speed(width: f64, side: f64, lined: f64) -> f64 {
    let width_term = if width < REFERENCE_WIDTH {
        -(REFERENCE_WIDTH - width) * NARROW_SLOPE
    } else {
        (width.min(WIDE_CAP) - REFERENCE_WIDTH) * WIDE_SLOPE
    };
    (FREE_SPEED + width_term - (side + lined) * FRICTION_SLOPE).clamp(MIN_SPEED, MAX_SPEED)
}

/// Obstacles are tested against the lane line at this spacing, metres.
pub const SAMPLE_STEP: f32 = 0.5;

/// Points every `step` metres along `line` (its own points are only the corners).
fn resample(line: &[Vec2], step: f32) -> Vec<Vec2> {
    let mut out = Vec::new();
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let n = ((a.distance(b) / step).ceil() as usize).max(1);
        for i in 0..n {
            out.push(a.lerp(b, i as f32 / n as f32));
        }
    }
    out.extend(line.last().copied());
    out
}

/// Distance from `point` to the shape (0 inside it).
fn distance_to(shape: &Shape, point: Vec2) -> f64 {
    match shape {
        Shape::Circle(c) => (f64::from(c.center.distance(point)) - f64::from(c.radius)).max(0.0),
        Shape::Polygon(p) => {
            if p.contains(point) {
                return 0.0;
            }
            p.edges()
                .map(|(a, b)| f64::from(closest_point_to_segment(point, a, b).distance(point)))
                .fold(f64::INFINITY, f64::min)
        }
    }
}

/// The weighted number of obstacles lining a lane: those whose nearest point to the lane's
/// line is within `OBSTACLE_REACH` of its edge.
fn weighted_obstacles(street: &Street, lane: &StreetLane, points: &[Vec2]) -> f64 {
    let half = f64::from(lane.width) * 0.5;
    street
        .obstacles
        .values()
        .filter(|obstacle| {
            let gap = points
                .iter()
                .map(|p| distance_to(&obstacle.shape, *p))
                .fold(f64::INFINITY, f64::min)
                - half;
            gap <= OBSTACLE_REACH
        })
        .map(|obstacle| {
            if obstacle.kind.blocks_sight() {
                SIGHT_BLOCKER_WEIGHT
            } else {
                1.0
            }
        })
        .sum()
}

/// The street speed of every road lane of `street` (movements are not in the map).
pub fn street_speeds(street: &Street) -> IndexMap<StreetLaneId, f64> {
    let mut speeds = IndexMap::new();
    for lane in street.lanes.values() {
        let (Some(segment), Some(strip)) = (lane.segment_id, lane.strip) else {
            continue;
        };
        let profile = &street.segments[&segment].profile;
        let side = strip_friction(profile, strip);
        let points = resample(&lane.polyline(), SAMPLE_STEP);
        let lined = obstacle_friction(weighted_obstacles(street, lane, &points), lane.length);
        speeds.insert(lane.id, lane_speed(f64::from(lane.width), side, lined));
    }
    speeds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::street::space::{Direction, Strip};

    fn strip(use_: StripUse, width: f32) -> Strip {
        Strip {
            use_,
            width,
            direction: if use_ == StripUse::CarLane {
                Direction::Forward
            } else {
                Direction::None
            },
            access: None,
            left: Separator::None,
            right: Separator::None,
            locked: false,
        }
    }

    fn kmh(v: f64) -> f64 {
        v * 3.6
    }

    #[test]
    fn a_wider_lane_is_faster() {
        let mut last = 0.0;
        for width in [2.5, 3.0, 3.5, 4.0, 4.5] {
            let v = lane_speed(width, 0.5, 0.0);
            assert!(v > last, "width {width}");
            last = v;
        }
    }

    #[test]
    fn more_at_the_edge_is_slower() {
        let open = lane_speed(3.2, strip_friction_of(StripUse::CarLane), 0.0);
        let parked = lane_speed(3.2, strip_friction_of(StripUse::Parking), 0.0);
        let lined = lane_speed(3.2, strip_friction_of(StripUse::CarLane), 1.0);
        assert!(parked < open && lined < open);
    }

    fn strip_friction_of(neighbour: StripUse) -> f64 {
        let profile = Profile {
            strips: vec![
                strip(neighbour, 2.0),
                strip(StripUse::CarLane, 3.2),
                strip(StripUse::CarLane, 3.2),
            ],
        };
        strip_friction(&profile, 1)
    }

    #[test]
    fn the_speed_stays_within_bounds() {
        for width in [0.5, 1.0, 3.0, 6.0, 12.0] {
            for side in [0.0, 1.0, 5.0] {
                for lined in [0.0, 1.0] {
                    let v = lane_speed(width, side, lined);
                    assert!(
                        (MIN_SPEED..=MAX_SPEED).contains(&v),
                        "{width} {side} {lined}"
                    );
                }
            }
        }
        assert_eq!(lane_speed(0.1, 9.0, 1.0), MIN_SPEED);
    }

    #[test]
    fn the_example_lanes_land_in_their_ranges() {
        // 3.5 m between two car lanes, nothing else.
        let open = Profile {
            strips: vec![
                strip(StripUse::CarLane, 3.5),
                strip(StripUse::CarLane, 3.5),
                strip(StripUse::CarLane, 3.5),
            ],
        };
        let v = kmh(lane_speed(3.5, strip_friction(&open, 1), 0.0));
        assert!((50.0..=55.0).contains(&v), "open lane {v}");
        // 3.0 m between a parking strip and a painted line.
        let parked = Profile {
            strips: vec![
                strip(StripUse::Parking, 2.0),
                strip(StripUse::CarLane, 3.0),
                strip(StripUse::CarLane, 3.0),
            ],
        };
        let v = kmh(lane_speed(3.0, strip_friction(&parked, 1), 0.0));
        assert!((35.0..=45.0).contains(&v), "parked lane {v}");
        // 2.75 m lined by bollards on both sides.
        let mut bollards = Profile {
            strips: vec![
                strip(StripUse::CarLane, 3.0),
                strip(StripUse::CarLane, 2.75),
                strip(StripUse::CarLane, 3.0),
            ],
        };
        bollards.strips[1].left = Separator::Bollards;
        bollards.strips[1].right = Separator::Bollards;
        let v = kmh(lane_speed(2.75, strip_friction(&bollards, 1), 0.0));
        assert!((25.0..=35.0).contains(&v), "bollard lane {v}");
        let lined = kmh(lane_speed(2.75, strip_friction(&open, 1), FRICTION_LINED));
        assert!((25.0..=35.0).contains(&lined), "lined lane {lined}");
    }

    #[test]
    fn obstacle_density_saturates() {
        assert_eq!(obstacle_friction(0.0, 100.0), 0.0);
        assert!(obstacle_friction(3.0, 100.0) < obstacle_friction(6.0, 100.0));
        assert_eq!(obstacle_friction(100.0, 10.0), FRICTION_LINED);
    }
}
