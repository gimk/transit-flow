//! The kerb (pivot plan §5.1 to §5.3, Phase 3): a walker whose way steps from a foot
//! area onto a road area stops at the kerb and decides when to go.
//!
//! - **Lights.** A crossing near a junction under signals has a pedestrian light. The
//!   conflicting approaches are the crossed street's own approach and the approaches
//!   whose straight movement leaves into it; turning traffic crosses on a permissive
//!   green and yields to walkers (French practice). A walker steps off by the rules only
//!   if its own crossing time, plus `SIGNAL_MARGIN_S`, fits before a conflicting
//!   approach gets green (`SignalPlan::until_conflict`); otherwise it is on red. On red
//!   it goes only if its kerb draw is below `(1 - compliance) * (1 + 2 * frustration)`,
//!   the rule bending of the drivers; a fully compliant walker never does.
//! - **Gap.** Then, lights or not, it looks at the vehicles it can see (`sight`) as they
//!   were one reaction time ago (the same snapshots drivers decide from) and goes when
//!   none would reach its crossing line before it is over: the crossing time at its
//!   pace, shrunk by its risk tolerance and by frustration for those who bend rules
//!   (`GAP_SHRINK`), plus a margin (`ZEBRA_MARGIN_S` on a zebra, `ROAD_MARGIN_S`
//!   elsewhere). A standing vehicle counts by the time it would take to get there from
//!   rest.
//! - **Trust at a zebra.** Drivers yield on a zebra (`yielding`). A walker stepping on
//!   one by the rules (not on red) does not wait for a vehicle that can still stop
//!   comfortably before its line: farther than its braking distance at
//!   `COMFORT_BRAKE` after `TRUST_REACTION_S`, plus `TRUST_MARGIN`. A vehicle that cannot
//!   stop holds it back like elsewhere, so a careful walker never steps in front of one.
//!   A standing vehicle can always stop, but its driver sees the walker only after its
//!   reaction time: one standing close to the line (`START_REACH`) is trusted only once
//!   the walker has shown it will cross for `INTENT_SHOWN_S`.
//! - A walker who steps off in a gap a careful walker (no shrink, no trust it is not owed)
//!   would not take records `AcceptedShortGap` for the cause chains.
//!
//! A vehicle's time to the crossing comes from its route: the route's line through each
//! road area is sampled once per route (`road_areas_of`), and the part of that line
//! within `CROSSING_REACH` of the walker's own crossing line (from where it stands to
//! the far kerb) is where the vehicle would meet it.

use indexmap::IndexMap;

use super::drive::{BRAKE_RISE_S, COMFORT_BRAKE, DT, travel_time};
use super::route::RoutePath;
use super::walk::{AreaKind, WalkSpace};
use super::walker::Walker;
use super::{MoveInfo, StreetRun};
use crate::geom::segment::closest_point_to_segment;
use crate::roads::lane::Turn;
use crate::street::ids::StreetLaneId;
use crate::street::model::Street;

/// A walker this close to a kerb (beyond its body) is at it, metres (start value).
pub const KERB_REACH: f32 = 0.4;
/// The most a gap shrinks for a walker who bends rules, as a share (start value).
pub const GAP_SHRINK: f64 = 0.5;
/// Seconds of spare time a walker wants in its gap on a zebra and elsewhere (start
/// values).
pub const ZEBRA_MARGIN_S: f64 = 0.5;
pub const ROAD_MARGIN_S: f64 = 1.0;
/// A vehicle slower than this is standing: it counts by its time to start and get there.
const STANDING_SPEED: f64 = 0.3;
/// At a zebra, a walker trusts a driver who can stop after this reaction time (more
/// than the slowest careful driver's, 1.2 s), braking at `COMFORT_BRAKE`, with this much
/// room left, seconds and metres (start values).
pub const TRUST_REACTION_S: f64 = 1.5;
pub const TRUST_MARGIN: f64 = 2.0;
/// A standing vehicle closer to the walker's line than this could pull away into it
/// before its driver has seen the walker (`ACCELERATION` during `TRUST_REACTION_S`, plus
/// `TRUST_MARGIN`), metres.
pub const START_REACH: f64 =
    0.5 * super::drive::ACCELERATION * TRUST_REACTION_S * TRUST_REACTION_S + TRUST_MARGIN;
