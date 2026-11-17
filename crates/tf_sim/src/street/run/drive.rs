//! How a driver decides (pivot plan §5.1): from what it has seen (a snapshot as old as
//! its reaction time) and its own state, it picks an acceleration. The careful behaviour
//! is the old game's formulas, in metres: following speed from the gap and the leader's
//! speed, a critical gap per turn, priority to the right, "do not block the box". Here
//! they are what a careful driver does, not what the world enforces: the personality
//! bends them, and nothing else stops a body from reaching another.

use glam::Vec2;
use indexmap::IndexMap;

use super::agent::{Agent, Approach};
use super::events::CauseKind;
use super::grid::Grid;
use super::mix;
use super::route::RoutePath;
use super::walker::WalkerSeen;
use super::yielding::{self, PedWorld};
use crate::rng::SimRng;
use crate::street::ids::{AgentId, StreetLaneId};
use crate::street::lanes::Turn;
use crate::street::level::{Mode, Sign};
use crate::street::model::Street;
use crate::street::signals::{Light, SignalPlan};
use crate::street::speed::{CELL, STREET_TOLERANCE, StreetSpeeds};

/// Seconds per tick (20 Hz).
pub const DT: f64 = 0.05;
/// Metres per second squared.
pub const ACCELERATION: f64 = 2.0;
/// The braking a driver plans with.
pub const COMFORT_BRAKE: f64 = 3.0;
/// What the road surface allows, as a factor on a vehicle's own maximum braking: 1 is dry
/// asphalt. One constant so that weather can lower it later (not modelled yet).
pub const ROAD_GRIP: f64 = 1.0;
/// Seconds from the first touch of the pedal to full braking (start value; brake rise
/// times of 0.2-0.4 s are reported in the braking literature, e.g. NCHRP 400, Fambro
/// et al. 1997). Phase 8 calibration.
pub const BRAKE_RISE_S: f64 = 0.3;
/// The braking at which the pedal needs the whole `BRAKE_RISE_S` (about the middle of a
/// car's maximum, see `draw_max_brake`); softer braking needs a proportionally shorter rise.
const REFERENCE_BRAKE: f64 = 8.5;
/// Salt of the stream that draws a vehicle's maximum braking: a stream of its own, so
/// that the draws the agent makes later (distractions) keep their values.
const BRAKE_STREAM: u64 = 0xB4A4_E5ED;
/// Braking harder than this to avoid someone is a conflict; harder than
/// `SERIOUS_BRAKE`, a serious one.
pub const HARD_BRAKE: f64 = 4.5;
pub const SERIOUS_BRAKE: f64 = 7.0;
/// Bumper to bumper at a standstill.
pub const MIN_GAP: f64 = 2.0;
/// Headway kept on top of the reaction time, by a driver who takes no risk.
pub const HEADWAY_MARGIN_S: f64 = 0.6;
/// Sideways acceleration a driver accepts in a turn.
pub const TURN_ACCELERATION: f64 = 2.0;
/// How far ahead a driver looks at most, metres.
pub const MAX_LOOKAHEAD: f64 = 60.0;
/// The route ahead is sampled every this many metres to find bodies in the way.
pub const SAMPLE_STEP: f64 = 0.75;
/// Room kept on each side of the body when passing something.
pub const SIDE_MARGIN: f64 = 0.3;
/// A waiting driver stops this far before the line.
pub const STOP_MARGIN: f64 = 0.5;
/// The decision at the line is taken this far before the braking distance.
pub const DECIDE_MARGIN: f64 = 8.0;
/// The old game's critical gaps (seconds): the time kept between oneself and the other
/// at the place where the paths cross.
pub const GAP_STRAIGHT_S: f64 = 2.0;
pub const GAP_RIGHT_S: f64 = 1.5;
pub const GAP_LEFT_S: f64 = 2.5;
/// A driver who does not obey still stops at a red older than this.
pub const LATE_RED_S: f64 = 2.5;
/// Slower than this is stopped.
pub const STOPPED_SPEED: f64 = 0.3;
/// A stopped vehicle this close to its line is waiting at it.
pub const AT_LINE: f64 = 4.0;
/// After people on foot held it at its line, a driver gives way to everyone for this
/// long: the others ignored it meanwhile, and see it moving only after their reaction
/// time (the slowest archetype reacts within 1.6 s), seconds.
pub const HELD_DEFER_S: f64 = 2.0;

/// What is fixed about a movement for a run.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveInfo {
    pub node: i32,
    pub from_segment: i32,
    /// Unit heading at the stop line.
    pub heading: Vec2,
    pub turn: Turn,
    pub out_lane: StreetLaneId,
    /// What the bend allows a driver of speed factor 1 (infinite when straight).
    pub turn_speed: f64,
    /// 2 without a sign, 1 with a yield sign, 0 with a stop sign; all 2 under signals.
    pub rank: u8,
    pub sign: Sign,
    pub signals: bool,
}

