//! Pedestrians (pivot plan §5.2, §5.3): a circle body that moves freely in the walkable
//! space, with a personality drawn at spawn from its own random stream.
//!
//! **Two collections.** Walkers live in `StreetRun::walkers`, apart from the vehicles in
//! `StreetRun::agents`. A walker has no route distance, no lane and no junction approach,
//! and a vehicle no 2D velocity or corridor: one struct with a kind would make every car
//! code path test the kind. Walker ids start at `WALKER_ID_BASE` and have their own
//! counter, so walkers never shift the ids (and so the random streams, which are seeded
//! from the id) of the vehicles: the car traffic of a level is the same with or without
//! its pedestrians as long as they do not meet (Phase 3 stage A). Ids stay unique in the
//! event log.
//!
//! **Where to cross (pivot plan Phase 3).** Each trip has a rule way (streets crossed
//! only on crossings) and a free way (a carriageway crossed anywhere), see `walk`. When
//! the free way crosses out of a crossing and saves a detour, a walker takes it with a
//! probability that grows with the detour and falls with its compliance
//! (`jaywalk_chance`). French law (Code de la route, R412-37) asks pedestrians to use a
//! crossing that is less than 50 m away; when none is that close, crossing anywhere is
//! allowed and everybody takes the free way if it saves `FREE_WAY_MIN_SAVING_M`.
//!
//! **Draw order.** Every walker's stream gives, at spawn and always in this order: the
//! archetype (1 draw), the traits (8), then for the first of a group only the group
//! size, the entry and the place on the entry line (3), then the way draw (1; the
//! group follows its first member's choice, the others draw it and do not use it), then
//! one draw per kerb of the chosen way (the decision to go on red there). Nothing is
//! drawn later, so the streams never depend on what happens in the run.
//!
//! **Groups.** One spawn can bring a small group (pairs, threes) that walks at the pace of
//! its slowest member and stays together (`crowd`). Each member has its own id and stream.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::agent::Population;
use super::crowd::ARRIVE_DISTANCE;
use super::events::{CauseKind, StreetEvent};
use super::walk::{AreaKind, WalkPath, path_length, string_pull};
use super::{StreetRun, mix};
use crate::geom::polygon::segment_intersection;
use crate::geom::segment::closest_point_to_segment;
use crate::rng::SimRng;
use crate::street::ids::AgentId;

/// The first walker id. Vehicles count from 1 and never get this far in a run.
pub const WALKER_ID_BASE: i32 = 1_000_000;
/// Group sizes per spawn: the share of spawns that are pairs, then threes. Start values
/// for a school street at rush hour (in shopping streets more than half the people walk
/// in groups: Moussaïd et al. 2010, PLoS ONE 5(4): e10047).
pub const GROUP_PAIR_SHARE: f32 = 0.20;
pub const GROUP_THREE_SHARE: f32 = 0.07;
/// The mean number of people a spawn brings. A pedestrian flow makes this many times
/// fewer wishes, so its `per_hour` counts people, not groups.
pub const MEAN_GROUP_SIZE: f32 = 1.0 + GROUP_PAIR_SHARE + 2.0 * GROUP_THREE_SHARE;
/// Members of a group come in this far apart, metres (start value: side by side with a
/// small gap).
pub const GROUP_SPACING: f32 = 0.7;
/// A walker comes in only this far clear of everybody already there, metres.
pub const SPAWN_GAP: f32 = 0.1;

/// French law: a pedestrian must use a crossing less than this far away, metres (Code de
/// la route, R412-37).
pub const LEGAL_CROSSING_DISTANCE: f32 = 50.0;
/// Where crossing anywhere is allowed, a walker leaves the crossings only to save at
/// least this much walking, metres (start value).
pub const FREE_WAY_MIN_SAVING_M: f32 = 5.0;
/// The share of the people who would break the rule that do it grows as
/// detour / (detour + `DETOUR_HALF_M`): half of them at this detour, metres (start
/// value).
pub const DETOUR_HALF_M: f32 = 20.0;
/// Scales `1 - compliance` into the chance to ignore a crossing (start value: a walker
/// of compliance 0.5 ignores a far crossing almost always).
pub const JAYWALK_GAIN: f32 = 2.0;