/// How long a walker shows it will cross (waiting at the kerb) before it trusts a car
/// standing close: longer than a driver's reaction time, seconds (start value).
pub const INTENT_SHOWN_S: f64 = 1.5;
/// Routes are sampled this often to find the road areas they cross, metres.
const ROUTE_STEP: f64 = 0.5;
/// A vehicle meets a walker where its route line comes this close to the walker's
/// crossing line: half a wide vehicle, a body and a little room, metres (start value).
pub const CROSSING_REACH: f32 = 2.0;
/// At signals, seconds a walker wants on top of its crossing time before the
/// conflicting traffic gets green (start value).
pub const SIGNAL_MARGIN_S: f64 = 2.0;

/// The part of a vehicle route inside one road area of the walk space.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteArea {
    pub area: usize,
    /// Route distances where the route enters and leaves the area.
    pub enter: f64,
    pub exit: f64,
    /// The route's line through the area: (route distance, point), every `ROUTE_STEP`.
    pub line: Vec<(f64, glam::Vec2)>,
}

/// What a walker at a kerb decides this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KerbChoice {
    Wait,
    /// Steps off the kerb. `gap_s`: the soonest a vehicle would reach the crossing, as
    /// the walker saw it (`None`: none coming).
    Go {
        gap_s: Option<f64>,
        on_red: bool,
        /// A careful walker would not have gone.
        short_gap: bool,
    },
}

/// The road areas a vehicle route drives through, in route order.
pub(super) fn road_areas_of(
    street: &Street,
    space: &WalkSpace,
    route: &RoutePath,
) -> Vec<RouteArea> {
    let mut found: Vec<RouteArea> = Vec::new();
    let roads: Vec<usize> = (0..space.areas.len())
        .filter(|a| space.areas[*a].kind.is_road())
        .collect();
    let steps = (route.length / ROUTE_STEP).ceil() as usize;
    for k in 0..=steps {
        let d = (k as f64 * ROUTE_STEP).min(route.length);
        let point = route.point(street, d);
        for &area in &roads {
            if !space.areas[area].polygon.contains(point) {
                continue;
            }
            match found.iter_mut().find(|r| r.area == area) {
                Some(entry) => {
                    entry.exit = d;
                    entry.line.push((d, point));
                }
                None => found.push(RouteArea {
                    area,
                    enter: d,
                    exit: d,
                    line: vec![(d, point)],
                }),
            }
        }
    }
    // The samples are `ROUTE_STEP` apart: the true edges of the area are found between the
    // last sample outside and the first inside (and the other way at the exit), so a
    // vehicle's stop line is the painted edge, not the sample after it.
    for entry in &mut found {
        let polygon = &space.areas[entry.area].polygon;
        let inside = |d: f64| polygon.contains(route.point(street, d));
        let edge = |outside: f64, within: f64| {
            let (mut out, mut inn) = (outside, within);
            for _ in 0..20 {
                let mid = 0.5 * (out + inn);
                if inside(mid) {
                    inn = mid;
                } else {
                    out = mid;
                }
            }
            inn
        };
        let (enter, exit) = (entry.enter, entry.exit);
        if enter > 0.0 {
            entry.enter = edge((enter - ROUTE_STEP).max(0.0), enter);
        }
        if exit < route.length {
            entry.exit = edge((exit + ROUTE_STEP).min(route.length), exit);
        }
    }
    found
}

/// Per crossing area under signals: the junction, and the approaches whose green lets
/// traffic drive over it.
pub(super) fn walk_signal_table(
    space: &WalkSpace,
    street: &Street,
    moves: &IndexMap<StreetLaneId, MoveInfo>,
) -> IndexMap<usize, (i32, Vec<i32>)> {
    let mut table = IndexMap::new();
    for (index, area) in space.areas.iter().enumerate() {
        let AreaKind::Crossing {
            segment,
            signal: Some(node),
            ..
        } = area.kind
        else {
            continue;
        };
        let mut conflicting = vec![segment];
        for info in moves.values() {
            let out = street.lanes[&info.out_lane].segment_id;
            if info.node == node
                && info.turn == Turn::Straight
                && out == Some(segment)
                && !conflicting.contains(&info.from_segment)
            {
                conflicting.push(info.from_segment);
            }
        }
        table.insert(index, (node, conflicting));
    }
    table
}

