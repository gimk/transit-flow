//! The kerb (pivot plan §5.1 to §5.3, Phase 3): a walker whose way steps from a foot
//! area onto a road area stops at the kerb and decides when to go.
//!
//! - **Lights.** A crossing near a junction under signals has a pedestrian light. The
//!   conflicting approaches are the crossed street's own approach and the approaches
//!   whose straight movement leaves into it; turning traffic crosses on a permissive
//!   green and yields to walkers (French practice). A walker steps off by the rules only
//!   if its own crossing time, plus `SIGNAL_MARGIN_S`, fits before a conflicting
//!   approach gets green (`SignalPlan::until_conflict`); otherwise it is on red. On red
//!   it goes only if its kerb draw (made at spawn) is below `red_go_chance`, computed
//!   now from what it sees: its red habit, whether a vehicle is coming and how soon
//!   (`clear_factor`), the time it has waited (frustration) and the people near it that
//!   are already crossing (they are followed; a group waits more). A walker with a red
//!   habit of 1 never goes.
//! - **Gap.** Then, lights or not, it looks at the vehicles it can see (`sight`) as they
//!   were one reaction time ago (the same snapshots drivers decide from) and goes when
//!   none would reach its crossing line before it is over: its crossing time at its
//!   pace, never less, plus a margin (`ZEBRA_MARGIN_S` on a zebra, `ROAD_MARGIN_S`
//!   elsewhere) that its risk tolerance and frustration shrink by up to `MARGIN_SHRINK`.
//!   A standing vehicle counts by the time it would take to get there from rest. At a
//!   kerb where it does not look properly (a draw at spawn against `miss_chance`, which
//!   grows as attention falls) it judges from a view `STALE_VIEW_S` older and takes the
//!   cars to be where they were, so it can step in front of one that has come closer.
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

use std::borrow::Cow;

use glam::Vec2;
use indexmap::IndexMap;

use super::drive::{BRAKE_RISE_S, COMFORT_BRAKE, DT, travel_time};
use super::meet::BAND_HALF_WIDTH;
use super::route::RoutePath;
use super::walk::{AreaKind, CUT_M, WalkPath, WalkSpace};
use super::walker::{DECISION_RANGE_M, KerbDraw, RoadAct, Walker, jaywalk_chance};
use super::{MoveInfo, StreetRun};
use crate::geom::segment::closest_point_to_segment;
use crate::street::ids::{AgentId, StreetLaneId};
use crate::street::lanes::Turn;
use crate::street::model::Street;

/// A walker this close to a kerb (beyond its body) is at it, metres (start value).
pub const KERB_REACH: f32 = 0.4;
/// The share of its margin a walker with the most risk tolerance gives up (start value).
/// The gap itself is never below the crossing time: everybody looks.
pub const MARGIN_SHRINK: f64 = 0.6;
/// The chance not to look properly at a kerb is `MISS_SCALE * (1 - attention)` (start
/// value: a child of attention 0.5 misses a kerb one time in eight).
pub const MISS_SCALE: f64 = 0.25;
/// A walker who does not look properly judges from a view this much older than its
/// reaction time, and takes the cars to be where they were (start value, seconds).
pub const STALE_VIEW_S: f64 = 1.5;
/// A vehicle this many seconds later than the crossing time (and its margin) no longer
/// worries a walker deciding where to cross or whether to go on red (start value).
pub const TRAFFIC_HORIZON_S: f64 = 8.0;
/// With cars coming, the chance to cross away from a zebra falls to this share (start
/// value).
pub const EASE_FLOOR: f64 = 0.25;
/// Each person already crossing near (up to three) raises the chance to go on red by this
/// share; someone in a group goes `GROUP_RED_FACTOR` as often. After Rosenbloom 2009,
/// "Crossing at a red light: Behaviour of individuals and groups", Transportation
/// Research Part F 12(5): people follow others, and groups cross less on red. The values
/// are start values, to be calibrated in Phase 8.
pub const FOLLOW_GAIN: f64 = 0.5;
pub const GROUP_RED_FACTOR: f64 = 0.6;
/// Others this close that are already crossing count as people to follow, metres (start
/// value).
pub const SOCIAL_RANGE: f32 = 10.0;
/// A car standing on the piece beside a zebra this close to a walker keeps it from cutting
/// the corner there, metres (start value: a queue of two cars).
pub const CUT_BLOCK_RANGE_M: f32 = 14.0;