/// One agent as the others see it.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub id: AgentId,
    pub pos: Vec2,
    pub dir: Vec2,
    pub length: f64,
    pub width: f64,
    pub speed: f64,
    /// The lane under its front bumper and the distance along it.
    pub lane: StreetLaneId,
    pub s: f64,
    /// The movement it is in or heading for, and its front's distance along it
    /// (negative before the stop line).
    pub movement: Option<StreetLaneId>,
    pub s_on_movement: f64,
    pub in_junction: bool,
    pub wait_ticks: u32,
    pub crashed: bool,
    /// A van or a bus: people behind it are hidden (`sight`).
    pub hides: bool,
    /// Held at its line by people on foot (the zebra past its exit is in use): it does
    /// not hold the drivers who give way to it.
    pub held_by_walkers: bool,
}

/// The world as it was after one tick.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub seen: Vec<Seen>,
    pub grid: Grid,
    /// The pedestrians, in id order, and their grid (`crowd::WALKER_CELL`).
    pub walkers: Vec<WalkerSeen>,
    pub walker_grid: Grid,
}

pub struct World<'a> {
    pub street: &'a Street,
    pub routes: &'a [RoutePath],
    pub moves: &'a IndexMap<StreetLaneId, MoveInfo>,
    pub plans: &'a IndexMap<i32, SignalPlan>,
    /// What the look of each road lane asks for (`street::speed`), m/s.
    pub street_speeds: &'a StreetSpeeds,
    pub time_s: f64,
    /// The walkers' world, for drivers who decide (`None` where only speeds are asked).
    pub peds: Option<&'a PedWorld<'a>>,
}

/// Why a driver does not enter the junction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Block {
    Red,
    Amber,
    StopSign,
    /// Gives way to this agent, who comes from this approach.
    Yield(AgentId, i32),
    /// This agent is in the junction, on a path that crosses.
    InJunction(AgentId, i32),
    ExitFull,
}

/// What a careful driver would have done at the zebra ahead that this one does not:
/// the walkers it would yield to, and (speed, careful limit) at a hidden kerb.
pub type CrossingPending = (Vec<AgentId>, Option<(f64, f64)>);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Decision {
    pub accel: f64,
    /// The decision at the line, when one was taken this tick.
    pub go: Option<bool>,
    pub stopped_at_sign: bool,
    /// The front has passed the line.
    pub committed: bool,
    /// What a careful driver would not have done, found when passing the line.
    pub causes: Vec<CauseKind>,
    /// Braked hard for this agent, at this rate.
    pub hard_brake: Option<(AgentId, f64)>,
    /// Coming up to a zebra: what a careful driver would have done that this one does
    /// not (the walker it would yield to, its speed against the hidden-kerb limit).
    pub crossing_pending: Option<CrossingPending>,
    /// The front has entered the zebra.
    pub crossing_entered: bool,
    /// Held at the line by people on the zebra past the junction exit.
    pub held_by_walkers: bool,
}

/// The hardest braking of one vehicle on a dry road, m/s², drawn once at spawn. Start
/// values (Phase 8 calibration): cars 7.5-9.5 (emergency stops of modern cars with ABS on
/// dry asphalt reach 8-10; Fambro et al. 1997, NCHRP 400, find drivers' emergency
/// decelerations around 0.55 g with a wide spread), vans 6.5-8.0, buses 5.0-6.5.
pub fn draw_max_brake(mode: Mode, seed: u64, id: AgentId) -> f64 {
    let (low, high) = match mode {
        Mode::Car => (7.5, 9.5),
        Mode::Van => (6.5, 8.0),
        _ => (5.0, 6.5),
    };
    let mut rng = SimRng::new(mix(mix(seed, id.0 as u64), BRAKE_STREAM));
    f64::from(rng.randf_range(low, high))
}

/// The deceleration that brings speed `v` down to `target` within `distance`
/// (`(v² - target²) / 2·distance`), m/s². This is what a driver needs to brake with, not
/// the jump to the target in one tick: a walker who steps out 25 m ahead of a car at
/// 30 km/h asks for 1.4 m/s², not 8.
pub fn needed_brake(v: f64, target: f64, distance: f64) -> f64 {
    if v <= target {
        return 0.0;
    }
    // The pedal takes `BRAKE_RISE_S` to build up to full braking: about half of it is lost
    // braking, so the distance that is left is shorter by that much driving. That is only
    // so for hard braking: a gentle brake is reached at once, its rise is short. Without
    // this scaling, a car 0.2 m/s over the speed of a lane that starts in 1 m would "need"
    // 10 m/s² and be logged as a hard brake (found on 20 km/h streets, where the junction
    // movement is a little slower than the road before it).
    let plain = (v * v - target * target) / (2.0 * distance.max(0.1));
    // A stop keeps the full loss: missing it by a few decimetres is a body on a zebra.
    let rise = if target > 0.0 {
        (plain / REFERENCE_BRAKE).min(1.0)
    } else {
        1.0
    };
    let left = distance - v * BRAKE_RISE_S * 0.5 * rise;
    (v * v - target * target) / (2.0 * left.max(0.1))
}