impl StreetRun {
    /// The pedestrian light of a crossing area now: `Some(true)` walk, `Some(false)` do
    /// not walk, `None` no light there.
    pub fn walk_light(&self, area: usize) -> Option<bool> {
        let (node, conflicting) = self.walk_signals.get(&area)?;
        let plan = self.plans.get(node)?;
        Some(plan.walk(conflicting, self.tick as f64 * DT))
    }

    /// Seconds until the traffic over a crossing area under signals gets green.
    pub fn until_conflict(&self, area: usize) -> Option<f64> {
        let (node, conflicting) = self.walk_signals.get(&area)?;
        let plan = self.plans.get(node)?;
        Some(plan.until_conflict(conflicting, self.tick as f64 * DT))
    }

    /// The vehicle light of `approach` at the junction `node` now (`None`: no signals
    /// there). For tests and views.
    pub fn approach_light(
        &self,
        node: i32,
        approach: i32,
    ) -> Option<crate::street::signals::Light> {
        self.plans
            .get(&node)
            .map(|plan| plan.light(approach, self.tick as f64 * DT))
    }

    /// The approaches whose green conflicts with a crossing area's walk light.
    pub fn walk_conflicts(&self, area: usize) -> Option<(i32, &[i32])> {
        self.walk_signals
            .get(&area)
            .map(|(node, conflicting)| (*node, conflicting.as_slice()))
    }

    /// The road areas a walker in corridor area `index` would cross from the kerb in
    /// front of it, and the portal index of the far kerb.
    fn roads_ahead(&self, walker: &Walker, index: usize) -> (Vec<usize>, usize) {
        let path = &self.walk_paths[walker.path];
        let roads: Vec<usize> = path.areas[index + 1..]
            .iter()
            .copied()
            .take_while(|a| self.walk_space.areas[*a].kind.is_road())
            .collect();
        let exit = index + roads.len();
        (roads, exit)
    }

    /// The street a walker at a kerb is about to cross, and the crossing it uses (`None`
    /// when its way crosses out of a crossing).
    pub(super) fn crossing_ahead(&self, walker: &Walker) -> (i32, Option<i32>) {
        let (roads, _) = self.roads_ahead(walker, walker.area_index);
        let mut segment = 0;
        let mut crossing = None;
        let mut off = false;
        for area in roads {
            match self.walk_space.areas[area].kind {
                AreaKind::Crossing { id, segment: s, .. } => {
                    segment = s;
                    crossing = Some(id);
                }
                AreaKind::Carriageway { segment: s } => {
                    segment = s;
                    off = true;
                }
                _ => {}
            }
        }
        (segment, if off { None } else { crossing })
    }

    /// `None` when the walker is not waiting at a kerb; otherwise whether it goes.
    pub(super) fn kerb_choice(&self, walker: &Walker) -> Option<KerbChoice> {
        let path = &self.walk_paths[walker.path];
        let k = walker.area_index;
        if walker.hit()
            || k + 1 >= path.areas.len()
            || !path.kerbs[k]
            || walker.cleared_kerb == Some(k)
        {
            return None;
        }
        let (l, r) = path.portals[k];
        let to_kerb = closest_point_to_segment(walker.pos, l, r).distance(walker.pos);
        // Once waiting, it stays at the kerb until it goes, even when the crowd there
        // pushes it back a little.
        if to_kerb > walker.radius + KERB_REACH && walker.kerb_since.is_none() {
            return None;
        }
        let (roads, exit) = self.roads_ahead(walker, k);
        let far = match path.portals.get(exit) {
            Some(&(l, r)) => closest_point_to_segment(walker.pos, l, r),
            None => closest_point_to_segment(walker.pos, path.goal.0, path.goal.1),
        };
        let crossing_s = f64::from(far.distance(walker.pos)) / walker.desired_speed.max(0.1);
        let t = &walker.traits;
        let time_left = roads.iter().find_map(|a| self.until_conflict(*a));
        let on_red = time_left.is_some_and(|left| left < crossing_s + SIGNAL_MARGIN_S);
        if on_red {
            let kerb = path.kerbs[..k].iter().filter(|kerb| **kerb).count();
            let draw = f64::from(walker.kerb_draws[kerb]);
            if draw >= (1.0 - t.compliance) * (1.0 + 2.0 * walker.frustration) {
                return Some(KerbChoice::Wait);
            }
        }
        let bend = (t.risk + walker.frustration * (1.0 - t.compliance)).min(1.0);
        let on_zebra = roads
            .iter()
            .any(|a| matches!(self.walk_space.areas[*a].kind, AreaKind::Crossing { .. }));
        let margin = if on_zebra {
            ZEBRA_MARGIN_S
        } else {
            ROAD_MARGIN_S
        };
        let needed = crossing_s * (1.0 - GAP_SHRINK * bend) + margin;
        let careful_needed = crossing_s + margin;
        let trust = on_zebra && !on_red;
        let vehicles = self.vehicles_ahead(walker, &roads, far);
        let holds = |need: f64| {
            vehicles
                .iter()
                .any(|&(time, can_stop)| time <= need && !(trust && can_stop))
        };
        if holds(needed) {
            return Some(KerbChoice::Wait);
        }
        let gap_s = vehicles.iter().map(|(time, _)| *time).reduce(f64::min);
        Some(KerbChoice::Go {
            gap_s,
            on_red,
            short_gap: holds(careful_needed),
        })
    }

