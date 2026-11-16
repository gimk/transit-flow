//! The kerb (pivot plan §5.1 to §5.3, Phase 3): a walker whose way steps from a foot
//! area onto a road area stops at the kerb and decides when to go.
//!
//! - **Lights.** A crossing near a junction under signals has a pedestrian light
//!   (`SignalPlan::walk`): walk while no approach whose traffic drives over the crossing
//!   has green (the crossed street's own approach, and the approaches whose straight
//!   movement leaves into it; turning traffic crosses on a permissive green and will
//!   yield in stage C). On red, a walker goes only if its kerb draw is below
//!   `(1 - compliance) * (1 + 2 * frustration)`, the rule bending of the drivers; a fully
//!   compliant walker never does.
//! - **Gap.** Then, lights or not, it looks at the vehicles as they were one reaction
//!   time ago (the same snapshots drivers decide from) and goes when none would reach
//!   the road areas it crosses before it is over: the crossing time at its pace, shrunk
//!   by its risk tolerance and by frustration for those who bend rules
//!   (`GAP_SHRINK`), plus a margin. Drivers do not yield yet (stage C), so walkers also
//!   wait for a gap at a zebra, with the smaller `ZEBRA_MARGIN_S`: by French law they
//!   have priority there.
//!
//! A vehicle's time to the crossing comes from its route: the route distances where it
//! enters and leaves each road area are found once per route (`road_areas_of`).

use indexmap::IndexMap;

use super::drive::DT;
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
/// A vehicle slower than this is standing (waiting at a line): it does not close a gap.
const STANDING_SPEED: f64 = 0.3;
/// Routes are sampled this often to find the road areas they cross, metres.
const ROUTE_STEP: f64 = 0.5;

/// What a walker at a kerb decides this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KerbChoice {
    Wait,
    /// Steps off the kerb. `gap_s`: the soonest a vehicle would reach the crossing, as
    /// the walker saw it (`None`: none coming).
    Go {
        gap_s: Option<f64>,
        on_red: bool,
    },
}

/// The road areas a vehicle route drives through: (area, route distance where the
/// route enters it, where it leaves it).
pub(super) fn road_areas_of(
    street: &Street,
    space: &WalkSpace,
    route: &RoutePath,
) -> Vec<(usize, f64, f64)> {
    let mut found: Vec<(usize, f64, f64)> = Vec::new();
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
            match found.iter_mut().find(|(a, ..)| *a == area) {
                Some(entry) => entry.2 = d,
                None => found.push((area, d, d)),
            }
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
        if k + 1 >= path.areas.len() || !path.kerbs[k] || walker.cleared_kerb == Some(k) {
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
        let lights = roads.iter().find_map(|a| self.walk_light(*a));
        let on_red = lights == Some(false);
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
        let gap_s = self.vehicle_gap(walker, &roads);
        if gap_s.is_some_and(|gap| gap <= needed) {
            return Some(KerbChoice::Wait);
        }
        Some(KerbChoice::Go { gap_s, on_red })
    }

    /// The soonest a vehicle reaches one of `roads`, seen one reaction time ago and
    /// brought forward to now at the speed it had; `None` when nobody is coming.
    fn vehicle_gap(&self, walker: &Walker, roads: &[usize]) -> Option<f64> {
        let back = ((walker.traits.reaction_s / DT).round() as usize)
            .max(1)
            .min(self.history.len());
        let snapshot = &self.history[self.history.len() - back];
        let age = back as f64 * DT;
        let mut soonest: Option<f64> = None;
        for seen in &snapshot.seen {
            let Some(agent) = self.agents.get(&seen.id) else {
                continue;
            };
            let route = &self.routes[agent.route];
            let Some(j) = route.lanes.iter().position(|lane| *lane == seen.lane) else {
                continue;
            };
            let front = route.offsets[j] + seen.s;
            for &(area, enter, leave) in &self.route_areas[agent.route] {
                if !roads.contains(&area) || front - seen.length >= leave {
                    continue;
                }
                let time = if front >= enter {
                    0.0
                } else if seen.speed > STANDING_SPEED {
                    ((enter - front) / seen.speed - age).max(0.0)
                } else {
                    continue;
                };
                soonest = Some(soonest.map_or(time, |s: f64| s.min(time)));
            }
        }
        soonest
    }
}