/// How free the street looks: 1 with no vehicle coming, 0 when the soonest one would
/// reach the walker's line before it can cross (and take its margin), growing over
/// `TRAFFIC_HORIZON_S`.
pub fn clear_factor(soonest: Option<f64>, crossing_s: f64) -> f64 {
    match soonest {
        None => 1.0,
        Some(t) => ((t - crossing_s - ROAD_MARGIN_S) / TRAFFIC_HORIZON_S).clamp(0.0, 1.0),
    }
}

/// How much a clear street raises the chance to cross away from a zebra: from
/// `EASE_FLOOR` (cars coming) to 1 (none).
pub fn traffic_ease(clear: f64) -> f64 {
    EASE_FLOOR + (1.0 - EASE_FLOOR) * clear
}

/// The chance that a walker steps off on a red light, from what it sees now. A walker
/// with a red habit of 1 never does.
pub fn red_go_chance(
    red_habit: f64,
    frustration: f64,
    clear: f64,
    followers: usize,
    in_group: bool,
) -> f64 {
    let social = 1.0 + FOLLOW_GAIN * followers.min(3) as f64;
    let group = if in_group { GROUP_RED_FACTOR } else { 1.0 };
    ((1.0 - red_habit) * (1.0 + 2.0 * frustration) * clear * social * group).min(1.0)
}

/// The chance that a walker does not look properly at a kerb.
pub fn miss_chance(attention: f64) -> f64 {
    MISS_SCALE * (1.0 - attention)
}

/// The chance that a walker cuts the corner at a zebra, more for the hurried (risk).
pub fn corner_chance(corner_habit: f64, risk: f64) -> f64 {
    (1.0 - corner_habit) * (0.4 + 0.6 * risk)
}
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
/// On the road a walker looks again this often, seconds (start value; staggered by id so
/// they do not all look on the same tick).
pub const ROAD_LOOK_S: f64 = 0.6;
const ROAD_LOOK_TICKS: u64 = (ROAD_LOOK_S / DT) as u64;
/// A vehicle that reaches the walker's path within this many seconds and does not slow
/// down makes it react (start value).
pub const ROAD_THREAT_S: f64 = 4.0;
/// On a zebra drivers are meant to yield and brake late and hard, so only a vehicle this
/// close in time that still does not slow down is a threat (start value).
pub const ZEBRA_THREAT_S: f64 = 2.0;
/// How far behind itself the walker extends its path, to know whether it stands in a
/// vehicle's lane, metres.
const ROAD_LOOK_BACK_M: f32 = 3.0;
/// A walker not yet in the lane stops if it is at least this far from it, else it steps
/// back, metres (start value).
pub const ROAD_STOP_CLEARANCE_M: f32 = 1.5;
/// A walker that stops on the road stands this far outside the band of every vehicle route
/// (half a car width beyond its body), metres (start value).
pub const BAND_STANDOFF_M: f32 = 0.5;
/// A fast walk, as a share of the desired speed (start value; a short run is about 2).
pub const HURRY_FACTOR: f64 = 1.5;
/// Spare time wanted when it hurries or steps back to clear a vehicle, seconds (start
/// value).
pub const HURRY_MARGIN_S: f64 = 0.3;
/// A vehicle was slowing if it lost `SLOWING_DROP` m/s over `SLOW_LOOK_S` (0.5 m/s2).
/// Slower than this, a vehicle is not a threat on the road, m/s (start value).
pub const ROAD_MOVING_SPEED: f64 = 2.5;
const SLOW_LOOK_S: f64 = 0.5;
const SLOWING_DROP: f64 = 0.25;
/// An assertive walker keeps its pace only if the car is still this much later than its
/// own reaction time, seconds (start value): nobody walks into a sure hit on purpose.
pub const ASSERT_MARGIN_S: f64 = 0.5;
/// The base chance that a walker on the road ignores a car that can stop (start value,
/// about 5 %, to calibrate in Phase 8), grown by risk tolerance and lack of courtesy.
pub const ASSERTIVE_BASE: f64 = 0.05;

