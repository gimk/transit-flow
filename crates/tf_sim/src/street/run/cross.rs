//! Leaving a lane (Phase 6 W4b, pivot plan §3.1 and §8 "classic conflicts"): what the
//! separators of the street allow. A vehicle leaves its lane to pass, to stop, to turn
//! early or to go round an obstacle; a rider leaves its lane to go round a van stopped in
//! it or an open door. `Separator::crossability` decides: `Blocked` never, `Paint` only
//! for a driver who bends the rule, `Free` always. Until this package nothing in the run
//! read it, so a painted lane and a kerb track behaved the same.
//!
//! The rule is read in one place (`may_cross`) so that a kerb, bollards or a planter stop
//! every vehicle the same way, and so that a painted line is a rule to bend and not a
//! wall. Whether a driver bends it is one draw fixed per agent and per rule (`bends`),
//! compared with a chance that grows with its lack of compliance, its risk and its
//! frustration: one person can stop on a painted lane and still not cut a corner.

use super::agent::Agent;
use super::perceive;
use crate::street::ids::StreetLaneId;
use crate::street::lanes::StreetLane;
use crate::street::model::Street;
use crate::street::space::{Access, Crossability, Direction, StripUse};

/// The travel strip beside a lane, and the boundary between the two.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Neighbour {
    /// The strip index in the segment profile.
    pub strip: usize,
    /// The boundary between the two strips, for `Street::boundary_crossability`.
    pub boundary: usize,
    pub use_: StripUse,
    /// The lane of that strip, when it is a travel strip with a direction.
    pub lane: Option<StreetLaneId>,
    /// Offset of that strip's middle from this lane's middle, to the right of travel
    /// (negative is left), metres.
    pub offset: f64,
    pub width: f64,
    /// The way that strip runs, for telling the lane beside from the oncoming one.
    pub direction: Direction,
}

/// The strip on the right (`right`) or on the left of `lane` in its segment profile.
/// `None` for a movement, at the edge of the profile, or for a lane without a strip.
pub fn neighbour(street: &Street, lane: &StreetLane, right: bool) -> Option<Neighbour> {
    let (segment, strip) = (lane.segment_id?, lane.strip?);
    let found = street.segments.get(&segment)?;
    let strips = &found.profile.strips;
    let forward = strips.get(strip)?.direction == Direction::Forward;
    // The profile runs from the left building line to the right one, looking along the
    // centerline: to the right of a lane that runs backwards is the strip before it.
    let step = if right == forward { 1 } else { -1 };
    let index = usize::try_from(strip as isize + step).ok()?;
    let next = strips.get(index)?;
    let middle = |i: usize| {
        let (a, b) = found.profile.strip_span(i);
        f64::from((a + b) * 0.5)
    };
    let mut offset = middle(index) - middle(strip);
    if !forward {
        offset = -offset;
    }
    let lane_id = found
        .lane_ids
        .iter()
        .copied()
        .find(|id| street.lanes[id].strip == Some(index));
    Some(Neighbour {
        strip: index,
        boundary: strip.max(index),
        use_: next.use_,
        lane: lane_id,
        offset,
        width: f64::from(next.width),
        direction: next.direction,
    })
}

/// Does the strip of `neighbour` run the same way as `lane`? A rider that leaves its lane
/// goes into the lane beside it in the same direction, never into the oncoming one.
pub fn same_direction(street: &Street, lane: &StreetLane, neighbour: &Neighbour) -> bool {
    let (Some(segment), Some(strip)) = (lane.segment_id, lane.strip) else {
        return false;
    };
    let Some(found) = street.segments.get(&segment) else {
        return false;
    };
    let strips = &found.profile.strips;
    strips.get(strip).map(|s| s.direction) == strips.get(neighbour.strip).map(|s| s.direction)
}

/// May a vehicle of `mode` cross from its lane into `neighbour`? `bends` says whether the
/// driver bends the rule: a painted line is crossable only for one who does, a kerb, a row
/// of bollards or a planter for nobody.
pub fn may_cross(
    street: &Street,
    segment: i32,
    neighbour: &Neighbour,
    mode: Access,
    bends: bool,
) -> bool {
    match street.boundary_crossability(segment, neighbour.boundary, mode) {
        Crossability::Free => true,
        Crossability::Paint => bends,
        Crossability::Blocked => false,
    }
}

/// The chance that a driver bends a rule that costs it time (crossing a painted line to
/// stop, to pass a queue, to cut a corner): its lack of compliance, its risk and its
/// frustration (start values).
pub fn bend_chance(agent: &Agent) -> f64 {
    ((1.0 - agent.traits.compliance) * (0.5 + 0.5 * agent.frustration) + 0.5 * agent.traits.risk)
        .min(1.0)
}

/// Does this agent bend the rule of `stream` now? One draw fixed per agent and per rule
/// (from the run seed and the id, so no stream shifts), compared with `bend_chance`.
pub fn bends(agent: &Agent, stream: u64) -> bool {
    perceive::roll(agent.gaze_seed, agent.id.0 as u64, stream) < bend_chance(agent)
}

