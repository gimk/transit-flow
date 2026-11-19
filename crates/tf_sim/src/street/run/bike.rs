//! The side position of road users in their lane, and what cyclists add to the driving
//! rules (pivot plan §5.2, Phase 6 package 1).
//!
//! A body has a `lateral` offset from the middle of its lane (to the right of its
//! direction of travel). It is real state: the body is placed there, the others see it
//! (`Seen::lateral`) and a contact is tested there. Three things move it:
//!
//! - **The rest position.** A cyclist in a lane shared with motor traffic rides near the
//!   right edge (0.8-1.0 m from it, more than 1 m from the side of parked cars because of
//!   opening doors); in a bicycle lane or track it rides in the middle; on a hostile
//!   street a rider may take the sidewalk. A car rests in the middle.
//! - **Passing.** A car behind a cyclist that is slower than it wants to go, or a cyclist
//!   behind a slower cyclist, moves sideways to leave room (1 m in town for a car, R414-4)
//!   and goes by, only if the room exists (the lane, or a neighbouring lane the paint lets
//!   it use) and nothing comes the other way in the stretch it needs. Otherwise it waits
//!   behind: that is delay, and delay is scored.
//! - **Filtering.** A cyclist who has the habit rides past a queue of standing vehicles,
//!   on the right when the lane is wide enough, else on the left (as a pass).
//!
//! Every choice is a pure function of the snapshot, so the order of the agents does not
//! matter.

use glam::Vec2;

use super::agent::Agent;
use super::cross;
use super::drive::{SIDE_MARGIN, Seen, Snapshot, World, desired_speed_at};
use super::perceive;
use crate::street::ids::{AgentId, StreetLaneId};
use crate::street::lanes::{StreetLane, Turn};
use crate::street::model::Street;
use crate::street::signals::Light;
use crate::street::space::{Access, Direction, StripUse};

/// How fast the side position changes, metres per second (start value).
pub const LATERAL_SPEED: f64 = 1.0;
/// A cyclist keeps at least this far from the side of parked cars: car doors open about
/// 1 m (start value, French guidance for cyclists).
pub const DOOR_ZONE_M: f64 = 1.0;
/// The side room a driver leaves a cyclist in town: 1 m (Code de la route R414-4; 1.5 m
/// outside built-up areas). Drivers with low compliance leave less (`pass_clearance`).
pub const PASS_CLEARANCE_TOWN_M: f64 = 1.0;
/// A cyclist's side room when it passes a slower cyclist or a queue (start values).
pub const BIKE_PASS_CLEARANCE_M: f64 = 0.5;
pub const BIKE_QUEUE_CLEARANCE_M: f64 = 0.4;
/// The share of the rule a driver keeps when it squeezes past instead of waiting (start
/// value: 0.4 m for a car, 0.2 m for a rider).
pub const SQUEEZE_ROOM_FACTOR: f64 = 0.4;
/// Room a driver adds to the rule when it passes as the rule asks, so that the room it
/// keeps stays above the rule while it eases sideways (start value, metres).
pub const PASS_MARGIN_M: f64 = 0.3;
/// Room kept on the right of a queue when a rider squeezes by on that side, metres.
const RIGHT_SQUEEZE_M: f64 = 0.15;
/// A pass is started when the gap to what is passed is under this, metres (start values).
const CAR_PASS_TRIGGER_M: f64 = 12.0;
const BIKE_PASS_TRIGGER_M: f64 = 10.0;
/// The longest stretch a pass is planned for, and the open road a pass needs beyond it, m.
const MAX_PASS_SPAN_M: f64 = 120.0;
/// The pace an oncoming road user is assumed to have at least when a pass is checked, m/s.
const ONCOMING_PACE: f64 = 5.0;
/// The fastest the oncoming traffic is assumed to come at, when looking for it before a
/// pass (m/s; a street limit of 50 km/h plus a margin).
const ONCOMING_MAX_SPEED: f64 = 15.0;
const OPEN_ROAD_M: f64 = 60.0;
/// A car starts no pass this near a junction line ahead or the exit behind (start value,
/// metres; French rules forbid passing near an intersection without clear view).
const NO_PASS_NEAR_JUNCTION_M: f64 = 30.0;
/// A body moving back into the lane of a follower waits when the follower could not stop: it
/// is assumed to brake at this rate, m/s², keep this time margin on top of its reaction, s,
/// and this much room, m (start values).
const FOLLOWER_BRAKE: f64 = 3.0;
const FOLLOWER_MARGIN_S: f64 = 0.5;
const FOLLOWER_ROOM_M: f64 = 1.0;
/// A pass that would take longer than this is not started, seconds.
const PASS_MAX_S: f64 = 15.0;
/// What is passed must be this much slower than what the passer wants, m/s.
const PASS_SPEED_MARGIN: f64 = 1.0;
/// A vehicle slower than this is a standing queue for a rider who filters, m/s.
const QUEUE_SPEED: f64 = 1.5;
/// The corridor of a pass is checked for others with this much side margin, metres, and
/// for the time the passer needs at no less than this speed, m/s.
const CORRIDOR_MARGIN_M: f64 = 0.5;
const CORRIDOR_MIN_SPEED: f64 = 3.0;
/// Others this near a junction (metres) count as its crossing traffic for a red light.
const CROSS_VIEW_M: f32 = 35.0;
/// Seen from a rider, a street starts to feel hostile above this speed (the limit of a 30
/// zone) and fully so `HOSTILE_RANGE` above it, m/s (start values).
const HOSTILE_FROM: f64 = 30.0 / 3.6;
const HOSTILE_RANGE: f64 = 4.2;
/// How fast a rider goes on a sidewalk, m/s.
pub const SIDEWALK_SPEED: f64 = 3.0;