/// The chance that a walker keeps its pace on the road when a car comes that can still
/// stop (French law gives priority to a walker already engaged, R415-11 at a crossing):
/// more with low courtesy and high risk tolerance.
pub fn assertive_chance(courtesy: f64, risk: f64) -> f64 {
    (ASSERTIVE_BASE * (1.0 + risk - courtesy)).clamp(0.0, 0.25)
}

/// Routes are sampled this often to find the road areas they cross, metres.
const ROUTE_STEP: f64 = 0.5;
/// A vehicle meets a walker where its route line comes this close to the walker's
/// crossing line: half a wide vehicle, a body and a little room, metres (start value).
pub const CROSSING_REACH: f32 = 2.0;
/// At signals, seconds a walker wants on top of its crossing time before the
/// conflicting traffic gets green (start value).
pub const SIGNAL_MARGIN_S: f64 = 2.0;

/// A vehicle on its way to meet a walker's line, as the walker saw it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Meeting {
    /// Seconds until it gets there, and whether it can still stop comfortably before.
    pub time: f64,
    pub can_stop: bool,
    /// Where its route comes within reach of the line and where it leaves it, metres
    /// along the line from its start.
    pub enter_m: f32,
    pub leave_m: f32,
    /// It was braking a moment ago.
    pub slowing: bool,
    /// It moves faster than `ROAD_MOVING_SPEED`: a standing or creeping vehicle waits for
    /// people and is no threat.
    pub moving: bool,
}

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
        self.roads_ahead_on(&self.walk_paths[walker.path], index)
    }

    fn roads_ahead_on(&self, path: &super::walk::WalkPath, index: usize) -> (Vec<usize>, usize) {
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
        let portals = self.walker_portals(walker);
        let (l, r) = portals[k];
        let to_kerb = closest_point_to_segment(walker.pos, l, r).distance(walker.pos);
        // Once waiting, it stays at the kerb until it goes, even when the crowd there
        // pushes it back a little.
        if to_kerb > walker.radius + KERB_REACH && walker.kerb_since.is_none() {
            return None;
        }
        let (roads, exit) = self.roads_ahead(walker, k);
        let far = match portals.get(exit) {
            Some(&(l, r)) => closest_point_to_segment(walker.pos, l, r),
            None => closest_point_to_segment(walker.pos, path.goal.0, path.goal.1),
        };
        let crossing_s = f64::from(far.distance(walker.pos)) / walker.desired_speed.max(0.1);
        let t = &walker.traits;
        let ordinal = path.kerbs[..k].iter().filter(|kerb| **kerb).count();
        let draw = walker.kerb_draws.get(ordinal).copied().unwrap_or(KerbDraw {
            red: 1.0,
            look: 1.0,
            corner: 1.0,
            assertive: 1.0,
        });
        // A walker who does not look properly here judges from an older view.
        let stale = f64::from(draw.look) < miss_chance(t.attention);
        let time_left = roads.iter().find_map(|a| self.until_conflict(*a));
        let on_red = time_left.is_some_and(|left| left < crossing_s + SIGNAL_MARGIN_S);
        // A walker cutting the zebra's corner is up to `CUT_M` beside the zebra, so the
        // cars it meets are those of the zebra within that much more of its line.
        let reach = CROSSING_REACH
            + if self.cut_pieces(walker, k).is_empty() {
                0.0
            } else {
                CUT_M
            };
        let vehicles = self.vehicles_ahead(walker, &roads, walker.pos, far, stale, reach);
        let soonest = vehicles.iter().map(|(time, _)| *time).reduce(f64::min);
        if on_red {
            let chance = red_go_chance(
                t.red_habit,
                walker.frustration,
                clear_factor(soonest, crossing_s),
                self.followers(walker),
                walker.members.len() > 1,
            );
            if f64::from(draw.red) >= chance {
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
        // Everybody looks: the gap is never below the crossing time. Risk only shrinks
        // the margin on top of it.
        let needed = crossing_s + margin * (1.0 - MARGIN_SHRINK * bend);
        let careful_needed = crossing_s + margin;
        let trust = on_zebra && !on_red;
        let holds = |need: f64| {
            vehicles
                .iter()
                .any(|&(time, can_stop)| time <= need && !(trust && can_stop))
        };
        if holds(needed) {
            return Some(KerbChoice::Wait);
        }
        Some(KerbChoice::Go {
            gap_s: soonest,
            on_red,
            short_gap: holds(careful_needed),
        })
    }

    /// Is the corner cut at the zebra after kerb `kerb_index` of the walker's path? Fixed
    /// by its spawn draw and its habit, so the walker steers the same way all the time.
    pub(super) fn cuts_at(path: &WalkPath, walker: &Walker, kerb_index: usize) -> bool {
        let ordinal = path.kerbs[..kerb_index].iter().filter(|k| **k).count();
        walker.no_cut != Some(kerb_index)
            && walker.kerb_draws.get(ordinal).is_some_and(|draw| {
                f64::from(draw.corner)
                    < corner_chance(walker.traits.corner_habit, walker.traits.risk)
            })
    }

    /// A car stands on the piece beside the zebra the walker would cut across, near it
    /// (a queue at the zebra): cutting there would put the walker between cars that wait
    /// for it, so it crosses on the zebra.
    pub(super) fn cut_blocked(&self, walker: &Walker) -> bool {
        let path = &self.walk_paths[walker.path];
        path.cuts
            .iter()
            .filter(|cut| cut.kerb == walker.area_index && Self::cuts_at(path, walker, cut.kerb))
            .any(|cut| {
                let piece = &self.walk_space.areas[cut.piece].polygon;
                self.agents.values().any(|car| {
                    car.speed < super::crowd::STANDING_VEHICLE
                        && piece.contains(car.pos)
                        && car.pos.distance(walker.pos) < CUT_BLOCK_RANGE_M
                })
            })
    }

    /// The carriageway pieces a walker at the kerb `k` of its path would also step onto
    /// (it cuts the corner of the zebra there).
    fn cut_pieces(&self, walker: &Walker, k: usize) -> Vec<usize> {
        let path = &self.walk_paths[walker.path];
        path.cuts
            .iter()
            .filter(|cut| cut.kerb == k && Self::cuts_at(path, walker, cut.kerb))
            .map(|cut| cut.piece)
            .collect()
    }

    /// The portals the walker steers through: its path's, with the two of a zebra it
    /// cuts replaced by the wider ones.
    pub(super) fn walker_portals(&self, walker: &Walker) -> Cow<'_, [(Vec2, Vec2)]> {
        let path = &self.walk_paths[walker.path];
        let cuts: Vec<_> = path
            .cuts
            .iter()
            .filter(|cut| Self::cuts_at(path, walker, cut.kerb))
            .collect();
        if cuts.is_empty() {
            return Cow::Borrowed(&path.portals);
        }
        let mut portals = path.portals.clone();
        for cut in cuts {
            portals[cut.kerb] = cut.near;
            portals[cut.kerb + 1] = cut.far;
        }
        Cow::Owned(portals)
    }

    /// The road areas the walker may be in: those of its corridor (`WalkSpace::open_roads`)
    /// and the piece beside a zebra whose corner it cuts, from the kerb to the far side.
    pub(super) fn open_for(
        space: &WalkSpace,
        path: &WalkPath,
        walker: &Walker,
        cleared: bool,
    ) -> Vec<usize> {
        let index = walker.area_index;
        let mut open = space.open_roads(path, index, cleared);
        for cut in &path.cuts {
            let in_use = (cut.kerb == index && cleared) || cut.kerb + 1 == index;
            if in_use && Self::cuts_at(path, walker, cut.kerb) && !open.contains(&cut.piece) {
                open.push(cut.piece);
            }
        }
        open
    }

    /// The road area a walker is on: its corridor's, or the piece beside a zebra it cuts.
    pub(super) fn walker_area(&self, walker: &Walker) -> usize {
        let path = &self.walk_paths[walker.path];
        let area = path.areas[walker.area_index];
        for cut in &path.cuts {
            if (cut.kerb == walker.area_index || cut.kerb + 1 == walker.area_index)
                && self.walk_space.areas[cut.piece]
                    .polygon
                    .contains(walker.pos)
            {
                return cut.piece;
            }
        }
        area
    }

    /// How many other people near the walker are already crossing (not its group).
    fn followers(&self, walker: &Walker) -> usize {
        self.walkers
            .values()
            .filter(|other| {
                other.id != walker.id
                    && other.group != walker.group
                    && !other.hit()
                    && other.pos.distance(walker.pos) <= SOCIAL_RANGE
                    && (other.cleared_kerb == Some(other.area_index)
                        || self.walk_space.areas
                            [self.walk_paths[other.path].areas[other.area_index]]
                            .kind
                            .is_road())
            })
            .count()
    }

    /// Walkers with two ways decide between them (once), when they get near the place
    /// where the free way leaves the sidewalk, from the traffic they see then. A group
    /// follows its first member. Runs at the start of the walkers' tick.
    pub(super) fn way_stage(&mut self) {
        let mut decided: Vec<(AgentId, bool)> = Vec::new();
        for (id, walker) in &self.walkers {
            if walker.way.is_none() || walker.hit() {
                continue;
            }
            let lead_decides = walker.group != walker.id
                && self
                    .walkers
                    .get(&walker.group)
                    .is_some_and(|lead| lead.way.is_some() && !lead.hit());
            if lead_decides {
                continue;
            }
            let Some(free) = self.way_decision(walker) else {
                continue;
            };
            if walker.group == walker.id {
                decided.extend(walker.members.iter().map(|m| (*m, free)));
            } else {
                decided.push((*id, free));
            }
        }
        for (id, free) in decided {
            let Some(walker) = self.walkers.get_mut(&id) else {
                continue;
            };
            let Some(way) = walker.way.take() else {
                continue;
            };
            let here = self.walk_paths[walker.path].areas[walker.area_index];
            let stepping = walker.cleared_kerb == Some(walker.area_index);
            if free && !stepping && self.walk_paths[way.free].areas[way.free_index] == here {
                walker.path = way.free;
                walker.area_index = way.free_index;
                walker.off_crossing = true;
                walker.detour_s = 0.0;
            }
        }
    }

    /// `Some(true)`: the walker takes the free way; `Some(false)`: the rule way; `None`:
    /// it is not at the place to decide yet.
    fn way_decision(&self, walker: &Walker) -> Option<bool> {
        let way = walker.way?;
        // It looks around for a tick before it decides (and a test can set its traits).
        if self.tick <= walker.spawn_tick {
            return None;
        }
        if walker.cleared_kerb == Some(walker.area_index) || walker.kerb_since.is_some() {
            return Some(false);
        }
        let free = &self.walk_paths[way.free];
        let here = self.walk_paths[walker.path].areas[walker.area_index];
        if free.areas[way.free_index] != here {
            return None;
        }
        let (l, r) = free.portals[way.free_index];
        let at = closest_point_to_segment(walker.pos, l, r);
        if at.distance(walker.pos) > DECISION_RANGE_M {
            return None;
        }
        if way.legal {
            // Nothing to break: no crossing within the legal distance.
            return Some(true);
        }
        let (roads, exit) = self.roads_ahead_on(free, way.free_index);
        let far = match free.portals.get(exit) {
            Some(&(l, r)) => closest_point_to_segment(at, l, r),
            None => closest_point_to_segment(at, free.goal.0, free.goal.1),
        };
        let crossing_s = f64::from(far.distance(at)) / walker.desired_speed.max(0.1);
        let vehicles = self.vehicles_ahead(walker, &roads, at, far, false, CROSSING_REACH);
        let soonest = vehicles.iter().map(|(time, _)| *time).reduce(f64::min);
        let ease = traffic_ease(clear_factor(soonest, crossing_s));
        let chance = jaywalk_chance(walker.traits.zebra_habit, way.detour_m) * ease;
        Some(f64::from(way.draw) < chance.min(1.0))
    }

    /// The vehicles the walker sees coming to its crossing line (from where it stands to
    /// `far`) on one of `roads`, seen one reaction time ago and brought forward to now:
    /// (seconds until it gets there, whether it can still stop before it).
    fn vehicles_ahead(
        &self,
        walker: &Walker,
        roads: &[usize],
        from: glam::Vec2,
        far: glam::Vec2,
        stale: bool,
        reach: f32,
    ) -> Vec<(f64, bool)> {
        self.vehicles_meeting(walker, roads, from, far, stale, reach)
            .into_iter()
            .map(|m| (m.time, m.can_stop))
            .collect()
    }

    /// `vehicles_ahead` with where along `from`-`far` each vehicle meets the line, and
    /// whether it is slowing down.
    fn vehicles_meeting(
        &self,
        walker: &Walker,
        roads: &[usize],
        from: glam::Vec2,
        far: glam::Vec2,
        stale: bool,
        reach: f32,
    ) -> Vec<Meeting> {
        let reaction = walker.traits.reaction_s + if stale { STALE_VIEW_S } else { 0.0 };
        let back = ((reaction / DT).round() as usize)
            .max(1)
            .min(self.history.len());
        let snapshot = &self.history[self.history.len() - back];
        // Looking properly, it brings what it saw forward to now; a walker who did not
        // look takes the old view as it is.
        let age = if stale { 0.0 } else { back as f64 * DT };
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
                let (mut near_m, mut far_m) = (f32::INFINITY, f32::NEG_INFINITY);
                let along = (far - from).normalize_or_zero();
                for &(d, point) in &route_area.line {
                    if closest_point_to_segment(point, from, far).distance(point) < reach {
                        meet = Some(meet.map_or((d, d), |(a, b)| (a.min(d), b.max(d))));
                        let m = (point - from).dot(along);
                        near_m = near_m.min(m);
                        far_m = far_m.max(m);
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
                found.push(Meeting {
                    time,
                    can_stop,
                    enter_m: near_m,
                    leave_m: far_m,
                    slowing: self.was_slowing(seen, back),
                    moving: seen.speed > ROAD_MOVING_SPEED,
                });
            }
        }
        found
    }
}