/// The acceleration applied this tick: braking builds up at a limited rate, full
/// braking `BRAKE_RISE_S` after the pedal is touched; letting go is at once.
pub fn brake_build_up(previous: f64, wanted: f64, max_brake: f64) -> f64 {
    if wanted >= previous {
        return wanted;
    }
    wanted.max(previous - max_brake / BRAKE_RISE_S * DT)
}

/// The speed from which one can still stop within `gap` behind a leader at `lead_speed`,
/// reacting after `tau` seconds and braking at `COMFORT_BRAKE` (the old following speed
/// `sqrt(v_lead² + 2·b·gap)`, with the reaction time taken out).
pub fn safe_speed(gap: f64, lead_speed: f64, tau: f64) -> f64 {
    if gap <= 0.0 {
        return 0.0;
    }
    let b = COMFORT_BRAKE;
    let root = b * b * tau * tau + lead_speed * lead_speed + 2.0 * b * gap;
    (root.sqrt() - b * tau).max(0.0)
}

/// Seconds to cover `distance` from `speed`, accelerating to `top_speed` (the old
/// `time_to_cover`).
pub fn travel_time(distance: f64, speed: f64, top_speed: f64) -> f64 {
    if distance <= 0.0 {
        return 0.0;
    }
    let top = top_speed.max(0.5);
    if speed >= top {
        return distance / speed;
    }
    let a = ACCELERATION;
    let ramp_time = (top - speed) / a;
    let ramp = speed * ramp_time + 0.5 * a * ramp_time * ramp_time;
    if ramp >= distance {
        ((speed * speed + 2.0 * a * distance).sqrt() - speed) / a
    } else {
        ramp_time + (distance - ramp) / top
    }
}

pub fn critical_gap(turn: Turn) -> f64 {
    match turn {
        Turn::Left | Turn::UTurn => GAP_LEFT_S,
        Turn::Right => GAP_RIGHT_S,
        Turn::Straight | Turn::None => GAP_STRAIGHT_S,
    }
}

fn turn_rank(turn: Turn) -> u8 {
    match turn {
        Turn::Straight | Turn::None => 0,
        Turn::Right => 1,
        Turn::Left | Turn::UTurn => 2,
    }
}

/// Does `me` give way to `other`? Signs first, then priority to the right (the French
/// default), then, face to face, the one who turns more.
pub fn must_yield(me: &MoveInfo, other: &MoveInfo) -> bool {
    if me.rank != other.rank {
        return me.rank < other.rank;
    }
    if me.from_segment == other.from_segment {
        return false;
    }
    // y down: a negative cross product means the other heads to my left, so it comes
    // from my right.
    let cross = me.heading.perp_dot(other.heading);
    if cross < -0.5 {
        return true;
    }
    if cross > 0.5 {
        return false;
    }
    turn_rank(me.turn) > turn_rank(other.turn)
}

/// The speed this agent wants on lane `id`. On a road lane the sign and the look of the
/// street (`street::speed`) are mixed by compliance: a fully compliant driver takes the
/// lower of the two, one who ignores the sign follows the street, and nobody goes over
/// `STREET_TOLERANCE` times the street speed (real drivers read the
/// street, not the sign). In a junction movement it is the limit and what the bend allows.
///
/// The street speed is the mean of the lane here; `desired_speed_at` takes the place along
/// the lane (the street speed changes along it).
pub fn desired_speed(agent: &Agent, id: StreetLaneId, world: &World) -> f64 {
    let street = world.street_speeds.mean(id);
    desired_speed_with(agent, id, street, world)
}

/// `desired_speed` at `s` metres along lane `id`.
pub fn desired_speed_at(agent: &Agent, id: StreetLaneId, s: f64, world: &World) -> f64 {
    let street = world.street_speeds.at(id, s);
    desired_speed_with(agent, id, street, world)
}

fn desired_speed_with(agent: &Agent, id: StreetLaneId, street: Option<f64>, world: &World) -> f64 {
    let lane = &world.street.lanes[&id];
    if let Some(info) = world.moves.get(&id) {
        let speed = lane.speed_limit * agent.traits.speed_factor;
        return speed.min(info.turn_speed * agent.traits.speed_factor);
    }
    let limit = lane.speed_limit;
    let street = street.unwrap_or(limit);
    let c = agent.traits.compliance;
    let wanted = agent.traits.speed_factor * (c * limit.min(street) + (1.0 - c) * street);
    wanted.min(STREET_TOLERANCE * street)
}

/// The time gap this agent keeps to what is ahead.
pub fn headway(agent: &Agent) -> f64 {
    agent.traits.reaction_s + HEADWAY_MARGIN_S * (1.0 - agent.traits.risk)
}