/// What is beside a lane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Side {
    /// Half the lane's width.
    pub half: f64,
    /// The strip on the right of the direction of travel.
    pub right_use: Option<StripUse>,
    /// Metres beside the left edge that a body may use: the next travel strip when only
    /// paint separates it (a car passing a cyclist can use the other lane), else 0.
    pub left_room: f64,
    /// Offset of the middle of the first sidewalk to the right, from the lane's middle.
    pub sidewalk: Option<f64>,
}

pub fn side_of(street: &Street, lane: &StreetLane) -> Side {
    let half = f64::from(lane.width) * 0.5;
    let none = Side {
        half,
        right_use: None,
        left_room: 0.0,
        sidewalk: None,
    };
    let (Some(segment), Some(strip)) = (lane.segment_id, lane.strip) else {
        return none;
    };
    let strips = &street.segments[&segment].profile.strips;
    let forward = strips[strip].direction == Direction::Forward;
    let at = |k: isize| -> Option<(usize, &crate::street::space::Strip)> {
        let step = if forward { k } else { -k };
        usize::try_from(strip as isize + step)
            .ok()
            .and_then(|i| strips.get(i).map(|s| (i, s)))
    };
    // The next travel strip on the left, when the separator lets a car cross into it: a car
    // passing a cyclist uses it (Phase 6 W4b: paint lets it, a kerb, bollards or a planter
    // do not). A bicycle strip is not a lane for a car to pass in.
    let left_room = match at(-1) {
        Some((index, next)) if next.use_.is_travel() && next.access().intersects(Access::MOTOR) => {
            let boundary = strip.max(index);
            if street.crossable_by(segment, boundary, Access::CAR) {
                f64::from(next.width)
            } else {
                0.0
            }
        }
        _ => 0.0,
    };
    let mut sidewalk = None;
    let mut distance = half;
    let mut k = 1;
    while let Some((_, next)) = at(k) {
        if next.use_ == StripUse::Sidewalk {
            sidewalk = Some(distance + f64::from(next.width) * 0.5);
            break;
        }
        distance += f64::from(next.width);
        k += 1;
    }
    Side {
        half,
        right_use: at(1).map(|(_, s)| s.use_),
        left_room,
        sidewalk,
    }
}

/// How hostile a lane feels to a rider, 0 (a 30 zone or calmer) to 1: the speed cars
/// choose there, the lower of the sign and the look of the street.
pub fn hostility(lane: &StreetLane, street_speed: Option<f64>) -> f64 {
    let speed = street_speed.map_or(lane.speed_limit, |s| s.min(lane.speed_limit));
    ((speed - HOSTILE_FROM) / HOSTILE_RANGE).clamp(0.0, 1.0)
}

/// The chance that this rider takes the sidewalk on `lane` (a lane it shares with motor
/// traffic; nobody leaves a bicycle lane or track for it). The habit grows as compliance
/// falls; the other factor is how hostile the street is.
pub fn sidewalk_chance(agent: &Agent, lane: &StreetLane, street_speed: Option<f64>) -> f64 {
    if !lane.access.intersects(Access::MOTOR) {
        return 0.0;
    }
    let habit = 0.15 + 0.7 * (1.0 - agent.traits.compliance);
    habit * hostility(lane, street_speed)
}