/// The streams of the rules a driver may bend, one per rule.
pub const STOP_STREAM: u64 = 0x53_54_4F;
pub const QUEUE_STREAM: u64 = 0x51_55_45;
pub const CUT_STREAM: u64 = 0x43_55_54;
pub const SQUEEZE_STREAM: u64 = 0x53_51_5A;
/// The fixed draw of a rider that decides if it looks back before it moves out.
pub const LOOK_BACK_STREAM: u64 = 0x4C_4F_4B;

/// Chance that a van makes a delivery stop on its trip (start value: one van in four).
pub const VAN_STOP_CHANCE: f64 = 0.25;
/// Chance that a car does, times its lack of compliance: a careful driver never stops on a
/// bicycle lane (start value).
pub const CAR_STOP_CHANCE: f64 = 0.15;
/// How long a delivery stop lasts, seconds (start value: unloading a parcel).
pub const STOP_DURATION_S: f64 = 20.0;
/// A van pulls over into the bicycle lane this far before its stop place, metres (start
/// value: it does not drive in the lane for the whole street).
pub const STOP_APPROACH_M: f64 = 25.0;

/// Where a van may pull over into the bicycle lane beside its route: the middle of the
/// longest stretch of painted bicycle lane on its right, and the side position of that
/// lane. `None` when the route has no such stretch (a track, a kerb, no bicycle strip).
pub fn delivery_stop_place(
    street: &Street,
    route: &super::route::RoutePath,
    mode: Access,
) -> Option<(f64, f64)> {
    let mut best: Option<(f64, f64, f64)> = None;
    for (index, id) in route.lanes.iter().enumerate() {
        let lane = &street.lanes[id];
        let Some(segment) = lane.segment_id else {
            continue;
        };
        let Some(side) = neighbour(street, lane, true) else {
            continue;
        };
        if side.use_ != StripUse::BikeLane || side.lane.is_none() {
            continue;
        }
        if !may_cross(street, segment, &side, mode, true) {
            continue;
        }
        if best.is_none_or(|(length, _, _)| lane.length > length) {
            best = Some((
                lane.length,
                route.offsets[index] + lane.length * 0.5,
                side.offset,
            ));
        }
    }
    best.map(|(_, at_d, lateral)| (at_d, lateral))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::street::design::Design;
    use crate::street::level::LevelFile;

    fn street() -> Street {
        let level = LevelFile::load("carrefour-ecole").expect("the level loads");
        Street::from_level_and_design(&level, &Design::new(&level.id)).expect("builds")
    }

    /// Segment 1 runs west to east: strip 1 is the westbound car lane, strip 2 the
    /// eastbound one, strip 3 the parking strip on its right.
    #[test]
    fn the_neighbour_of_a_lane_is_the_strip_on_that_side_of_travel() {
        let street = street();
        let east = street
            .segment_lanes(1)
            .find(|lane| lane.strip == Some(2))
            .expect("the eastbound lane");
        let right = neighbour(&street, east, true).expect("a strip on the right");
        assert_eq!(right.strip, 3);
        assert_eq!(right.use_, StripUse::Parking);
        assert_eq!(right.boundary, 3);
        assert!((right.offset - 2.5).abs() < 0.01, "{right:?}");
        let left = neighbour(&street, east, false).expect("a strip on the left");
        assert_eq!(left.strip, 1);
        assert!((left.offset + 3.0).abs() < 0.01, "{left:?}");
        // The westbound lane runs backwards: its right is the strip before it.
        let west = street
            .segment_lanes(1)
            .find(|lane| lane.strip == Some(1))
            .expect("the westbound lane");
        assert_eq!(neighbour(&street, west, true).map(|n| n.strip), Some(0));
        assert_eq!(neighbour(&street, west, false).map(|n| n.strip), Some(2));
    }

    #[test]
    fn paint_lets_a_driver_that_bends_the_rule_cross_and_a_kerb_nobody() {
        let street = street();
        let east = street
            .segment_lanes(1)
            .find(|lane| lane.strip == Some(2))
            .expect("the eastbound lane");
        // Strip 2 has paint on its left (the centre line) and none on its right (parking).
        let centre = neighbour(&street, east, false).expect("a strip on the left");
        assert!(may_cross(&street, 1, &centre, Access::CAR, true));
        assert!(!may_cross(&street, 1, &centre, Access::CAR, false));
        // The kerb between the sidewalk and the westbound lane blocks every vehicle.
        let west = street
            .segment_lanes(1)
            .find(|lane| lane.strip == Some(1))
            .expect("the westbound lane");
        let sidewalk = neighbour(&street, west, true).expect("a strip on the right");
        assert_eq!(sidewalk.use_, StripUse::Sidewalk);
        assert!(!may_cross(&street, 1, &sidewalk, Access::CAR, true));
        assert!(!may_cross(&street, 1, &sidewalk, Access::BICYCLE, true));
    }
}