    /// The vehicles the walker sees coming to its crossing line (from where it stands to
    /// `far`) on one of `roads`, seen one reaction time ago and brought forward to now:
    /// (seconds until it gets there, whether it can still stop before it).
    fn vehicles_ahead(
        &self,
        walker: &Walker,
        roads: &[usize],
        far: glam::Vec2,
    ) -> Vec<(f64, bool)> {
        let back = ((walker.traits.reaction_s / DT).round() as usize)
            .max(1)
            .min(self.history.len());
        let snapshot = &self.history[self.history.len() - back];
        let age = back as f64 * DT;
        let mut found = Vec::new();
        for seen in &snapshot.seen {
            let Some(agent) = self.agents.get(&seen.id) else {
                continue;
            };
            let nose = seen.pos + seen.dir * (seen.length as f32 * 0.5);
            if self
                .sight
                .hider(walker.pos, nose, &snapshot.seen, Some(seen.id))
                .is_some()
            {
                continue;
            }
            let route = &self.routes[agent.route];
            let Some(j) = route.lanes.iter().position(|lane| *lane == seen.lane) else {
                continue;
            };
            let front = route.offsets[j] + seen.s;
            for route_area in &self.route_areas[agent.route] {
                if !roads.contains(&route_area.area) {
                    continue;
                }
                // Where the route meets the walker's line.
                let mut meet: Option<(f64, f64)> = None;
                for &(d, point) in &route_area.line {
                    if closest_point_to_segment(point, walker.pos, far).distance(point)
                        < CROSSING_REACH
                    {
                        meet = Some(meet.map_or((d, d), |(a, b)| (a.min(d), b.max(d))));
                    }
                }
                let Some((enter, leave)) = meet else {
                    continue;
                };
                if front - seen.length >= leave {
                    continue;
                }
                let distance = enter - front;
                // Whether it can stop is measured to the line its driver stops at: on a
                // zebra the painted edge less the margin (`yielding::stop_line`).
                let to_line = if matches!(
                    self.walk_space.areas[route_area.area].kind,
                    AreaKind::Crossing { .. }
                ) {
                    super::yielding::stop_line(route_area) - front
                } else {
                    distance
                };
                let v = seen.speed;
                let (time, can_stop) = if distance <= 0.0 {
                    (0.0, false)
                } else if v > STANDING_SPEED {
                    // The driver reacts after its reaction time, then the brake builds up.
                    let stop =
                        v * v / (2.0 * COMFORT_BRAKE) + v * (TRUST_REACTION_S + BRAKE_RISE_S * 0.5);
                    (
                        (distance / v - age).max(0.0),
                        to_line - v * age > stop + TRUST_MARGIN,
                    )
                } else {
                    let limit = self.street.lanes[&seen.lane].speed_limit;
                    let shown = walker
                        .kerb_since
                        .is_some_and(|since| (self.tick - since) as f64 * DT >= INTENT_SHOWN_S);
                    (
                        travel_time(distance, 0.0, limit),
                        to_line > START_REACH || shown,
                    )
                };
                found.push((time, can_stop));
            }
        }
        found
    }
}