fn rides_on_sidewalk(
    agent: &Agent,
    lane: &StreetLane,
    lane_id: StreetLaneId,
    world: &World,
) -> bool {
    agent.bike.is_some_and(|habits| {
        habits.sidewalk_u < sidewalk_chance(agent, lane, world.street_speeds.mean(lane_id))
    })
}

/// True when this agent is a cyclist on a sidewalk now (its lane's chance and its habit).
pub fn on_sidewalk(agent: &Agent, world: &World) -> bool {
    let route = &world.routes[agent.route];
    let lane_id = route.lanes[route.lane_index(agent.d)];
    if world.moves.contains_key(&lane_id) {
        return false;
    }
    let lane = &world.street.lanes[&lane_id];
    rides_on_sidewalk(agent, lane, lane_id, world) && side_of(world.street, lane).sidewalk.is_some()
}

/// Where the agent rides in `lane_id` when nothing is in the way.
pub fn rest_lateral(agent: &Agent, lane_id: StreetLaneId, world: &World) -> f64 {
    let Some(habits) = agent.bike else {
        return 0.0;
    };
    if world.moves.contains_key(&lane_id) {
        return 0.0;
    }
    let lane = &world.street.lanes[&lane_id];
    let side = side_of(world.street, lane);
    if rides_on_sidewalk(agent, lane, lane_id, world)
        && let Some(offset) = side.sidewalk
    {
        return offset;
    }
    if !lane.access.intersects(Access::MOTOR) {
        return 0.0;
    }
    let edge = match side.right_use {
        Some(StripUse::Parking | StripUse::DeliveryBay) => habits.edge_m.max(DOOR_ZONE_M),
        _ => habits.edge_m,
    };
    let limit = (side.half - agent.width * 0.5).max(0.0);
    (side.half - edge).clamp(-limit, limit)
}

/// The chance that a rider rides past a queue (0 for full compliance and no taste for
/// risk, so the careful reference never does).
pub fn filter_chance(agent: &Agent) -> f64 {
    ((1.0 - agent.traits.compliance) * 0.8 + agent.traits.risk * 0.5).min(1.0)
}

/// The chance that a rider goes on red when the street it would cross is `empty` (nobody
/// of the crossing traffic within `CROSS_VIEW_M` of the junction): with nobody in view
/// most of those who bend the rule go, with someone coming few do. Frustration raises it.
pub fn red_chance(agent: &Agent, empty: bool) -> f64 {
    let habit = ((1.0 - agent.traits.compliance) * (1.0 + agent.frustration)).min(1.0);
    habit * if empty { 1.0 } else { 0.15 }
}

/// Is nobody coming from the other arms of the junction, in view of a rider at `line`?
pub fn cross_traffic_empty(
    agent: &Agent,
    node: i32,
    from_segment: i32,
    snapshot: &Snapshot,
    world: &World,
) -> bool {
    let centre = world.street.nodes[&node].position;
    let threat = |other: &Seen| -> bool {
        if other.id == agent.id || other.crashed || other.pos.distance(centre) > CROSS_VIEW_M {
            return false;
        }
        match other.movement {
            Some(movement) => world.moves[&movement].from_segment != from_segment,
            None => {
                let lane = &world.street.lanes[&other.lane];
                lane.to_node == node && lane.segment_id != Some(from_segment)
            }
        }
    };
    !snapshot.seen.iter().any(threat)
}

/// The side room the rule asks when `agent` passes `other`, metres: 1 m in town for a car
/// passing a rider (R414-4); a rider keeps less (start values).
pub fn rule_clearance(agent: &Agent, other: &Seen) -> f64 {
    if agent.bike.is_none() {
        PASS_CLEARANCE_TOWN_M
    } else if other.bike {
        BIKE_PASS_CLEARANCE_M
    } else {
        BIKE_QUEUE_CLEARANCE_M
    }
}

/// The chance that a driver squeezes past a rider with less room than the rule asks
/// instead of waiting: its lack of compliance, its risk, its frustration and how much it
/// is in a hurry (start values). A careful driver never does (compliance 1, no risk, no
/// hurry), so it waits or passes with the whole metre.
pub fn squeeze_chance(agent: &Agent) -> f64 {
    // `speed_factor` is a factor of the limit for a motor vehicle and a speed in m/s for a
    // rider: only a driver can be in a hurry.
    let hurry = if agent.bike.is_some() {
        0.0
    } else {
        (agent.traits.speed_factor - 1.0).max(0.0)
    };
    ((1.0 - agent.traits.compliance) * (0.5 + 0.5 * agent.frustration)
        + 0.5 * agent.traits.risk
        + 0.5 * hurry)
        .min(1.0)
}

