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
//! **Groups.** One spawn can bring a small group (pairs, threes) that walks at the pace of
//! its slowest member and stays together (`crowd`). Each member has its own id and stream.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::agent::Population;
use super::events::StreetEvent;
use super::walk::{WalkPath, path_length, string_pull};
use super::{StreetRun, mix};
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
/// Members of a group come in this far apart, metres (start value: side by side with a
/// small gap).
pub const GROUP_SPACING: f32 = 0.7;
/// A walker comes in only this far clear of everybody already there, metres.
pub const SPAWN_GAP: f32 = 0.1;

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

    /// The path index from entry `from` to gate `to`, found once per run.
    fn walk_path_for(&mut self, from: usize, to: &str) -> Option<usize> {
        let key = (from, to.to_string());
        if let Some(found) = self.walk_route_cache.get(&key) {
            return *found;
        }
        let found = self.walk_space.find_path(from, to).map(|path| {
            self.walk_paths.push(path);
            self.walk_paths.len() - 1
        });
        self.walk_route_cache.insert(key, found);
        found
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
        let open: Vec<(usize, usize)> = entries
            .into_iter()
            .filter_map(|entry| self.walk_path_for(entry, to).map(|path| (entry, path)))
            .collect();
        if open.is_empty() || size == 0 {
            return WalkSpawn::NoRoute;
        }
        let pick = ((entry_draw * open.len() as f32) as usize).min(open.len() - 1);
        let (entry_index, path_index) = open[pick];
        let mut drawn = vec![lead];
        for k in 1..size {
            drawn.push(self.draw_walker(AgentId(first.0 + k as i32)));
        }
        // Side by side across the entry line, in rows when it is too narrow.
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
        for (d, pos) in drawn.iter().zip(&positions) {
            let r = d.traits.radius as f32;
            if !self.walk_space.contains(*pos) || self.walk_space.obstacle_depth(*pos, r) > 0.0 {
                return WalkSpawn::Blocked;
            }
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
        let mut ids = Vec::with_capacity(size);
        let mut spawned = Vec::with_capacity(size);
        for (d, pos) in drawn.into_iter().zip(positions) {
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
                free_time_s: f64::from(length) / speed,
                density: 0.0,
                crowded_ticks: 0,
                bumps: 0,
                stuck_ticks: 0,
                detour: None,
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
}