impl StreetRun {
    /// Was the vehicle braking at the view the walker looks at (`view_back` snapshots back)?
    /// It lost speed over the half second before it.
    fn was_slowing(&self, seen: &super::drive::Seen, view_back: usize) -> bool {
        let back = (SLOW_LOOK_S / DT).round() as usize;
        let len = self.history.len();
        // With too little history nothing is known, so it counts as not slowing.
        let Some(older) = len
            .checked_sub(view_back + back)
            .and_then(|i| self.history.get(i))
        else {
            return false;
        };
        older
            .seen
            .iter()
            .find(|s| s.id == seen.id)
            .is_some_and(|s| s.speed - seen.speed > SLOWING_DROP)
    }

    /// The index of the kerb portal the walker's current road run starts at.
    fn road_entry_kerb(&self, walker: &Walker) -> Option<usize> {
        let path = &self.walk_paths[walker.path];
        (0..=walker.area_index.min(path.kerbs.len().saturating_sub(1)))
            .rev()
            .find(|&i| path.kerbs[i])
    }

    /// Is the walker further than `BAND_STANDOFF_M` (beyond its body) from the line of every
    /// vehicle route through the road area it is on?
    fn clear_of_bands(&self, walker: &Walker, area: usize) -> bool {
        self.clear_of_bands_at(walker.pos, walker.radius, area)
    }