/// Does this driver squeeze past now? One draw fixed per agent and per rule, so no stream
/// shifts and the same driver squeezes the same way all run.
pub fn squeezes(agent: &Agent) -> bool {
    perceive::roll(agent.gaze_seed, agent.id.0 as u64, cross::SQUEEZE_STREAM)
        < squeeze_chance(agent)
}

/// The room this driver leaves what it passes: the rule and a little more, or
/// `SQUEEZE_ROOM_FACTOR` of the rule for one that squeezes past. The margin keeps the room
/// the rule asks while the body eases sideways into the pass (start value).
pub fn pass_clearance(agent: &Agent, other: &Seen) -> f64 {
    let rule = rule_clearance(agent, other);
    if squeezes(agent) {
        rule * SQUEEZE_ROOM_FACTOR
    } else {
        rule + PASS_MARGIN_M
    }
}

/// Where the agent's side position is heading this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    pub target: f64,
    pub passing: bool,
    /// A pass just started with less room than the rule asks: the one passed.
    pub too_close: Option<AgentId>,
}

/// Is the stretch beside and ahead of the agent, shifted to `target`, free of everybody
/// but those it passes? Others are taken where they are and where they will be when the
/// passer is by.
fn corridor_clear(
    agent: &Agent,
    target: f64,
    passed: &[&Seen],
    span: f64,
    snapshot: &Snapshot,
) -> bool {
    let half = agent.width * 0.5;
    let tight = agent.bike.is_none() && squeezes(agent);
    let shift = (target - agent.lateral) as f32;
    let perp = agent.dir.perp();
    // Others are seen as they were one reaction time ago: that much is added.
    let seconds = span / agent.speed.max(CORRIDOR_MIN_SPEED) + agent.traits.reaction_s;
    let mut near = Vec::new();
    snapshot.grid.query(
        agent.pos,
        (span + 20.0 + ONCOMING_MAX_SPEED * seconds) as f32,
        &mut near,
    );
    for item in near {
        let other = &snapshot.seen[item as usize];
        if other.id == agent.id || passed.iter().any(|p| p.id == other.id) {
            continue;
        }
        // What comes the other way is given the margin of the rule, unless this driver is one
        // that squeezes: it takes a tight gap there too, with the traits that let it squeeze.
        let oncoming = other.dir.dot(agent.dir) < -0.5;
        let margin = if oncoming && tight {
            CORRIDOR_MARGIN_M * SQUEEZE_ROOM_FACTOR
        } else {
            CORRIDOR_MARGIN_M
        };
        let reach = (half + other.width * 0.5 + margin) as f32;
        let half_length = (other.length * 0.5 - other.width * 0.5).max(0.0) as f32;
        for k in [0.0, 0.5, 1.0] {
            let pace = if other.dir.dot(agent.dir) < -0.5 {
                // What comes the other way may be standing now and pull away before the
                // pass is done: it is taken at a usual street pace at least.
                other.speed.max(ONCOMING_PACE)
            } else {
                other.speed
            };
            let centre = other.pos + other.dir * (pace * seconds * k) as f32;
            for offset in [-half_length, 0.0, half_length] {
                let rel: Vec2 = centre + other.dir * offset - agent.pos;
                let along = f64::from(rel.dot(agent.dir));
                let side = rel.dot(perp) - shift;
                if along > -agent.length && along < span && side.abs() < reach {
                    return false;
                }
            }
        }
    }
    true
}

/// The side position this agent aims for, and whether it is passing.
pub fn lateral_plan(agent: &Agent, snapshot: &Snapshot, world: &World) -> Plan {
    hold_if_blocked(agent, plan_inner(agent, snapshot, world), snapshot)
}