fn light_of(world: &World, info: &MoveInfo) -> Option<Light> {
    if !info.signals {
        return None;
    }
    world
        .plans
        .get(&info.node)
        .map(|plan| plan.light(info.from_segment, world.time_s))
}

/// May the agent enter the junction now? `careful` asks what a driver with full
/// compliance and attention and no taste for risk would answer from the same view.
pub fn junction_check(
    agent: &Agent,
    approach: &Approach,
    snapshot: &Snapshot,
    world: &World,
    careful: bool,
) -> Result<(), Block> {
    match junction_blocks(agent, approach, snapshot, world, careful, true).first() {
        Some(block) => Err(*block),
        None => Ok(()),
    }
}

/// Everything that stops the agent from entering (all of it, or just the first when
/// `first_only`): a contact is blamed on every rule the driver broke, not on the first
/// one the check happened to find.
pub fn junction_blocks(
    agent: &Agent,
    approach: &Approach,
    snapshot: &Snapshot,
    world: &World,
    careful: bool,
    first_only: bool,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    macro_rules! stop {
        ($block:expr) => {{
            blocks.push($block);
            if first_only {
                return blocks;
            }
        }};
    }
    let info = &world.moves[&approach.movement];
    let movement = &world.street.lanes[&approach.movement];
    let to_line = (approach.line_d - agent.d).max(0.0);
    match light_of(world, info) {
        Some(Light::Red(age)) => {
            if careful || approach.obeys_signal || age > LATE_RED_S {
                stop!(Block::Red);
            }
        }
        Some(Light::Amber(_)) => {
            let can_stop = to_line >= agent.speed * agent.speed / (2.0 * COMFORT_BRAKE);
            if can_stop && (careful || approach.obeys_signal) {
                stop!(Block::Amber);
            }
        }
        Some(Light::Green(_)) | None => {}
    }
    if info.sign == Sign::Stop
        && !info.signals
        && !approach.stopped_at_sign
        && (careful || approach.full_stop)
    {
        stop!(Block::StopSign);
    }
    let top_speed = desired_speed(agent, approach.movement, world);
    let mut margin = critical_gap(info.turn);
    if !careful {
        margin *= 1.0 - agent.traits.risk * (0.6 + 0.4 * agent.frustration);
    }
    let defers = agent
        .last_held
        .is_some_and(|tick| world.time_s - tick as f64 * DT <= HELD_DEFER_S);
    for other in &snapshot.seen {
        let Some(other_movement) = other.movement else {
            continue;
        };
        if other.id == agent.id {
            continue;
        }
        let Some(my_zone) = movement.conflict_zones.get(&other_movement) else {
            continue;
        };
        let other_info = &world.moves[&other_movement];
        if !careful && approach.unseen.contains(&other_info.from_segment) {
            continue;
        }
        let their_zone = world.street.lanes[&other_movement].conflict_zones[&approach.movement];
        let their_out = f64::from(their_zone.y) + other.length - other.s_on_movement;
        if their_out <= 0.0 {
            continue;
        }
        let their_in = (f64::from(their_zone.x) - other.s_on_movement).max(0.0);
        let (arrives, leaves, block);
        if other.in_junction {
            block = Block::InJunction(other.id, other_info.from_segment);
            if other.speed < STOPPED_SPEED {
                (arrives, leaves) = (0.0, f64::INFINITY);
            } else {
                // It may still be picking up speed: the earliest it can arrive.
                let limit = world.street.lanes[&other_movement]
                    .speed_limit
                    .max(other.speed);
                (arrives, leaves) = (
                    travel_time(their_in, other.speed, limit),
                    their_out / other.speed,
                );
            }
        } else {
            block = Block::Yield(other.id, other_info.from_segment);
            if matches!(light_of(world, other_info), Some(Light::Red(_))) {
                continue;
            }
            let owes = must_yield(info, other_info);
            if !(owes || defers) || (!careful && !approach.yields) {
                continue;
            }
            // Deferring after a hold is owed to those who may have started meanwhile, not
            // to cars standing at their lines (they would wait for it in turn).
            if !owes && other.speed < STOPPED_SPEED {
                continue;
            }
            if other.speed < STOPPED_SPEED && -other.s_on_movement < AT_LINE {
                // Held by people on foot, it is not about to go: it holds nobody.
                if other.held_by_walkers {
                    continue;
                }
                // It has priority but it stands at its line. If it is about to go, I
                // wait; only in a ring of waiting cars that all give way to each other
                // does one of them (see `leads_ring`) go first.
                if !leads_ring(agent, snapshot, world) {
                    stop!(block);
                }
                continue;
            }
            let limit = world.street.lanes[&other_movement]
                .speed_limit
                .max(other.speed);
            arrives = travel_time(their_in, other.speed, limit);
            leaves = their_out / other.speed.max(1.0);
        }
        let my_in = travel_time(to_line + f64::from(my_zone.x), agent.speed, top_speed);
        let my_out = travel_time(
            to_line + f64::from(my_zone.y) + agent.length,
            agent.speed,
            top_speed,
        );
        if my_in < leaves + margin && arrives < my_out + margin {
            stop!(block);
        }
    }
    if careful || approach.keeps_box {
        let room = agent.length + MIN_GAP + 1.0;
        let full = snapshot.seen.iter().any(|other| {
            other.id != agent.id
                && other.lane == info.out_lane
                && other.s - other.length < room
                && other.speed < 1.5
        });
        if full {
            stop!(Block::ExitFull);
        }
    }
    blocks
}

