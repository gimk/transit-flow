//! Drivers and people on foot (pivot plan §5.1, §5.5, Phase 3 stage C). A driver reads
//! the walkers from the snapshot as old as its reaction time, like everything else, and
//! sees one only if the sight line from its front to the walker is not cut (`sight`).
//!
//! What a careful driver does:
//! - **Brakes for any walker it sees in its path**, on a crossing or not: a walker on a
//!   road area of its route whose body is, or will be when the car gets there, within
//!   the car's half width of its line.
//! - **Yields at a zebra** (Code de la route R415-11): it stops before the crossing while
//!   someone it sees is on it, or waits at its kerb to step on it. At signals a walker at
//!   the kerb counts only while it has walk (a driver turning on a permissive green).
//! - **Approaches a hidden zebra slowly**: when a kerb of a zebra ahead is hidden (by a
//!   parked van, a bus, a tree), it drives no faster than the speed from which it can
//!   stop before the zebra after its reaction time, braking at `COMFORT_BRAKE`:
//!   `v = sqrt(b²τ² + 2bD) - bτ` with D the distance to the zebra less `PED_STOP_MARGIN`
//!   (the same following formula as for a standing car), but never below
//!   `CREEP_SPEED`, so it moves up until it sees a kerb hidden right at the zebra's edge.
//!   Someone stepping out from behind the van then always finds a car that can stop.
//! - **Keeps crossings clear**: it does not stop on a zebra behind a queue that leaves
//!   no room past it, and when it must stop at a junction whose line is just past a
//!   zebra, it stops before the zebra (where the stop line is in the street).
//!
//! The personality bends this per zebra, with two draws made when the zebra comes up
//! (`CrossingApproach`): `yields` (compliance and frustration, as at the junction) and
//! `looks` (attention): a driver who does not look takes in neither the people at the
//! kerb nor the hidden kerb, only what is in front of it. Distraction (looking away)
//! already blinds it to everything. When a driver's front enters a zebra where a
//! careful driver would have stopped, the careful check gives the causes.

use glam::Vec2;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::agent::Agent;
use super::drive::{COMFORT_BRAKE, DT, MAX_BRAKE, SIDE_MARGIN, Snapshot, safe_speed};
use super::events::CauseKind;
use super::kerb::RouteArea;
use super::sight::{Hider, SightBlockers};
use super::walk::{AreaKind, WalkSpace};
use crate::street::ids::AgentId;

/// A driver comes up to a zebra whose kerb stays hidden at this speed until it sees the
/// kerb, m/s (start value): it stops within about 2 m (its headway of at most 1.8 s at
/// 1 m/s, and 0.17 m of braking at `COMFORT_BRAKE`), and the in-path rule brakes for
/// whoever steps out.
pub const CREEP_SPEED: f64 = 1.0;
/// A driver stops this far before a zebra it yields at, metres (start value).
pub const PED_STOP_MARGIN: f64 = 1.0;
/// When it must stop at a junction line just past a zebra, it stops this far before the
/// zebra: little, so its front stays within `AT_LINE` of the line for a 3 m zebra.
pub const ZEBRA_LINE_MARGIN: f64 = 0.3;
/// A zebra ending this close before a junction's stop line belongs to that line.
const ZEBRA_AT_LINE: f64 = 1.0;
/// Room kept to a walker in the path, beyond its body, metres (start value).
pub const WALKER_GAP: f64 = 1.5;
/// A walker on a zebra this far beside the car's path, walking away from it, no longer
/// holds the driver, metres (start value).
pub const PASSED_MARGIN: f64 = 1.0;

/// What a driver decided about the zebra coming up on its route. The draws are made
/// once, when the zebra comes up, so a personality acts the same way through it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrossingApproach {
    /// The walk-space area of the zebra.
    pub area: usize,
    /// Route distances where the route enters and leaves it.
    pub enter_d: f64,
    pub exit_d: f64,
    pub yields: bool,
    pub looks: bool,
    /// The front has entered it (the careful check has been made).
    pub entered: bool,
    /// Coming up to it, the last walker a careful driver would have yielded to and this
    /// one did not (kept until the front enters the zebra).
    pub pending_yield: Option<AgentId>,
    /// Coming up to it with a hidden kerb, the worst (speed, careful limit), m/s.
    pub worst_fast: Option<(f64, f64)>,
}

/// The walk world as drivers see it.
pub struct PedWorld<'a> {
    pub space: &'a WalkSpace,
    pub route_areas: &'a [Vec<RouteArea>],
    pub sight: &'a SightBlockers,
    /// Per zebra area: points of its kerbs (both ends and the middle of each).
    pub kerb_points: &'a IndexMap<usize, Vec<Vec2>>,
    /// Per zebra area under signals: is its walk light on now?
    pub walk_lights: &'a IndexMap<usize, bool>,
}