fn plan_inner(agent: &Agent, snapshot: &Snapshot, world: &World) -> Plan {
    let route = &world.routes[agent.route];
    let index = route.lane_index(agent.d);
    let lane_id = route.lanes[index];
    let rest = rest_lateral(agent, lane_id, world);
    let stay = Plan {
        target: rest,
        passing: false,
        too_close: None,
    };
    if world.moves.contains_key(&lane_id) {
        // A pass that began on the road is finished inside the junction, so a body does
        // not swing back in beside the one it is still passing.
        if agent.passing && beside_anyone(agent, snapshot) {
            return Plan {
                target: agent.lateral_target,
                passing: true,
                too_close: None,
            };
        }
        return stay;
    }
    if agent.crashed() || on_sidewalk(agent, world) {
        return stay;
    }
    let lane = &world.street.lanes[&lane_id];
    let is_bike = agent.bike.is_some();
    let filters = agent
        .bike
        .is_some_and(|habits| habits.filter_u < filter_chance(agent));
    let here = agent.d - route.offsets[index];
    let wanted = desired_speed_at(agent, lane_id, here, world);
    let trigger = if is_bike {
        BIKE_PASS_TRIGGER_M
    } else {
        CAR_PASS_TRIGGER_M
    };

    // A driver that cuts the corner: it turns right across the painted bicycle lane before
    // the corner instead of at it (Phase 6 W4b). A kerb track forces the turn at the corner.
    if !is_bike && let Some(target) = cut_corner(agent, world, lane) {
        return Plan {
            target,
            passing: false,
            too_close: None,
        };
    }

    // A rider whose lane is blocked (a van stopped in it, an open door) leaves it for the
    // lane beside, when the separator lets a bicycle cross and the way is free; else it
    // brakes for the obstacle (`nearest_obstacle`). A kerb track keeps it in: it waits.
    if is_bike && let Some(target) = leave_lane(agent, snapshot, world, lane) {
        return Plan {
            target,
            passing: true,
            too_close: None,
        };
    }

    // What is in the way and would be passed.
    let mut passed: Vec<&Seen> = Vec::new();
    for other in &snapshot.seen {
        if other.id == agent.id || other.lane != lane_id {
            continue;
        }
        let front = route.offsets[index] + other.s;
        // Already behind. Others are seen as they were one reaction time ago, so what the
        // other has moved since is added, or the pass would end too soon.
        if front + other.speed * agent.traits.reaction_s <= agent.d - agent.length - 1.0 {
            continue;
        }
        if front - other.length - agent.d > trigger {
            continue;
        }
        // Something that stands is a queue (a rider with the habit rides past it, the
        // others wait); something that moves slowly is passed.
        let queue = other.speed < QUEUE_SPEED;
        let slow = !queue && other.speed + PASS_SPEED_MARGIN < wanted;
        // Once started, a pass goes on past whatever it began with, even if that has
        // stopped meanwhile (a car swinging back in beside a cyclist who has just stopped
        // would touch it).
        // A car may use the painted bicycle lane on its right to pass a queue (Phase 6
        // W4b): a kerb, bollards or a planter keep it out.
        let queue_pass = !is_bike && queue && cross::bends(agent, cross::QUEUE_STREAM);
        let qualifies = if agent.passing {
            other.bike || (filters && queue)
        } else if is_bike {
            (other.bike && slow) || (filters && queue)
        } else {
            (other.bike && slow) || queue_pass
        };
        if qualifies {
            passed.push(other);
        }
    }
    if passed.is_empty() {
        return stay;
    }

    let side = side_of(world.street, lane);
    let half = agent.width * 0.5;
    let mut left = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut reduced = None;
    let mut farthest: f64 = 0.0;
    let mut all_queue = true;
    for other in &passed {
        let clearance = pass_clearance(agent, other);
        if clearance < rule_clearance(agent, other) - 1e-9 && reduced.is_none() {
            reduced = Some(other.id);
        }
        left = left.min(other.lateral - other.width * 0.5 - clearance - half);
        right = right.max(other.lateral + other.width * 0.5 + RIGHT_SQUEEZE_M + half);
        farthest = farthest.max(route.offsets[index] + other.s - agent.d);
        all_queue &= other.speed < QUEUE_SPEED;
    }
    let target_left = left.min(rest);
    // A rider keeps to its own lane when it passes: the lane of the oncoming traffic is for
    // cars (a rider there meets a car pulling away from the junction line).
    let left_room = if is_bike { 0.0 } else { side.left_room };
    let left_ok = target_left >= -side.half - left_room + half;
    let target_right = right.max(rest);
    let right_ok = filters && all_queue && target_right + half <= side.half;
    // A car that passes a queue in the painted bicycle lane beside it.
    let bike_lane_pass = if !is_bike && all_queue {
        cross::neighbour(world.street, lane, true)
            .filter(|n| n.use_ == StripUse::BikeLane && n.lane.is_some())
            .filter(|n| {
                lane.segment_id.is_some_and(|segment| {
                    cross::may_cross(
                        world.street,
                        segment,
                        n,
                        Access::CAR,
                        cross::bends(agent, cross::QUEUE_STREAM),
                    )
                })
            })
            .map(|n| n.offset)
    } else {
        None
    };
    let target = if let Some(target) = bike_lane_pass {
        target
    } else if right_ok {
        target_right
    } else if left_ok {
        target_left
    } else {
        return stay;
    };

    if !agent.passing {
        let remaining = lane.length - here;
        // A car does not start to pass (into the lane of the oncoming traffic) near a
        // junction: the line ahead, or the exit just behind, where the other way starts and
        // stops without warning (two cars passing from opposite sides met there).
        if !is_bike && (remaining < NO_PASS_NEAR_JUNCTION_M || here < NO_PASS_NEAR_JUNCTION_M) {
            return stay;
        }
        // The ground the passer covers while it gains `farthest + its length + 3` metres on
        // the slowest of what it passes, from its own speed and pull-away: the whole of it
        // must be free, so a small speed difference asks for a long free stretch. A queue,
        // which stands, is ridden up to and asks for no more than its length.
        let relative = farthest + agent.length + 3.0;
        let span = if all_queue {
            relative
        } else {
            let others_speed = passed.iter().map(|o| o.speed).fold(0.0, f64::max);
            let accel = super::drive::accel_of(agent);
            match pass_distance(accel, agent.speed, wanted, others_speed, relative) {
                Some(distance) => distance.min(MAX_PASS_SPAN_M),
                None => return stay,
            }
        };
        if !all_queue {
            // Moving traffic is passed inside the lane, and with the open road ahead: the
            // oncoming lane beyond the end of the route is out of sight (a car entering
            // there would meet the passer head on).
            if remaining < span || route.length - agent.d < span + OPEN_ROAD_M {
                return stay;
            }
        }
        // Everybody else near the stretch must be out of it, in this lane or beyond.
        if !corridor_clear(agent, target, &passed, span.max(relative), snapshot) {
            return stay;
        }
    }
    Plan {
        target,
        passing: true,
        too_close: if agent.passing { None } else { reduced },
    }
}