/// What holds back a car that already stands inside the junction (it went in when the
/// way was clear, then was held up by what is ahead of it) before it pulls away: the cars
/// in the junction on paths that cross its own. Two such cars may wait for each other, so
/// of two standing ones the one who has waited longer goes (the same past tick for both).
fn box_blocks(
    agent: &Agent,
    approach: &Approach,
    snapshot: &Snapshot,
    world: &World,
    careful: bool,
) -> Vec<Block> {
    let info = &world.moves[&approach.movement];
    let movement = &world.street.lanes[&approach.movement];
    let front = agent.d - approach.line_d;
    let top_speed = desired_speed(agent, approach.movement, world);
    let mut margin = critical_gap(info.turn);
    if !careful {
        margin *= 1.0 - agent.traits.risk * (0.6 + 0.4 * agent.frustration);
    }
    let my_wait = snapshot
        .seen
        .iter()
        .find(|me| me.id == agent.id)
        .map_or(agent.wait_ticks, |me| me.wait_ticks);
    let mine = (my_wait, std::cmp::Reverse(agent.id));
    let mut blocks = Vec::new();
    for other in &snapshot.seen {
        let Some(other_movement) = other.movement else {
            continue;
        };
        if other.id == agent.id || !other.in_junction || other.crashed {
            continue;
        }
        let Some(my_zone) = movement.conflict_zones.get(&other_movement) else {
            continue;
        };
        let other_info = &world.moves[&other_movement];
        if !careful && approach.unseen.contains(&other_info.from_segment) {
            continue;
        }
        let their_zone = world.street.lanes[&other_movement].conflict_zones[&approach.movement];
        let their_out = f64::from(their_zone.y) + other.length - other.s_on_movement;
        if their_out <= 0.0 {
            continue;
        }
        let their_in = (f64::from(their_zone.x) - other.s_on_movement).max(0.0);
        let (arrives, leaves);
        if other.speed < STOPPED_SPEED {
            if mine > (other.wait_ticks, std::cmp::Reverse(other.id)) {
                continue;
            }
            (arrives, leaves) = (0.0, f64::INFINITY);
        } else {
            let limit = world.street.lanes[&other_movement]
                .speed_limit
                .max(other.speed);
            (arrives, leaves) = (
                travel_time(their_in, other.speed, limit),
                their_out / other.speed,
            );
        }
        let my_in = travel_time(
            (f64::from(my_zone.x) - front).max(0.0),
            agent.speed,
            top_speed,
        );
        let my_out = travel_time(
            (f64::from(my_zone.y) + agent.length - front).max(0.0),
            agent.speed,
            top_speed,
        );
        if my_in < leaves + margin && arrives < my_out + margin {
            blocks.push(Block::InJunction(other.id, other_info.from_segment));
        }
    }
    blocks
}

/// Cars standing at their lines can wait for each other in a ring (each gives way to the
/// next: the old game's priority-to-the-right deadlock). In the ring of `agent`, the one
/// who has waited longest goes, the lower id on a tie. Everyone reads the same past tick,
/// the waits too, so exactly one of a ring goes; a car that waits for a car that is free
/// to go is not in a ring and waits.
fn leads_ring(agent: &Agent, snapshot: &Snapshot, world: &World) -> bool {
    let standing: Vec<&Seen> = snapshot
        .seen
        .iter()
        .filter(|s| {
            !s.crashed
                && !s.in_junction
                && s.speed < STOPPED_SPEED
                && -s.s_on_movement < AT_LINE
                && s.movement.is_some()
        })
        .collect();
    let n = standing.len();
    let Some(me) = standing.iter().position(|s| s.id == agent.id) else {
        return false;
    };
    let mut reach = vec![vec![false; n]; n];
    for x in 0..n {
        for y in 0..n {
            if let (true, Some(mx), Some(my)) = (x != y, standing[x].movement, standing[y].movement)
            {
                reach[x][y] = world.street.lanes[&mx].conflict_zones.contains_key(&my)
                    && must_yield(&world.moves[&mx], &world.moves[&my]);
            }
        }
    }
    for k in 0..n {
        for i in 0..n {
            for j in 0..n {
                if reach[i][k] && reach[k][j] {
                    reach[i][j] = true;
                }
            }
        }
    }
    if !reach[me][me] {
        return false;
    }
    let mine = (standing[me].wait_ticks, std::cmp::Reverse(standing[me].id));
    (0..n)
        .filter(|&x| x != me && reach[x][me] && reach[me][x])
        .all(|x| mine > (standing[x].wait_ticks, std::cmp::Reverse(standing[x].id)))
}