/// What the walkers ahead ask of a driver.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WalkerView {
    /// The speed the walkers allow now, and who sets it.
    pub target: f64,
    pub blocker: Option<AgentId>,
    /// The walker a careful driver yields to at the next zebra.
    pub yield_to: Option<AgentId>,
    /// The limit of a hidden zebra, and what hides its kerb.
    pub hidden: Option<(f64, Hider)>,
}

/// The front of a car: where its driver looks from.
pub fn eye(agent: &Agent) -> Vec2 {
    agent.pos + agent.dir * (agent.length as f32 * 0.5)
}

/// The route distance of the route line point nearest to `point`, and how far beside
/// the line it is.
pub fn project(line: &[(f64, Vec2)], point: Vec2) -> Option<(f64, f32)> {
    line.iter()
        .map(|(d, p)| (*d, p.distance(point)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The speed walkers allow `agent` this tick. `careful` asks what a driver with full
/// compliance and attention would do from the same view.
pub fn walker_view(
    agent: &Agent,
    snapshot: &Snapshot,
    peds: &PedWorld,
    careful: bool,
    tau: f64,
) -> WalkerView {
    let mut view = WalkerView {
        target: f64::INFINITY,
        ..WalkerView::default()
    };
    let front = agent.d;
    let v = agent.speed;
    let me = eye(agent);
    let looks = careful
        || agent
            .crossing
            .as_ref()
            .is_none_or(|approach| approach.looks);
    let yields = careful
        || agent
            .crossing
            .as_ref()
            .is_none_or(|approach| approach.yields);
    let risk = if careful { 0.0 } else { agent.traits.risk };
    let half = (agent.width * 0.5) as f32;
    for area in &peds.route_areas[agent.route] {
        if area.exit <= front - agent.length || area.enter - front > super::drive::MAX_LOOKAHEAD {
            continue;
        }
        let zebra = matches!(peds.space.areas[area.area].kind, AreaKind::Crossing { .. });
        let ahead = front < area.enter;
        // The hidden zebra: can a walker step out unseen?
        if zebra && ahead && looks {
            let hidden = peds.kerb_points.get(&area.area).and_then(|points| {
                points
                    .iter()
                    .find_map(|p| peds.sight.hider(me, *p, &snapshot.seen, Some(agent.id)))
            });
            if let Some(hider) = hidden {
                let room = area.enter - front - PED_STOP_MARGIN;
                // Never slower than a creep: a kerb hidden right at the zebra's edge stays
                // hidden until the car has moved up (stage C review, finding 2).
                let limit = safe_speed(room, 0.0, tau).max(CREEP_SPEED) * (1.0 + risk);
                if limit < view.target {
                    view.target = limit;
                    view.blocker = None;
                }
                if view.hidden.is_none_or(|(l, _)| limit < l) {
                    view.hidden = Some((limit, hider));
                }
            }
        }
        let lights = peds.walk_lights.get(&area.area).copied();
        for walker in &snapshot.walkers {
            let on = walker.area == Some(area.area);
            let intends = walker.intent == Some(area.area);
            if !on && !intends {
                continue;
            }
            if peds
                .sight
                .hider(me, walker.pos, &snapshot.seen, Some(agent.id))
                .is_some()
            {
                continue;
            }
            let Some((d_w, beside)) = project(&area.line, walker.pos) else {
                continue;
            };
            // Is it, or will it be when the car gets there, in the car's path?
            let reach = half + walker.radius + SIDE_MARGIN as f32;
            let line_at = area
                .line
                .iter()
                .find(|(d, _)| *d == d_w)
                .map_or(walker.pos, |(_, p)| *p);
            let toward_line = (line_at - walker.pos).normalize_or_zero();
            let closing = walker.vel.dot(toward_line);
            let t_car = ((d_w - front) / v.max(1.0)).max(0.0);
            let predicted = beside - (closing * t_car as f32).max(0.0);
            let in_path = on && (beside < reach || predicted < reach);
            if in_path && d_w > front - 0.5 {
                let gap = d_w - front - f64::from(walker.radius) - WALKER_GAP;
                let limit = safe_speed(gap, 0.0, DT);
                if limit < view.target {
                    view.target = limit;
                    view.blocker = Some(walker.id);
                }
            }
            if !zebra || !ahead || !yields {
                continue;
            }
            // Yield at the zebra to someone on it (not yet past the car's path), or
            // about to step on it (and, at signals, allowed to).
            let passed = on && beside > reach + PASSED_MARGIN as f32 && closing <= 0.0;
            let waiting = intends && looks && lights.is_none_or(|walk| walk);
            if (on && !passed) || waiting {
                let room = area.enter - front - PED_STOP_MARGIN;
                // Only if it can still stop before the zebra (a car already standing
                // closer than the margin stays where it is): otherwise it is through
                // before them, and the in-path rule brakes for whoever is in the way.
                if area.enter - front > v * v / (2.0 * MAX_BRAKE) {
                    // With its own headway: it comes up slowly to people waiting.
                    let limit = safe_speed(room, 0.0, tau);
                    if limit < view.target {
                        view.target = limit;
                        view.blocker = Some(walker.id);
                    }
                    view.yield_to.get_or_insert(walker.id);
                }
            }
        }
    }
    view
}

/// A driver who bends no rule: the careful check would say the same.
pub fn is_careful(agent: &Agent) -> bool {
    agent.traits.compliance >= 1.0 && agent.traits.attention >= 1.0 && agent.traits.risk <= 0.0
}

/// The points of each zebra's kerbs (both ends and the middle): where a walker steps on.
pub fn kerb_points(space: &WalkSpace) -> IndexMap<usize, Vec<Vec2>> {
    let mut points: IndexMap<usize, Vec<Vec2>> = IndexMap::new();
    for kerb in &space.kerbs {
        for (zebra, other) in [(kerb.inside, kerb.outside), (kerb.outside, kerb.inside)] {
            if matches!(space.areas[zebra].kind, AreaKind::Crossing { .. })
                && !space.areas[other].kind.is_road()
            {
                points
                    .entry(zebra)
                    .or_default()
                    .extend([kerb.a, (kerb.a + kerb.b) * 0.5, kerb.b]);
            }
        }
    }
    points
}

/// Is the zebra just past the junction exit of this approach busy (someone on it, or
/// about to step on it and allowed to)? A driver who keeps the box clear does not enter
/// the junction then: it would stand in it, blocking the crossing traffic.
pub fn exit_zebra_busy(agent: &Agent, end_d: f64, snapshot: &Snapshot, peds: &PedWorld) -> bool {
    peds.route_areas[agent.route].iter().any(|area| {
        matches!(peds.space.areas[area.area].kind, AreaKind::Crossing { .. })
            && area.enter >= end_d - 1.0
            && area.enter <= end_d + EXIT_ZEBRA_REACH
            && snapshot.walkers.iter().any(|walker| {
                walker.area == Some(area.area)
                    || (walker.intent == Some(area.area)
                        && peds.walk_lights.get(&area.area).is_none_or(|walk| *walk))
            })
    })
}

/// A zebra this close past the end of a junction movement is at its exit, metres.
const EXIT_ZEBRA_REACH: f64 = 6.0;

/// Where a driver stops for its junction line: before a zebra that ends at the line.
pub fn stop_point(agent: &Agent, line_d: f64, peds: Option<&PedWorld>) -> (f64, f64) {
    let Some(peds) = peds else {
        return (line_d, super::drive::STOP_MARGIN);
    };
    for area in &peds.route_areas[agent.route] {
        let zebra = matches!(peds.space.areas[area.area].kind, AreaKind::Crossing { .. });
        if zebra && area.exit <= line_d + 0.01 && line_d - area.exit < ZEBRA_AT_LINE {
            return (area.enter, ZEBRA_LINE_MARGIN);
        }
    }
    (line_d, super::drive::STOP_MARGIN)
}

/// The causes of entering a zebra where a careful driver would have stopped or been
/// slower: what the careful check found while the driver came up to it.
pub fn crossing_causes(approach: &CrossingApproach) -> Vec<CauseKind> {
    let mut causes = Vec::new();
    if let Some(walker) = approach.pending_yield {
        causes.push(if approach.looks {
            CauseKind::DidNotYieldToWalker { to: walker }
        } else {
            CauseKind::DidNotLook { at: walker }
        });
    }
    if let Some((speed, limit)) = approach.worst_fast {
        causes.push(CauseKind::TooFastForSight {
            kmh: super::events::rounded(speed * 3.6, 10.0),
            safe_kmh: super::events::rounded(limit * 3.6, 10.0),
        });
    }
    causes
}

/// Courtesy at a zebra: with a slow queue ahead leaving no room past the zebra, stop
/// before it. `leader` is (gap to the leader's rear, its speed).
pub fn keep_crossing_clear(
    agent: &Agent,
    leader: Option<(f64, f64)>,
    peds: &PedWorld,
) -> Option<f64> {
    let (gap, speed) = leader?;
    if speed >= 1.5 {
        return None;
    }
    let rear_of_leader = agent.d + gap;
    for area in &peds.route_areas[agent.route] {
        let zebra = matches!(peds.space.areas[area.area].kind, AreaKind::Crossing { .. });
        if !zebra || agent.d >= area.enter || rear_of_leader < area.enter {
            continue;
        }
        if rear_of_leader - area.exit < agent.length + super::drive::MIN_GAP {
            let room = area.enter - agent.d - PED_STOP_MARGIN;
            if room > agent.speed * agent.speed / (2.0 * COMFORT_BRAKE) {
                return Some(safe_speed(room, 0.0, DT));
            }
        }
    }
    None
}