/// How far before its line a driver may start to cut a corner, metres (start value).
const CUT_REACH_M: f64 = 25.0;

/// Where a driver that cuts a corner goes: the middle of the painted bicycle lane on its
/// right, when it turns right and the separator lets it cross. `None` otherwise, and for a
/// careful driver (it turns at the corner).
fn cut_corner(agent: &Agent, world: &World, lane: &StreetLane) -> Option<f64> {
    let approach = agent.approach.as_ref()?;
    let info = world.moves.get(&approach.movement)?;
    if info.turn != Turn::Right {
        return None;
    }
    let to_line = approach.line_d - agent.d;
    if !(0.0..CUT_REACH_M).contains(&to_line) {
        return None;
    }
    // A driver that waits at its line does not cut the corner: it cuts while it comes up
    // to the turn, on a green or without signals.
    if agent.speed < super::drive::STOPPED_SPEED {
        return None;
    }
    if info.signals
        && world.plans.get(&info.node).is_some_and(|plan| {
            matches!(plan.light(info.from_segment, world.time_s), Light::Red(_))
        })
    {
        return None;
    }
    let segment = lane.segment_id?;
    let side = cross::neighbour(world.street, lane, true)?;
    if side.use_ != StripUse::BikeLane || side.lane.is_none() {
        return None;
    }
    if !cross::may_cross(
        world.street,
        segment,
        &side,
        Access::CAR,
        cross::bends(agent, cross::CUT_STREAM),
    ) {
        return None;
    }
    Some(side.offset)
}

/// A vehicle that moves over waits for what comes up behind it in the strip it enters,
/// metres (start value: it does not pull in front of a rider).
const PULL_IN_CLEAR_M: f64 = 20.0;