/// What the careful check says a driver did wrong by entering anyway.
fn cause_of(block: Block, approach: &Approach) -> Option<CauseKind> {
    match block {
        Block::Red => Some(CauseKind::RanRed),
        Block::Amber => None,
        Block::StopSign => Some(CauseKind::RolledStop),
        Block::Yield(other, segment) => Some(if approach.unseen.contains(&segment) {
            CauseKind::DidNotLook { at: other }
        } else if !approach.yields {
            CauseKind::DidNotYield { to: other }
        } else {
            CauseKind::GapTooShort { to: other }
        }),
        Block::InJunction(other, segment) => Some(if approach.unseen.contains(&segment) {
            CauseKind::DidNotLook { at: other }
        } else {
            CauseKind::GapTooShort { to: other }
        }),
        Block::ExitFull => Some(CauseKind::BlockedBox),
    }
}

/// The nearest body in the way of the agent: (gap to it, its speed along the agent's
/// path, who). Vehicles in a lane of the agent's route are measured along it; the
/// others are found by sampling the path ahead against their bodies.
fn nearest_obstacle(
    agent: &Agent,
    snapshot: &Snapshot,
    world: &World,
    lookahead: f64,
) -> Option<(f64, f64, AgentId)> {
    let route = &world.routes[agent.route];
    let index = route.lane_index(agent.d);
    let mut near = Vec::new();
    snapshot
        .grid
        .query(agent.pos, (lookahead + 10.0) as f32, &mut near);
    let mut samples: Vec<Vec2> = Vec::new();
    let mut best: Option<(f64, f64, AgentId)> = None;
    for item in near {
        let other = &snapshot.seen[item as usize];
        if other.id == agent.id {
            continue;
        }
        let on_route = route.lanes[index..]
            .iter()
            .position(|lane| *lane == other.lane)
            .map(|ahead| index + ahead);
        let (gap, speed) = if let Some(lane_index) = on_route {
            let front = route.offsets[lane_index] + other.s;
            if front <= agent.d {
                continue;
            }
            (front - other.length - agent.d, other.speed)
        } else {
            if samples.is_empty() {
                let count = (lookahead / SAMPLE_STEP).ceil() as usize + 1;
                samples = (0..=count)
                    .map(|k| route.point(world.street, agent.d + k as f64 * SAMPLE_STEP))
                    .collect();
            }
            let reach = (other.width * 0.5 + agent.width * 0.5 + SIDE_MARGIN) as f32;
            let half = (other.length * 0.5 - other.width * 0.5).max(0.0) as f32;
            let mut hit: Option<usize> = None;
            for offset in [-half, 0.0, half] {
                let center = other.pos + other.dir * offset;
                // Only what is ahead of the front bumper is in the way.
                if (center - samples[0]).dot(agent.dir) <= 0.0 {
                    continue;
                }
                let first = samples
                    .iter()
                    .position(|point| point.distance_squared(center) < reach * reach);
                if let Some(k) = first {
                    hit = Some(hit.map_or(k, |h| h.min(k)));
                }
            }
            let Some(k) = hit else {
                continue;
            };
            let tangent = (samples[(k + 1).min(samples.len() - 1)] - samples[k.saturating_sub(1)])
                .normalize_or_zero();
            let along = f64::from(other.dir.dot(tangent)).max(0.0);
            (k as f64 * SAMPLE_STEP, other.speed * along)
        };
        if gap > lookahead {
            continue;
        }
        if best.is_none_or(|(g, _, _)| gap < g) {
            best = Some((gap, speed, other.id));
        }
    }
    best
}