/// The chance that a walker of `compliance` crosses out of a crossing to save `detour_m`.
pub fn jaywalk_chance(compliance: f64, detour_m: f32) -> f64 {
    let detour = f64::from(detour_m.max(0.0));
    let share = detour / (detour + f64::from(DETOUR_HALF_M));
    (f64::from(JAYWALK_GAIN) * (1.0 - compliance) * share).min(1.0)
}

/// The two ways of a trip from one entry to a gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WalkChoice {
    /// Crossing streets only on crossings; `None` when there is no such way.
    pub rule: Option<usize>,
    /// Crossing anywhere, when that is a different way.
    pub free: Option<usize>,
    /// How much longer the rule way is, metres.
    pub detour_m: f32,
    /// No crossing within `LEGAL_CROSSING_DISTANCE` of where the free way crosses.
    pub legal: bool,
}

/// Is this id a walker's?
pub fn is_walker(id: AgentId) -> bool {
    id.0 >= WALKER_ID_BASE
}

/// The ranges a walker's personality is drawn from: (lowest, highest) per trait.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WalkerArchetype {
    pub name: &'static str,
    /// Free walking speed, m/s.
    pub speed: (f32, f32),
    /// Body radius, m.
    pub radius: (f32, f32),
    pub patience_s: (f32, f32),
    pub compliance: (f32, f32),
    pub attention: (f32, f32),
    pub reaction_s: (f32, f32),
    pub risk: (f32, f32),
    pub courtesy: (f32, f32),
}

// Start values, to be calibrated in Phase 8. Free speeds: a mean of 1.34 m/s for adults
// (Weidmann 1993, "Transporttechnik der Fussgänger", ETH Zürich); older people about
// 1.0 to 1.2 m/s (Knoblauch, Pietrucha & Nitzburg 1996, Transportation Research Record
// 1538). Radii: half a shoulder width (about 0.46 m for adults, Weidmann 1993).
pub const WALK_CAREFUL: WalkerArchetype = WalkerArchetype {
    name: "careful walker",
    speed: (1.2, 1.5),
    radius: (0.22, 0.28),
    patience_s: (60.0, 120.0),
    compliance: (1.0, 1.0),
    attention: (1.0, 1.0),
    reaction_s: (0.5, 0.9),
    risk: (0.0, 0.0),
    courtesy: (1.0, 1.0),
};
pub const WALK_COMMUTER: WalkerArchetype = WalkerArchetype {
    name: "commuter on foot",
    speed: (1.35, 1.65),
    radius: (0.22, 0.28),
    patience_s: (30.0, 60.0),
    compliance: (0.6, 0.95),
    attention: (0.8, 0.98),
    reaction_s: (0.5, 0.9),
    risk: (0.2, 0.6),
    courtesy: (0.3, 0.8),
};
pub const WALK_RESIDENT: WalkerArchetype = WalkerArchetype {
    name: "resident on foot",
    speed: (1.15, 1.45),
    radius: (0.22, 0.28),
    patience_s: (60.0, 120.0),
    compliance: (0.8, 1.0),
    attention: (0.85, 1.0),
    reaction_s: (0.6, 1.0),
    risk: (0.0, 0.4),
    courtesy: (0.5, 1.0),
};
pub const WALK_CHILD: WalkerArchetype = WalkerArchetype {
    name: "child",
    speed: (1.0, 1.4),
    radius: (0.18, 0.22),
    patience_s: (20.0, 60.0),
    compliance: (0.5, 0.9),
    attention: (0.5, 0.85),
    reaction_s: (0.8, 1.3),
    risk: (0.2, 0.8),
    courtesy: (0.3, 0.9),
};
pub const WALK_ELDERLY: WalkerArchetype = WalkerArchetype {
    name: "older person",
    speed: (0.8, 1.15),
    radius: (0.22, 0.28),
    patience_s: (90.0, 150.0),
    compliance: (0.9, 1.0),
    attention: (0.8, 0.95),
    reaction_s: (0.9, 1.5),
    risk: (0.0, 0.2),
    courtesy: (0.7, 1.0),
};
pub const WALK_HURRIED: WalkerArchetype = WalkerArchetype {
    name: "hurried walker",
    speed: (1.4, 1.8),
    radius: (0.22, 0.28),
    patience_s: (10.0, 30.0),
    compliance: (0.2, 0.6),
    attention: (0.5, 0.85),
    reaction_s: (0.6, 1.2),
    risk: (0.5, 1.0),
    courtesy: (0.0, 0.5),
};