/// The side position kept out of the bodies beside it and behind it: a vehicle that moves
/// over does not sweep into a rider that is alongside, and does not pull in front of one
/// that comes up behind it in the strip it enters (Phase 6 W4b). A van that pulls over into
/// the bicycle lane waits for the rider behind it to pass, and stops short of the one
/// beside it, instead of pushing it off the street.
pub fn lateral_limit(agent: &Agent, snapshot: &Snapshot, target: f64, pull_in: bool) -> f64 {
    if (target - agent.lateral).abs() < 1e-9 {
        return target;
    }
    let perp = agent.dir.perp();
    let mut limit = target;
    for other in &snapshot.seen {
        if other.id == agent.id {
            continue;
        }
        // The snapshot is one reaction time old: the other body is moved forward to where it
        // is now, as `hold_if_blocked` does. Without it a van that overtakes a rider loses
        // the limit while its tail still covers the rider.
        let centre = other.pos + other.dir * (other.speed * agent.traits.reaction_s) as f32;
        let rel = centre - agent.pos;
        let along = f64::from(rel.dot(agent.dir));
        let side = f64::from(rel.dot(perp));
        let room = (agent.width + other.width) * 0.5 + SIDE_MARGIN;
        // Alongside: its body overlaps ours, with a margin.
        let beside = along.abs() <= (agent.length + other.length) * 0.5 + SIDE_MARGIN;
        // In the strip we mean to enter, near enough to reach: behind and not falling back,
        // or ahead and slower, so that we would cut it off. Only away from a junction: at
        // the line the turn tables and the corridor check say what is safe.
        let near = pull_in
            && along.abs() < PULL_IN_CLEAR_M
            && if along < 0.0 {
                other.speed + 1.0 >= agent.speed
            } else {
                other.speed + 1.0 < agent.speed
            };
        if target > agent.lateral {
            if side > 0.0 && (beside || near) && side - room < limit {
                limit = (side - room).max(agent.lateral);
            }
        } else if side < 0.0 && (beside || near) && side + room > limit {
            limit = (side + room).min(agent.lateral);
        }
    }
    limit
}

/// How far ahead a rider looks for an obstacle that blocks its lane, metres (start value).
const LEAVE_REACH_M: f64 = 25.0;
/// The stretch beside and ahead that must be free before a rider leaves its lane, metres
/// (start value: it must be able to come back into its lane after the obstacle).
const LEAVE_SPAN_M: f64 = 20.0;

/// Where a rider goes round an obstacle that blocks its lane (a van stopped in the bicycle
/// lane, an open door): the middle of the travel strip beside it, when the separator lets a
/// bicycle cross and the way is free. `None` when it must brake instead (a kerb track, or a
/// body in the way).
fn leave_lane(agent: &Agent, snapshot: &Snapshot, world: &World, lane: &StreetLane) -> Option<f64> {
    // A motor vehicle that stands in the lane (a van at a delivery stop) or an open door:
    // a rider that stands ahead is a queue, which the rider waits behind or filters past.
    // The body is taken where it is, not by its lane: a van that pulled over into the
    // bicycle lane still has the lane of the car lane under its wheels.
    let perp = agent.dir.perp();
    let blocked = snapshot.seen.iter().any(|other| {
        if other.id == agent.id || other.bike || other.speed >= QUEUE_SPEED {
            return false;
        }
        let rel = other.pos - agent.pos;
        let along = f64::from(rel.dot(agent.dir));
        let side = f64::from(rel.dot(perp));
        along > -agent.length
            && along - other.length < LEAVE_REACH_M
            && side.abs() < (other.width + agent.width) * 0.5 + SIDE_MARGIN
    }) || world.doors.ahead(agent, world, LEAVE_REACH_M).is_some();
    if !blocked {
        return None;
    }
    let segment = lane.segment_id?;
    for right in [false, true] {
        let Some(side) = cross::neighbour(world.street, lane, right) else {
            continue;
        };
        let Some(beside) = side.lane else {
            continue;
        };
        if !world.street.lanes[&beside].access.contains(Access::BICYCLE) {
            continue;
        }
        // Never into the lane of the oncoming traffic: a rider there meets a car head on.
        if !cross::same_direction(world.street, lane, &side) {
            continue;
        }
        // Going round an obstacle in its own lane is not bending a rule: paint is enough,
        // a kerb, bollards or a planter are not.
        if !cross::may_cross(world.street, segment, &side, Access::BICYCLE, true) {
            continue;
        }
        let target = side.offset.clamp(-side.width * 0.5, side.width * 0.5);
        if corridor_clear(agent, target, &[], LEAVE_SPAN_M, snapshot) {
            return Some(target);
        }
    }
    None
}