/// The acceleration for this tick, and what was decided on the way.
pub fn decide(agent: &Agent, snapshot: &Snapshot, world: &World, tick: u64) -> Decision {
    let mut decision = Decision::default();
    if agent.crashed() {
        return decision;
    }
    let route = &world.routes[agent.route];
    let index = route.lane_index(agent.d);
    let v = agent.speed;
    let tau = headway(agent);

    // Passing the line: what would a careful driver have said?
    if let Some(approach) = &agent.approach
        && !approach.committed
        && agent.d >= approach.line_d
    {
        decision.committed = true;
        for block in junction_blocks(agent, approach, snapshot, world, true, false) {
            decision.causes.extend(cause_of(block, approach));
        }
    }

    // Entering a zebra: what a careful driver would have done there.
    if let Some(crossing) = &agent.crossing
        && !crossing.entered
        && agent.d >= crossing.enter_d
    {
        decision.crossing_entered = true;
        decision.causes.extend(yielding::crossing_causes(crossing));
    }

    if tick < agent.distracted_until {
        // Looking away: the foot stays where it is.
        return decision;
    }

    // The road itself: the limit here, and the slower lanes and bends ahead.
    let here = agent.d - route.offsets[index];
    let mut target = desired_speed_at(agent, route.lanes[index], here, world);
    // The deceleration the things ahead ask for (see `needed_brake`), measured to the
    // thing itself: the margins a driver keeps are comfort, not distance it may not use.
    let mut need = 0.0_f64;
    let mut view_need = 0.0_f64;
    let lookahead =
        (v * v / (2.0 * COMFORT_BRAKE) + v * tau + 12.0 + agent.length).min(MAX_LOOKAHEAD);
    // The street speed changes along a lane, so the cells ahead are looked at too: the
    // rest of this lane, then each next lane from its start.
    for next in index..route.lanes.len() {
        let lane = route.lanes[next];
        let length = world.street.lanes[&lane].length;
        let start = route.offsets[next] - agent.d;
        let mut cell = if next == index {
            (here / CELL) as usize + 1
        } else {
            0
        };
        loop {
            let along = cell as f64 * CELL;
            let distance = start + along;
            if distance > lookahead {
                break;
            }
            if along < length || cell == 0 {
                let speed = desired_speed_at(agent, lane, along, world);
                target = target.min((speed * speed + 2.0 * COMFORT_BRAKE * distance).sqrt());
                need = need.max(needed_brake(v, speed, distance));
            }
            if world.moves.contains_key(&lane) || along >= length {
                break;
            }
            cell += 1;
        }
        if start > lookahead {
            break;
        }
    }

    // The junction ahead: go, or stop at the line.
    if let Some(approach) = &agent.approach
        && !approach.committed
        && agent.d < approach.line_d
    {
        let to_line = approach.line_d - agent.d;
        if to_line < v * v / (2.0 * COMFORT_BRAKE) + v * tau + DECIDE_MARGIN {
            // Too close to stop without braking hard: the decision to go stands.
            let no_return = approach.go && to_line < v * v / (2.0 * HARD_BRAKE);
            let exit_busy = world.peds.is_some_and(|peds| {
                (yielding::is_careful(agent) || approach.keeps_box)
                    && yielding::exit_zebra_busy(agent, approach.end_d, snapshot, peds)
            });
            let go = no_return
                || (!exit_busy && junction_check(agent, approach, snapshot, world, false).is_ok());
            decision.held_by_walkers = exit_busy && !go;
            decision.go = Some(go);
            if !go {
                let (stop_d, margin) = yielding::stop_point(agent, approach.line_d, world.peds);
                target = target.min(safe_speed(stop_d - agent.d - margin, 0.0, DT));
                need = need.max(needed_brake(v, 0.0, stop_d - agent.d));
            }
            decision.stopped_at_sign = v < STOPPED_SPEED && to_line < AT_LINE;
        }
    }

    // Standing inside the junction (held up by what is ahead) and about to pull away:
    // look at the crossing traffic again, as a driver at the line would.
    if let Some(approach) = &agent.approach
        && approach.committed
        && v < STOPPED_SPEED
        && agent.d - agent.length < approach.end_d
    {
        if box_blocks(agent, approach, snapshot, world, false).is_empty() {
            for block in box_blocks(agent, approach, snapshot, world, true) {
                decision.causes.extend(cause_of(block, approach));
            }
        } else {
            target = 0.0;
        }
    }

    // The bodies in the way. A driver who is stopping at the line has no business with
    // what is beyond it: the stop already keeps them clear, and braking for a body
    // crossing ahead as if it were a leader would be harder than the stop itself.
    let stopping_at = match (decision.go, &agent.approach) {
        (Some(false), Some(approach)) => Some(approach.line_d - agent.d),
        _ => None,
    };
    let mut blocker = None;
    let mut leader = None;
    if let Some((gap, speed, other)) = nearest_obstacle(agent, snapshot, world, lookahead)
        && stopping_at.is_none_or(|to_line| gap <= to_line)
    {
        leader = Some((gap, speed));
        let min_gap = MIN_GAP * (1.0 - 0.5 * agent.traits.risk);
        let safe = safe_speed(gap - min_gap, speed, tau);
        need = need.max(needed_brake(v, speed, gap));
        if safe < target {
            target = safe;
            blocker = Some(other);
        }
    }

    // People on foot: in the path, at a zebra, behind a hidden kerb.
    if let Some(peds) = world.peds {
        let view = yielding::walker_view(agent, snapshot, peds, false, tau);
        // Standing at its line for people on the zebra before it (they hold it, even when
        // the line holds it too): the drivers who give way to it do not wait for it
        // (stage C review, finding 4).
        if view.blocker.is_some()
            && view.target <= target
            && v < STOPPED_SPEED
            && agent
                .approach
                .as_ref()
                .is_some_and(|approach| !approach.committed && approach.line_d - agent.d < AT_LINE)
        {
            decision.held_by_walkers = true;
        }
        view_need = view.need;
        if view.target < target {
            target = view.target;
            blocker = view.blocker;
        }
        let courteous = yielding::is_careful(agent) || agent.traits.courtesy >= 0.5;
        if courteous
            && let Some(limit) = yielding::keep_crossing_clear(agent, leader, peds)
            && limit < target
        {
            target = limit;
            blocker = None;
        }
        if let Some(crossing) = &agent.crossing
            && agent.d < crossing.enter_d
            && !yielding::is_careful(agent)
        {
            let careful = yielding::walker_view(agent, snapshot, peds, true, tau);
            let not_yielding = if view.yield_to.is_none() {
                careful.yield_all
            } else {
                Vec::new()
            };
            let too_fast = careful
                .hidden
                .filter(|(limit, _)| v > limit + 1.0)
                .map(|(limit, _)| (v, limit));
            decision.crossing_pending = Some((not_yielding, too_fast));
        }
    }

    // Reaching the target: gently on the way up. Above it, the driver brakes with what the
    // stop (or the slower speed) asks for over the distance left, never softer than the
    // comfortable rate that keeps it on the safe-speed curve, and never harder than the
    // vehicle can. (The jump to the target in one tick would be 8 m/s² for any walker
    // stepping out.)
    let limit = agent.max_brake * ROAD_GRIP;
    if target >= v {
        decision.accel = ((target - v) / DT).min(ACCELERATION);
    } else {
        need = need.max(view_need);
        let brake = need.max(COMFORT_BRAKE.min((v - target) / DT)).min(limit);
        decision.accel = -brake;
        if let Some(other) = blocker
            && brake > HARD_BRAKE
            && v > 1.0
        {
            decision.hard_brake = Some((other, brake));
        }
    }
    decision
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(segment: i32, heading: Vec2, turn: Turn, rank: u8) -> MoveInfo {
        MoveInfo {
            node: 0,
            from_segment: segment,
            heading,
            turn,
            out_lane: StreetLaneId(0),
            turn_speed: f64::INFINITY,
            rank,
            sign: Sign::None,
            signals: false,
        }
    }

    #[test]
    fn priority_goes_to_the_right_then_to_the_one_who_turns_less() {
        // y down: heading north is -Y; the car from the east heads west.
        let north = info(1, -Vec2::Y, Turn::Straight, 2);
        let from_east = info(2, -Vec2::X, Turn::Straight, 2);
        assert!(must_yield(&north, &from_east));
        assert!(!must_yield(&from_east, &north));
        let oncoming_left = info(3, Vec2::Y, Turn::Left, 2);
        assert!(must_yield(&oncoming_left, &north));
        assert!(!must_yield(&north, &oncoming_left));
        let with_yield_sign = info(2, -Vec2::X, Turn::Straight, 1);
        assert!(
            !must_yield(&north, &with_yield_sign),
            "a sign beats the right"
        );
        assert!(must_yield(&with_yield_sign, &north));
    }

    #[test]
    fn the_safe_speed_stops_behind_a_standing_leader() {
        // Driving at the safe speed every tick never uses up the gap.
        let (mut gap, mut speed) = (40.0, 10.0);
        for _ in 0..600 {
            let safe = safe_speed(gap, 0.0, 1.0);
            speed = safe.min(speed + ACCELERATION * DT).max(speed - 8.0 * DT);
            gap -= speed * DT;
            assert!(gap > 0.0);
        }
        assert!(
            speed < 0.05 && gap < 1.0,
            "stopped close: {speed} m/s, {gap} m"
        );
    }

    #[test]
    fn a_small_speed_step_in_the_last_metre_is_not_a_hard_brake() {
        // 0.2 m/s over a lane that starts in 0.8 m: about 2 m/s², not 10.
        assert!(needed_brake(6.8, 6.6, 0.8) < 3.0);
        // A stop from speed is as hard as before: the pedal rise is lost in full.
        let stop = needed_brake(14.0, 0.0, 14.0);
        assert!(stop > 7.0, "{stop}");
    }

    #[test]
    fn travel_time_ramps_then_cruises() {
        assert_eq!(travel_time(0.0, 3.0, 8.0), 0.0);
        assert!((travel_time(20.0, 10.0, 8.0) - 2.0).abs() < 1e-9);
        // From rest: 1 m takes sqrt(2·1/2) = 1 s.
        assert!((travel_time(1.0, 0.0, 8.0) - 1.0).abs() < 1e-9);
        // 16 m of ramp (4 s), then 8 m at 8 m/s.
        assert!((travel_time(24.0, 0.0, 8.0) - 5.0).abs() < 1e-9);
    }
}