    fn clear_of_bands_at(&self, pos: Vec2, radius: f32, area: usize) -> bool {
        self.route_areas.iter().all(|areas| {
            areas
                .iter()
                .find(|a| a.area == area)
                .is_none_or(|route_area| {
                    super::yielding::project(&route_area.line, pos).is_none_or(|(_, beside)| {
                        beside >= BAND_HALF_WIDTH + radius + BAND_STANDOFF_M
                    })
                })
        })
    }

    /// Where a walker that steps back goes: just inside the road from the kerb it came
    /// from (it never leaves the area it is in).
    pub(super) fn road_back_target(&self, walker: &Walker) -> Vec2 {
        let path = &self.walk_paths[walker.path];
        let Some(kerb) = self.road_entry_kerb(walker) else {
            return walker.pos;
        };
        let (l, r) = path.portals[kerb];
        let at = closest_point_to_segment(walker.pos, l, r);
        at + (walker.pos - at).normalize_or_zero() * (walker.radius + 0.15)
    }

    /// A walker on a carriageway (a zebra or not) keeps looking at the vehicles, from the
    /// same delayed views as at the kerb (a look it misses there it misses here: the same
    /// draw), every `ROAD_LOOK_S`. A vehicle that reaches its path within `ROAD_THREAT_S`
    /// and is not slowing down makes it react: if it has not yet reached that vehicle's
    /// lane it stops, or steps back when too close to the lane; if it is in or near the
    /// lane it hurries forward or steps back, whichever gets it out of the path sooner.
    /// A rare assertive walker (`assertive_chance`, a draw per kerb made at spawn) keeps
    /// its pace for a car that can still stop, as long as the car has more than the
    /// walker's reaction time and `ASSERT_MARGIN_S` left. `Some` is the decision of a look
    /// made this tick; `None`, no look: the last decision stands.
    pub(super) fn road_look(&self, walker: &Walker) -> Option<RoadAct> {
        let path = &self.walk_paths[walker.path];
        let k = walker.area_index;
        let area = self.walker_area(walker);
        let on_road = !walker.hit() && self.walk_space.areas[area].kind.is_road();
        if !on_road {
            return (walker.road_act != RoadAct::Free).then_some(RoadAct::Free);
        }
        if !(self.tick + walker.id.0 as u64).is_multiple_of(ROAD_LOOK_TICKS) {
            return None;
        }
        let base = if self.walk_space.areas[path.areas[k]].kind.is_road() {
            k.saturating_sub(1)
        } else {
            k
        };
        let (mut roads, exit) = self.roads_ahead_on(path, base);
        if area != path.areas[k] && !roads.contains(&area) {
            roads.push(area);
        }
        let portals = self.walker_portals(walker);
        let far = match portals.get(exit) {
            Some(&(l, r)) => closest_point_to_segment(walker.pos, l, r),
            None => closest_point_to_segment(walker.pos, path.goal.0, path.goal.1),
        };
        let to_far = far - walker.pos;
        if to_far.length() < 0.3 {
            return Some(RoadAct::Free);
        }
        let dir = to_far.normalize_or_zero();
        let entry = self.road_entry_kerb(walker);
        let ordinal = entry.map_or(0, |kerb| path.kerbs[..kerb].iter().filter(|x| **x).count());
        let draw = walker.kerb_draws.get(ordinal).copied().unwrap_or(KerbDraw {
            red: 1.0,
            look: 1.0,
            corner: 1.0,
            assertive: 1.0,
        });
        let t = &walker.traits;
        let stale = f64::from(draw.look) < miss_chance(t.attention);
        // A walker who missed its look at the kerb is distracted for the whole crossing of
        // it (phone, talk): it keeps only the short-range reflex of `crowd` (`CAR_AHEAD`).
        if stale {
            return Some(RoadAct::Free);
        }
        let assertive = f64::from(draw.assertive) < assertive_chance(t.courtesy, t.risk);
        let from = walker.pos - dir * ROAD_LOOK_BACK_M;
        let meetings = self.vehicles_meeting(walker, &roads, from, far, stale, CROSSING_REACH);
        let radius = walker.radius;
        let speed = walker.desired_speed.max(0.1);
        let back_room = entry.map_or(0.0, |i| {
            let (l, r) = path.portals[i];
            closest_point_to_segment(walker.pos, l, r).distance(walker.pos) - radius - 0.15
        });
        // Stepping back is no help where the spot behind is itself in a band (the strip
        // between the kerb and the first lane is narrower than a body).
        let back_room = if self.clear_of_bands_at(self.road_back_target(walker), radius, area) {
            back_room
        } else {
            0.0
        };
        let on_zebra = matches!(self.walk_space.areas[area].kind, AreaKind::Crossing { .. });
        let mut worst: Option<&Meeting> = None;
        let mut holds = false;
        for m in &meetings {
            let leave = m.leave_m - ROAD_LOOK_BACK_M;
            if m.time > ROAD_THREAT_S || m.slowing || !m.moving || leave < -radius {
                continue;
            }
            if assertive && m.can_stop && m.time > t.reaction_s + ASSERT_MARGIN_S {
                holds = true;
                continue;
            }
            // On a zebra it trusts a driver who can still stop, as at the kerb, and
            // drivers brake late there, so only a vehicle very close in time is a threat.
            if on_zebra && (m.can_stop || m.time > ZEBRA_THREAT_S) {
                continue;
            }
            if worst.is_none_or(|w| m.time < w.time) {
                worst = Some(m);
            }
        }
        let Some(m) = worst else {
            return Some(if holds { RoadAct::Hold } else { RoadAct::Free });
        };
        let enter = m.enter_m - ROAD_LOOK_BACK_M;
        let leave = m.leave_m - ROAD_LOOK_BACK_M + radius;
        // It stops only clear of every vehicle band of the road (the stop must not be a near
        // miss itself); beside one, it gets out of it like a walker in the lane.
        if enter > radius + 0.5 && self.clear_of_bands(walker, area) {
            // Not in its lane yet.
            return Some(if enter < ROAD_STOP_CLEARANCE_M && back_room > 0.2 {
                RoadAct::Back
            } else {
                RoadAct::Stop
            });
        }
        let hurry_s = f64::from(leave.max(0.0)) / (speed * HURRY_FACTOR);
        let out_back = radius - enter;
        let back_s = if back_room > 0.2 && back_room >= out_back {
            f64::from(out_back.max(0.0)) / speed
        } else {
            f64::INFINITY
        };
        let ok = |s: f64| s + HURRY_MARGIN_S <= m.time;
        let back = match (ok(hurry_s), ok(back_s)) {
            (true, true) => back_s < hurry_s,
            (false, true) => true,
            (true, false) => false,
            (false, false) => back_s < hurry_s,
        };
        Some(if back { RoadAct::Back } else { RoadAct::Hurry })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_street_with_no_car_is_clear_and_one_with_a_car_on_the_line_is_not() {
        assert_eq!(clear_factor(None, 5.0), 1.0);
        assert_eq!(clear_factor(Some(5.0), 5.0), 0.0);
        assert_eq!(
            clear_factor(Some(5.0 + ROAD_MARGIN_S + TRAFFIC_HORIZON_S), 5.0),
            1.0
        );
        assert!(clear_factor(Some(9.0), 5.0) < clear_factor(Some(12.0), 5.0));
        assert!(traffic_ease(0.0) < traffic_ease(1.0));
        assert_eq!(traffic_ease(1.0), 1.0);
    }

    #[test]
    fn the_red_chance_follows_the_traffic_the_habit_and_the_people_around() {
        assert_eq!(
            red_go_chance(1.0, 1.0, 1.0, 3, false),
            0.0,
            "a compliant walker"
        );
        assert_eq!(
            red_go_chance(0.3, 0.0, 0.0, 3, false),
            0.0,
            "a car on the line"
        );
        let alone = red_go_chance(0.5, 0.0, 1.0, 0, false);
        assert!(
            red_go_chance(0.5, 0.0, 1.0, 2, false) > alone,
            "people follow"
        );
        assert!(red_go_chance(0.5, 0.0, 1.0, 0, true) < alone, "groups wait");
        assert!(
            red_go_chance(0.5, 0.5, 1.0, 0, false) > alone,
            "frustration"
        );
        assert!(
            red_go_chance(0.5, 0.0, 0.5, 0, false) < alone,
            "cars coming"
        );
        assert!(red_go_chance(0.0, 1.0, 1.0, 3, false) <= 1.0);
    }

    #[test]
    fn a_walker_who_looks_never_misses_and_one_who_is_corner_compliant_never_cuts() {
        assert_eq!(miss_chance(1.0), 0.0);
        assert!(miss_chance(0.5) > miss_chance(0.9));
        assert_eq!(corner_chance(1.0, 1.0), 0.0);
        assert!(corner_chance(0.3, 0.9) > corner_chance(0.3, 0.1));
    }
}