impl Population {
    /// One draw from the walker's stream picks the archetype.
    pub fn walker_archetype(self, rng: &mut SimRng) -> WalkerArchetype {
        let pick = rng.randf();
        match self {
            Population::Careful => WALK_CAREFUL,
            Population::Careless => WALK_HURRIED,
            Population::Mixed if pick < 0.4 => WALK_COMMUTER,
            Population::Mixed if pick < 0.7 => WALK_RESIDENT,
            Population::Mixed if pick < 0.85 => WALK_CHILD,
            Population::Mixed => WALK_ELDERLY,
        }
    }
}

/// The personality of one walker (pivot plan §5.3), fixed at spawn. Compliance,
/// attention, reaction, risk and courtesy are not used before crossings and cars
/// (stage B and C); they are drawn now so the stream stays the same then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WalkerTraits {
    pub speed: f64,
    pub radius: f64,
    pub patience_s: f64,
    pub compliance: f64,
    pub attention: f64,
    pub reaction_s: f64,
    pub risk: f64,
    pub courtesy: f64,
}

impl WalkerTraits {
    /// Eight draws, always in this order.
    pub fn draw(archetype: &WalkerArchetype, rng: &mut SimRng) -> Self {
        let mut pick = |range: (f32, f32)| f64::from(rng.randf_range(range.0, range.1));
        Self {
            speed: pick(archetype.speed),
            radius: pick(archetype.radius),
            patience_s: pick(archetype.patience_s),
            compliance: pick(archetype.compliance),
            attention: pick(archetype.attention),
            reaction_s: pick(archetype.reaction_s),
            risk: pick(archetype.risk),
            courtesy: pick(archetype.courtesy),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Walker {
    pub id: AgentId,
    pub archetype: &'static str,
    pub traits: WalkerTraits,
    /// Stage B and C draw from it (kerb decisions, looking at cars).
    #[allow(dead_code)]
    pub(super) rng: SimRng,
    /// Index into the run's walk paths.
    pub path: usize,
    /// Where the walker is in its corridor: an index into the path's areas.
    pub area_index: usize,
    pub radius: f32,
    /// The speed it walks at on a free sidewalk: its own, or its group's (the slowest
    /// member's).
    pub desired_speed: f64,
    /// Middle of the body, metres, y down; `prev_pos` one tick before.
    pub pos: Vec2,
    pub vel: Vec2,
    pub prev_pos: Vec2,
    /// The velocity at the start of the tick (for the stability measure).
    pub vel_before: Vec2,
    /// The acceleration the forces asked for last tick (for the stability measure).
    pub accel: Vec2,
    /// The first member's id; the walker's own when it walks alone.
    pub group: AgentId,
    /// Every member of its group, itself included, in id order.
    pub members: Vec<AgentId>,
    pub spawn_tick: u64,
    /// The trip time on an empty sidewalk.
    pub free_time_s: f64,
    /// People per square metre around it, smoothed over about a second.
    pub density: f64,
    pub crowded_ticks: u32,
    pub bumps: u32,
    /// Ticks in a row it has hardly moved.
    pub stuck_ticks: u32,
    /// The fixed obstacle it is going around, and on which side (true = right).
    pub detour: Option<(usize, bool)>,
    /// Its way crosses a carriageway out of a crossing.
    pub off_crossing: bool,
    /// The detour the rule way made it walk, seconds (0 when it took the free way).
    pub detour_s: f64,
    /// One draw per kerb of its way, in order (the decision to go on red).
    pub kerb_draws: Vec<f32>,
    /// The corridor index whose kerb it has decided to step over.
    pub cleared_kerb: Option<usize>,
    /// Waiting at a kerb since this tick.
    pub kerb_since: Option<u64>,
    /// It decided to wait at a kerb this tick (waiting counts as stuck, however much the
    /// people around jostle it).
    pub waiting: bool,
    pub kerb_wait_ticks: u32,
    /// 0..1: grows while it waits at a kerb, eases slowly while it walks.
    pub frustration: f64,
    /// What it did wrong lately (crossing out of a zebra, on red, in a short gap), for
    /// the cause chains.
    pub recent: Vec<(u64, CauseKind)>,
    /// Hit by a vehicle: it stays where it fell until this tick, then is taken away.
    pub hit_until: Option<u64>,
}

impl Walker {
    pub fn hit(&self) -> bool {
        self.hit_until.is_some()
    }
}

impl Walker {
    pub fn speed(&self) -> f64 {
        f64::from(self.vel.length())
    }
}

/// What came of an attempt to bring walkers in.
#[derive(Clone, Debug, PartialEq)]
pub enum WalkSpawn {
    Spawned(Vec<AgentId>),
    /// The entry is crowded; try again later.
    Blocked,
    /// No walkable way between the gates (or a gate has no entry).
    NoRoute,
}

/// A walker's own stream and what was drawn from it at spawn.
struct Drawn {
    id: AgentId,
    rng: SimRng,
    archetype: WalkerArchetype,
    traits: WalkerTraits,
}

impl StreetRun {
    fn draw_walker(&self, id: AgentId) -> Drawn {
        let mut rng = SimRng::new(mix(self.seed, id.0 as u64));
        let archetype = self.population.walker_archetype(&mut rng);
        let traits = WalkerTraits::draw(&archetype, &mut rng);
        Drawn {
            id,
            rng,
            archetype,
            traits,
        }
    }

    /// The ways from entry `from` to gate `to`, found once per run; `None` when there is
    /// none.
    fn walk_choice_for(&mut self, from: usize, to: &str) -> Option<WalkChoice> {
        let key = (from, to.to_string());
        if let Some(found) = self.walk_route_cache.get(&key) {
            return *found;
        }
        let rule = self.walk_space.find_path(from, to, false);
        let free = self
            .walk_space
            .find_path(from, to, true)
            .filter(|path| path.off_crossing);
        let found = if rule.is_none() && free.is_none() {
            None
        } else {
            let detour_m = match (&rule, &free) {
                (Some(r), Some(f)) => (r.length - f.length).max(0.0),
                _ => 0.0,
            };
            let legal = free
                .as_ref()
                .is_some_and(|f| self.legal_off_crossing(from, f));
            let mut push = |path: WalkPath| {
                self.walk_paths.push(path);
                self.walk_paths.len() - 1
            };
            Some(WalkChoice {
                rule: rule.map(&mut push),
                free: free.map(&mut push),
                detour_m,
                legal,
            })
        };
        self.walk_route_cache.insert(key, found);
        found
    }

    /// Where `path` (from entry `from`) first steps onto a carriageway, is there no
    /// crossing of the same street (any segment with its name) within
    /// `LEGAL_CROSSING_DISTANCE`, measured between positions?
    fn legal_off_crossing(&self, from: usize, path: &WalkPath) -> bool {
        let space = &self.walk_space;
        let Some(j) = path
            .areas
            .iter()
            .position(|a| matches!(space.areas[*a].kind, AreaKind::Carriageway { .. }))
        else {
            return true;
        };
        let AreaKind::Carriageway { segment } = space.areas[path.areas[j]].kind else {
            return true;
        };
        let entry = &space.entries[from];
        let goal = (path.goal.0 + path.goal.1) * 0.5;
        let line = string_pull((entry.a + entry.b) * 0.5, &path.portals, goal);
        let (l, r) = path.portals[j - 1];
        let point = line
            .windows(2)
            .find_map(|pair| {
                segment_intersection(pair[0], pair[1], l, r).map(|t| pair[0].lerp(pair[1], t))
            })
            .unwrap_or((l + r) * 0.5);
        let name = &self.street.segments[&segment].name;
        self.street.crossings.values().all(|c| {
            let other = &self.street.segments[&c.segment];
            if c.segment != segment && (name.is_empty() || other.name != *name) {
                return true;
            }
            let at = point_at(&other.centerline, c.at);
            at.distance(point) >= LEGAL_CROSSING_DISTANCE
        })
    }

    /// Where a walker at `pos` on `path` heads first, and the length of its whole way.
    pub(super) fn walk_target(path: &WalkPath, area_index: usize, pos: Vec2) -> (Vec2, f32) {
        let last = path.areas.len() - 1;
        let end = if area_index >= last {
            closest_point_to_segment(pos, path.goal.0, path.goal.1)
        } else {
            let (l, r) = path.portals[last - 1];
            closest_point_to_segment((l + r) * 0.5, path.goal.0, path.goal.1)
        };
        let points = string_pull(pos, &path.portals[area_index.min(last)..], end);
        let mut target = points[1.min(points.len() - 1)];
        if target.distance(pos) < 0.01 && points.len() > 2 {
            target = points[2];
        }
        (target, path_length(&points))
    }

    /// Where the walkers of one spawn stand at the entry: side by side across the entry
    /// line, in rows when it is too narrow.
    fn entry_positions(&self, entry_index: usize, drawn: &[Drawn], lateral: f32) -> Vec<Vec2> {
        let size = drawn.len();
        let entry = &self.walk_space.entries[entry_index];
        let length = entry.a.distance(entry.b);
        let along = (entry.b - entry.a).normalize_or_zero();
        let margin = drawn
            .iter()
            .map(|d| d.traits.radius as f32)
            .fold(0.0, f32::max)
            + SPAWN_GAP;
        let per_row = (((length - 2.0 * margin) / GROUP_SPACING).floor() as usize + 1).max(1);
        let mut positions = Vec::with_capacity(size);
        for k in 0..size {
            let (row, col) = (k / per_row, k % per_row);
            let in_row = per_row.min(size - row * per_row);
            let half = (in_row - 1) as f32 * 0.5 * GROUP_SPACING;
            let low = margin + half;
            let high = length - margin - half;
            let center = if low <= high {
                (lateral * length).clamp(low, high)
            } else {
                length * 0.5
            };
            let t = center + (col as f32 * GROUP_SPACING - half);
            positions
                .push(entry.a + along * t + entry.inward * (margin + row as f32 * GROUP_SPACING));
        }
        positions
    }

    /// Brings in walkers from gate `from` to gate `to`: one walker, or the group its draws
    /// make (`group_size` overrides the size drawn, for tests). The draws come from the
    /// first walker's stream, so a blocked attempt draws the same again next time.
    pub fn add_walkers(&mut self, from: &str, to: &str, group_size: Option<usize>) -> WalkSpawn {
        let first = AgentId(self.next_walker);
        let mut lead = self.draw_walker(first);
        let size_draw = lead.rng.randf();
        let entry_draw = lead.rng.randf();
        let lateral = lead.rng.randf();
        let size = group_size.unwrap_or(if size_draw < GROUP_PAIR_SHARE {
            2
        } else if size_draw < GROUP_PAIR_SHARE + GROUP_THREE_SHARE {
            3
        } else {
            1
        });
        let entries: Vec<usize> = self.walk_space.entries_of(from).collect();
        let open: Vec<(usize, WalkChoice)> = entries
            .into_iter()
            .filter_map(|entry| self.walk_choice_for(entry, to).map(|c| (entry, c)))
            .collect();
        if open.is_empty() || size == 0 {
            return WalkSpawn::NoRoute;
        }
        let pick = ((entry_draw * open.len() as f32) as usize).min(open.len() - 1);
        let (entry_index, choice) = open[pick];
        let way_draw = f64::from(lead.rng.randf());
        let takes_free = match (choice.rule, choice.free) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(_), Some(_)) => {
                if choice.legal {
                    choice.detour_m >= FREE_WAY_MIN_SAVING_M
                } else {
                    way_draw < jaywalk_chance(lead.traits.compliance, choice.detour_m)
                }
            }
        };
        let path_index = if takes_free {
            choice.free.expect("a free way")
        } else {
            choice.rule.expect("a rule way")
        };
        let kerbs = self.walk_paths[path_index]
            .kerbs
            .iter()
            .filter(|k| **k)
            .count();
        let kerb_draws = |d: &mut Drawn| (0..kerbs).map(|_| d.rng.randf()).collect::<Vec<f32>>();
        let mut drawn = vec![lead];
        for k in 1..size {
            let mut member = self.draw_walker(AgentId(first.0 + k as i32));
            member.rng.randf();
            drawn.push(member);
        }
        let draws: Vec<Vec<f32>> = drawn.iter_mut().map(kerb_draws).collect();
        // A group that does not fit at the entry (it is too narrow, or an obstacle stands
        // there) comes in as its first walker alone; only a crowd at the entry makes the
        // spawn wait.
        let mut positions = self.entry_positions(entry_index, &drawn, lateral);
        let fits = |run: &Self, drawn: &[Drawn], positions: &[Vec2]| {
            drawn.iter().zip(positions).all(|(d, pos)| {
                run.walk_space.contains_for(*pos, &[])
                    && run.walk_space.obstacle_depth(*pos, d.traits.radius as f32) <= 0.0
            })
        };
        if !fits(self, &drawn, &positions) {
            if drawn.len() == 1 {
                return WalkSpawn::NoRoute;
            }
            drawn.truncate(1);
            positions = self.entry_positions(entry_index, &drawn, lateral);
            if !fits(self, &drawn, &positions) {
                return WalkSpawn::NoRoute;
            }
        }
        let size = drawn.len();
        for (d, pos) in drawn.iter().zip(&positions) {
            let r = d.traits.radius as f32;
            let crowded = self
                .walkers
                .values()
                .any(|w| w.pos.distance(*pos) < w.radius + r + SPAWN_GAP);
            if crowded {
                return WalkSpawn::Blocked;
            }
        }
        let speed = drawn
            .iter()
            .map(|d| d.traits.speed)
            .fold(f64::INFINITY, f64::min);
        let members: Vec<AgentId> = drawn.iter().map(|d| d.id).collect();
        let path = &self.walk_paths[path_index];
        let detour_s = if takes_free || choice.free.is_none() {
            0.0
        } else {
            f64::from(choice.detour_m) / speed
        };
        let mut ids = Vec::with_capacity(size);
        let mut spawned = Vec::with_capacity(size);
        for ((d, pos), kerb_draws) in drawn.into_iter().zip(positions).zip(draws) {
            let (target, length) = Self::walk_target(path, 0, pos);
            let vel = (target - pos).normalize_or_zero() * speed as f32;
            ids.push(d.id);
            spawned.push(Walker {
                id: d.id,
                archetype: d.archetype.name,
                traits: d.traits,
                rng: d.rng,
                path: path_index,
                area_index: 0,
                radius: d.traits.radius as f32,
                desired_speed: speed,
                pos,
                vel,
                prev_pos: pos,
                vel_before: vel,
                accel: Vec2::ZERO,
                group: first,
                members: members.clone(),
                spawn_tick: self.tick,
                free_time_s: f64::from((length - ARRIVE_DISTANCE).max(0.0)) / speed,
                density: 0.0,
                crowded_ticks: 0,
                bumps: 0,
                stuck_ticks: 0,
                detour: None,
                off_crossing: path.off_crossing,
                detour_s,
                kerb_draws,
                cleared_kerb: None,
                kerb_since: None,
                waiting: false,
                kerb_wait_ticks: 0,
                frustration: 0.0,
                recent: Vec::new(),
                hit_until: None,
            });
        }
        self.next_walker += size as i32;
        for walker in spawned {
            self.log(StreetEvent::WalkerSpawned {
                agent: walker.id,
                archetype: walker.archetype.to_string(),
                from: from.to_string(),
                to: to.to_string(),
                group: first,
                group_size: size as u32,
            });
            self.walkers.insert(walker.id, walker);
        }
        WalkSpawn::Spawned(ids)
    }
}

/// A walker as the others see it at the end of a tick.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WalkerSeen {
    pub id: AgentId,
    pub pos: Vec2,
    pub vel: Vec2,
    pub radius: f32,
    pub group: AgentId,
    /// The walk-space area its corridor has it in.
    pub area: Option<usize>,
    /// The road area it is about to step onto: waiting at the kerb, decided to go, or
    /// walking up to the kerb (drivers yield to it at a zebra).
    pub intent: Option<usize>,
    pub hit: bool,
}

/// The point `distance` metres along `centerline` (its last point beyond its end).
pub fn point_at(centerline: &[Vec2], distance: f32) -> Vec2 {
    let mut left = distance;
    for pair in centerline.windows(2) {
        let length = pair[0].distance(pair[1]);
        if left <= length {
            return pair[0].lerp(pair[1], (left / length.max(1e-6)).max(0.0));
        }
        left -= length;
    }
    *centerline.last().expect("a centerline has points")
}