/// The ground a body covers while it gains `relative` metres on one that rides at
/// `other` m/s, starting at `v0` and pulling away at `accel` up to `wanted` (with a margin
/// of a fifth); `None` when it cannot within `PASS_MAX_S`.
fn pass_distance(accel: f64, v0: f64, wanted: f64, other: f64, relative: f64) -> Option<f64> {
    const STEP_S: f64 = 0.25;
    if wanted <= other + 0.5 {
        return None;
    }
    let top = wanted.max(v0);
    let (mut t, mut v, mut ground, mut gained) = (0.0, v0, 0.0, 0.0);
    while t < PASS_MAX_S {
        v = (v + accel * STEP_S).min(top);
        ground += v * STEP_S;
        gained += (v - other) * STEP_S;
        t += STEP_S;
        if gained >= relative {
            return Some(ground * 1.2);
        }
    }
    None
}

/// Is anybody beside the agent (level with its body, within a lane or two)?
fn beside_anyone(agent: &Agent, snapshot: &Snapshot) -> bool {
    let perp = agent.dir.perp();
    let mut near = Vec::new();
    snapshot.grid.query(agent.pos, 10.0, &mut near);
    near.into_iter().any(|item| {
        let other = &snapshot.seen[item as usize];
        if other.id == agent.id {
            return false;
        }
        let centre = other.pos + other.dir * (other.speed * agent.traits.reaction_s) as f32;
        let rel = centre - agent.pos;
        f64::from(rel.dot(agent.dir)).abs() < (agent.length + other.length) * 0.5 + 1.0
            && f64::from(rel.dot(perp)).abs() < 3.0
    })
}

/// Moving sideways into a body that is beside the agent would be a contact: the agent
/// keeps its place until the side is clear.
fn hold_if_blocked(agent: &Agent, plan: Plan, snapshot: &Snapshot) -> Plan {
    let step = plan.target - agent.lateral;
    if step.abs() < 1e-9 {
        return plan;
    }
    let perp = agent.dir.perp();
    let mut near = Vec::new();
    snapshot.grid.query(agent.pos, 16.0, &mut near);
    for item in near {
        let other = &snapshot.seen[item as usize];
        if other.id == agent.id {
            continue;
        }
        let centre = other.pos + other.dir * (other.speed * agent.traits.reaction_s) as f32;
        let rel = centre - agent.pos;
        let along = f64::from(rel.dot(agent.dir));
        let band = (agent.width + other.width) * 0.5 + 0.1;
        // Swinging back into the lane of a follower that is coming up behind and could not
        // stop for the body in its way: wait until it is by (the follower saw this body
        // beside its own path, not in it, so it did not brake).
        if other.dir.dot(agent.dir) > 0.7 && along < 0.0 {
            let side_now = f64::from(rel.dot(perp));
            let behind = -along - (agent.length + other.length) * 0.5;
            let stop = other.speed * other.speed / (2.0 * FOLLOWER_BRAKE)
                + other.speed * (agent.traits.reaction_s + FOLLOWER_MARGIN_S)
                + FOLLOWER_ROOM_M;
            let in_band_now = side_now.abs() < band;
            let in_band_then = (side_now - step).abs() < band;
            if behind < stop && other.speed > agent.speed && in_band_then && !in_band_now {
                return Plan {
                    target: agent.lateral,
                    ..plan
                };
            }
        }
        if along.abs() > (agent.length + other.length) * 0.5 + 0.5 {
            continue;
        }
        let side = f64::from(rel.dot(perp));
        let need = (agent.width + other.width) * 0.5 + 0.1;
        // The other moves sideways too, and is seen as it was a reaction time ago.
        let slack = 0.15 + LATERAL_SPEED * agent.traits.reaction_s;
        if side * step > 0.0 && side.abs() < need + slack {
            return Plan {
                target: agent.lateral,
                ..plan
            };
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostility_is_zero_in_a_30_zone_and_full_at_50() {
        let mut lane = crate::street::lanes::StreetLane {
            id: StreetLaneId(0),
            segment_id: Some(1),
            strip: Some(0),
            from_node: 0,
            to_node: 1,
            curve: crate::geom::Curve2D::from_points(&[Vec2::ZERO, Vec2::X]),
            length: 1.0,
            width: 3.0,
            access: Access::CAR | Access::BICYCLE,
            lateral: 0.0,
            speed_limit: 30.0 / 3.6,
            turn: crate::street::lanes::Turn::None,
            feeder: None,
            outgoing: Vec::new(),
            conflict_zones: indexmap::IndexMap::new(),
        };
        assert_eq!(hostility(&lane, None), 0.0);
        lane.speed_limit = 50.0 / 3.6;
        assert_eq!(hostility(&lane, None), 1.0);
        // A narrow street calms a 50 sign.
        assert_eq!(hostility(&lane, Some(6.0)), 0.0);
    }
}
