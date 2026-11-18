//! One run of a level (pivot plan §5): the day's traffic on the street the player
//! designed, stepped at 20 Hz. A run is a pure function of `level + design + seed`:
//! every agent has its own random stream, agents decide from the world as it was one
//! reaction time ago, so the order they are updated in does not matter, and nothing
//! from the frame enters. The event log is the result; `summary()` and `log_hash()` read
//! it.
//!
//! A tick goes through these stages, in this order (one private method each): slice,
//! spawn, prepare, decide, integrate, walk, zones, contacts, removals, snapshot, end.
//!
//! Vehicles are `agents` (they follow lanes, `drive`); pedestrians are `walkers` (they
//! move freely in the walkable space, `walk`, `walker`, `crowd`, and wait at the kerb,
//! `kerb`). Walkers look at vehicles before they cross; vehicles do not see walkers yet
//! and the two never touch (Phase 3 stage C).

pub mod agent;
mod conflicts;
pub mod crowd;
pub mod detect;
pub mod drive;
pub mod events;
pub mod grid;
pub mod kerb;
mod meet;
pub mod perceive;
pub mod route;
pub mod sight;
mod spawn;
pub mod walk;
pub mod walker;
pub mod yielding;

use std::collections::VecDeque;

use glam::Vec2;
use indexmap::IndexMap;
use serde::Serialize;

use agent::chance;
pub use agent::{Agent, Approach, Population, Traits};
use conflicts::{ZoneExits, flush_zones};
pub use crowd::WalkStats;
use drive::{DT, Decision, MoveInfo, STOPPED_SPEED, Seen, Snapshot, TURN_ACCELERATION, World};
use events::rounded;
pub use events::{
    Cause, CauseKind, ConflictKind, EndReason, Grade, LoggedEvent, Outcome, StreetEvent,
};
use grid::Grid;
use route::RoutePath;
use walk::{WalkPath, WalkSpace};
pub use walker::{WalkSpawn, Walker, is_walker};

use crate::rng::SimRng;
use crate::street::design::Design;
use crate::street::ids::{AgentId, StreetLaneId};
use crate::street::lanes::Turn;
use crate::street::level::{Gate, JunctionControl, LevelFile, Mode, Sign, Slice};
use crate::street::model::{EditError, Street};
use crate::street::signals::{
    DEFAULT_ALL_RED_S, DEFAULT_AMBER_S, DEFAULT_GREEN_S, Light, SignalPhase, SignalPlan,
    pair_opposite_arms,
};

/// Ticks per second of play time.
pub const TICK_RATE: u32 = 20;
/// Snapshots kept: the newest ones are what agents have seen a reaction time ago.
pub const HISTORY: usize = 40;
/// A vehicle stopped by a contact stays this long before it is taken away.
pub const CRASH_BLOCK_S: f64 = 15.0;
/// An agent stuck this long leaves the level.
pub const GIVE_UP_S: f64 = 120.0;
/// After the last slice the run waits this long for the street to empty.
pub const GRACE_S: f64 = 90.0;
/// Post-encroachment times below these make a light / serious conflict.
pub const PET_LIGHT_S: f64 = 1.5;
pub const PET_SERIOUS_S: f64 = 0.75;
/// What an agent did wrong stays blamable this long.
pub const RECENT_S: f64 = 8.0;
/// The same kind of conflict between the same two is logged once per this many seconds.
pub const CONFLICT_COOLDOWN_S: f64 = 5.0;
/// Chance per second, times `1 - attention`, to look away.
pub const DISTRACTION_RATE: f64 = 0.08;

fn splitmix64(x: u64) -> u64 {
    let x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let z = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Seeds for the independent streams of a run.
pub(crate) fn mix(a: u64, b: u64) -> u64 {
    splitmix64(a ^ splitmix64(b))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunOptions {
    pub seed: u64,
    pub population: Population,
    /// Multiplies every flow of the level.
    pub demand_scale: f64,
}

impl RunOptions {
    pub fn for_level(level: &LevelFile) -> Self {
        Self {
            seed: level.seed,
            population: Population::Mixed,
            demand_scale: 1.0,
        }
    }
}

/// What the event log adds up to.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Summary {
    pub spawned: u32,
    pub arrived: u32,
    pub gave_up: u32,
    /// Vehicles still on the street at the end, and wishes that never entered (the
    /// score counts both like give-ups).
    pub unfinished: u32,
    pub not_entered: u32,
    pub conflicts_light: u32,
    pub conflicts_serious: u32,
    pub contacts: u32,
    pub light_injuries: u32,
    pub serious_injuries: u32,
    pub deaths: u32,
    pub mean_delay_s: f64,
    pub total_annoyance: f64,
    pub ticks: u64,
    pub ended: Option<EndReason>,
    /// The pedestrians (the fields above count vehicles only).
    pub walkers: WalkSummary,
}

/// What the pedestrians of a run add up to.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct WalkSummary {
    pub spawned: u32,
    pub arrived: u32,
    pub gave_up: u32,
    /// People who did not finish or never entered (`Unfinished` events).
    pub unfinished: u32,
    pub not_entered: u32,
    pub mean_delay_s: f64,
    /// Mean time an arrived walker spent in a dense crowd.
    pub mean_crowded_s: f64,
    pub bumps: u32,
    pub total_annoyance: f64,
    /// Still on the street when the run ended (the grace time ran out).
    pub still_walking: u32,
    /// Steps off a kerb, those out of a crossing, and those against a red light.
    pub crossings: u32,
    pub off_crossing: u32,
    pub on_red: u32,
    /// Contacts with vehicles (the vehicle fields above count vehicles against vehicles
    /// only), and their outcomes for the walkers.
    pub contacts: u32,
    pub light_injuries: u32,
    pub serious_injuries: u32,
    pub deaths: u32,
    /// Mean time an arrived walker waited at kerbs.
    pub mean_kerb_wait_s: f64,
}

/// Agents of one flow of one slice that want to enter, and the stream that makes them.
struct FlowState {
    rng: SimRng,
    /// The ticks at which the agents that still want to enter wished to (oldest first).
    backlog: Vec<u64>,
}

pub struct StreetRun {
    pub tick: u64,
    /// The vehicles.
    pub agents: IndexMap<AgentId, Agent>,
    /// The pedestrians (`walker`: why they are kept apart).
    pub walkers: IndexMap<AgentId, Walker>,
    pub walk_space: WalkSpace,
    pub walk_paths: Vec<WalkPath>,
    pub walk_stats: WalkStats,
    pub street: Street,
    seed: u64,
    population: Population,
    demand_scale: f64,
    slices: Vec<Slice>,
    gates: Vec<Gate>,
    /// Tick at which each slice ends.
    slice_ends: Vec<u64>,
    current_slice: Option<usize>,
    flow_state: Vec<Vec<FlowState>>,
    moves: IndexMap<StreetLaneId, MoveInfo>,
    /// Per junction node, the segments traffic comes in from, in movement order.
    approaches: IndexMap<i32, Vec<i32>>,
    plans: IndexMap<i32, SignalPlan>,
    /// Street speed of every road lane, computed once (the street is fixed in a run).
    street_speeds: crate::street::speed::StreetSpeeds,
    routes: Vec<RoutePath>,
    /// Per route (same index): the road areas of the walk space it drives through, with
    /// its route distances in and out and its line through them (`kerb::road_areas_of`).
    route_areas: Vec<Vec<kerb::RouteArea>>,
    /// Crossing areas under signals: (junction, the approaches that conflict).
    walk_signals: IndexMap<usize, (i32, Vec<i32>)>,
    /// What blocks sight, and the kerb points of each zebra (`yielding`).
    sight: sight::SightBlockers,
    kerb_points: IndexMap<usize, Vec<Vec2>>,
    /// (class bits, from gate, to gate) -> index into `routes`; `None` when no way exists.
    route_cache: IndexMap<(u8, String, String), Option<usize>>,
    next_agent: i32,
    /// (entry, destination gate) -> its ways (indices into `walk_paths`); `None` when no
    /// way exists.
    walk_route_cache: IndexMap<(usize, String), Option<walker::WalkChoice>>,
    next_walker: i32,
    /// The tick of the last bump logged between two walkers.
    bump_last: IndexMap<(AgentId, AgentId), u64>,
    history: VecDeque<Snapshot>,
    /// (movement, the movement it conflicts with) -> who left the zone of the first
    /// against the second, and when.
    zone_exits: ZoneExits,
    touching: Vec<(AgentId, AgentId)>,
    /// Who is in the band of each vehicle route through each road area (`meet`).
    bands: meet::Bands,
    conflict_last: IndexMap<(ConflictKind, AgentId, AgentId), u64>,
    log: Vec<LoggedEvent>,
    ended: Option<EndReason>,
}

/// The movements of every junction, as the drivers see them, and the approaches of each.
fn movement_table(street: &Street) -> (IndexMap<StreetLaneId, MoveInfo>, IndexMap<i32, Vec<i32>>) {
    let mut moves = IndexMap::new();
    let mut approaches: IndexMap<i32, Vec<i32>> = IndexMap::new();
    for (node, junction) in &street.junctions {
        let signals = junction.control == JunctionControl::Signals;
        for id in &junction.movement_ids {
            let lane = &street.lanes[id];
            let feeder = &street.lanes[&lane.feeder.expect("a movement has a feeder")];
            let from_segment = feeder.segment_id.expect("a feeder is a road lane");
            let fed = feeder.polyline();
            let heading = (fed[fed.len() - 1] - fed[fed.len() - 2]).normalize_or_zero();
            let sign = junction
                .signs
                .get(&from_segment)
                .copied()
                .unwrap_or(Sign::None);
            let rank = if signals {
                2
            } else {
                match sign {
                    Sign::None => 2,
                    Sign::Yield => 1,
                    Sign::Stop => 0,
                }
            };
            moves.insert(
                *id,
                MoveInfo {
                    node: *node,
                    from_segment,
                    heading,
                    turn: lane.turn,
                    out_lane: lane.outgoing[0],
                    turn_speed: turn_speed(&lane.polyline(), lane.turn),
                    rank,
                    sign,
                    signals,
                },
            );
            let list = approaches.entry(*node).or_default();
            if !list.contains(&from_segment) {
                list.push(from_segment);
            }
        }
    }
    (moves, approaches)
}

/// The speed at which a turn asks `TURN_ACCELERATION` sideways: `sqrt(a·R)` with the
/// radius of the arc between the movement's ends (square roots only, no trigonometry,
/// so every platform gets the same bits).
fn turn_speed(points: &[Vec2], turn: Turn) -> f64 {
    if matches!(turn, Turn::Straight | Turn::None) || points.len() < 2 {
        return f64::INFINITY;
    }
    let n = points.len();
    let chord = f64::from(points[0].distance(points[n - 1]));
    let start = (points[1] - points[0]).normalize_or_zero();
    let end = (points[n - 1] - points[n - 2]).normalize_or_zero();
    let flat = 1.0 - f64::from(start.dot(end));
    if flat < 1e-6 {
        return f64::INFINITY;
    }
    let radius = chord / (2.0 * (flat / 2.0).sqrt());
    (TURN_ACCELERATION * radius).sqrt()
}

/// The plans of the junctions under signals: the level's own, or one phase per pair of
/// opposite approaches.
fn signal_plans(
    street: &Street,
    moves: &IndexMap<StreetLaneId, MoveInfo>,
    approaches: &IndexMap<i32, Vec<i32>>,
) -> IndexMap<i32, SignalPlan> {
    let mut plans = IndexMap::new();
    for (node, junction) in &street.junctions {
        if junction.control != JunctionControl::Signals {
            continue;
        }
        if let Some(plan) = &junction.signal_plan {
            plans.insert(*node, plan.clone());
            continue;
        }
        let heading_of = |segment: i32| {
            moves
                .values()
                .find(|m| m.node == *node && m.from_segment == segment)
                .map_or(Vec2::ZERO, |m| m.heading)
        };
        let arms = approaches.get(node).cloned().unwrap_or_default();
        let phases = pair_opposite_arms(&arms, heading_of)
            .into_iter()
            .map(|green| SignalPhase {
                green,
                green_s: DEFAULT_GREEN_S,
            })
            .collect();
        plans.insert(
            *node,
            SignalPlan {
                phases,
                amber_s: DEFAULT_AMBER_S,
                all_red_s: DEFAULT_ALL_RED_S,
            },
        );
    }
    plans
}

impl StreetRun {
    pub fn new(
        level: &LevelFile,
        design: &Design,
        options: RunOptions,
    ) -> Result<StreetRun, EditError> {
        let street = Street::from_level_and_design(level, design)?;
        let (moves, approaches) = movement_table(&street);
        let plans = signal_plans(&street, &moves, &approaches);
        let street_speeds = crate::street::speed::street_speeds(&street);
        let mut end = 0;
        let slice_ends = level
            .demand
            .iter()
            .map(|slice| {
                end += (f64::from(slice.duration_s) / DT).round() as u64;
                end
            })
            .collect();
        let flow_state = level
            .demand
            .iter()
            .enumerate()
            .map(|(s, slice)| {
                (0..slice.flows.len())
                    .map(|f| FlowState {
                        rng: SimRng::new(mix(options.seed, 1_000_000 + (s * 1000 + f) as u64)),
                        backlog: Vec::new(),
                    })
                    .collect()
            })
            .collect();
        let first = Snapshot {
            seen: Vec::new(),
            grid: Grid::new(street.bounds),
            walkers: Vec::new(),
            walker_grid: Grid::with_cell(street.bounds, crowd::WALKER_CELL),
            tick: 0,
            noise: perceive::background_noise(0),
        };
        let walk_space = WalkSpace::new(&street, &level.gates);
        let walk_signals = kerb::walk_signal_table(&walk_space, &street, &moves);
        let sight = sight::SightBlockers::new(&street);
        let kerb_points = yielding::kerb_points(&walk_space);
        Ok(StreetRun {
            tick: 0,
            agents: IndexMap::new(),
            walkers: IndexMap::new(),
            walk_space,
            walk_paths: Vec::new(),
            walk_stats: WalkStats::default(),
            street,
            seed: options.seed,
            population: options.population,
            demand_scale: options.demand_scale,
            slices: level.demand.clone(),
            gates: level.gates.clone(),
            slice_ends,
            current_slice: None,
            flow_state,
            moves,
            approaches,
            plans,
            street_speeds,
            routes: Vec::new(),
            route_areas: Vec::new(),
            walk_signals,
            sight,
            kerb_points,
            route_cache: IndexMap::new(),
            next_agent: 1,
            walk_route_cache: IndexMap::new(),
            next_walker: walker::WALKER_ID_BASE,
            bump_last: IndexMap::new(),
            history: VecDeque::from([first]),
            zone_exits: IndexMap::new(),
            touching: Vec::new(),
            bands: IndexMap::new(),
            conflict_last: IndexMap::new(),
            log: Vec::new(),
            ended: None,
        })
    }

    /// The signal plan of each junction under signals (the level's, the design's, or the
    /// default one), by node id. For views, which colour the lights from it.
    pub fn signal_plans(&self) -> &IndexMap<i32, SignalPlan> {
        &self.plans
    }

    pub fn finished(&self) -> bool {
        self.ended.is_some()
    }

    pub fn ended(&self) -> Option<EndReason> {
        self.ended
    }

    pub fn events(&self) -> &[LoggedEvent] {
        &self.log
    }

    /// The light the agent's next junction movement shows now (this tick's time); `None`
    /// without a movement ahead or without signals there. For tests and views.
    pub fn light_for(&self, agent: &Agent) -> Option<Light> {
        let info = &self.moves[&agent.approach.as_ref()?.movement];
        if !info.signals {
            return None;
        }
        self.plans
            .get(&info.node)
            .map(|plan| plan.light(info.from_segment, self.tick as f64 * DT))
    }

    /// The speed limit of the lane under the agent's front bumper, metres per second.
    pub fn limit_for(&self, agent: &Agent) -> f64 {
        let route = &self.routes[agent.route];
        self.street.lanes[&route.lanes[route.lane_index(agent.d)]].speed_limit
    }

    pub fn run_to_end(&mut self) {
        while !self.finished() {
            self.step();
        }
    }

    /// Advances one tick; does nothing once the run has ended.
    pub fn step(&mut self) {
        if self.ended.is_some() {
            return;
        }
        self.slice_stage();
        self.spawn_stage();
        self.prepare_stage();
        let decisions = self.decide_stage();
        self.integrate_stage(decisions);
        self.walk_stage();
        self.zone_stage();
        self.contact_stage();
        self.walker_contact_stage();
        self.walker_pet_stage();
        self.removal_stage();
        self.walk_removal_stage();
        self.snapshot_stage();
        self.tick += 1;
        self.end_stage();
    }

    fn log(&mut self, event: StreetEvent) {
        self.log.push(LoggedEvent {
            tick: self.tick,
            event,
        });
    }

    fn total_ticks(&self) -> u64 {
        self.slice_ends.last().copied().unwrap_or(0)
    }

    /// Stage 1: slices run one after the other; after the last there is none.
    fn slice_stage(&mut self) {
        let current = self.slice_ends.iter().position(|end| self.tick < *end);
        if current != self.current_slice {
            self.current_slice = current;
            if let Some(index) = current {
                let name = self.slices[index].name.clone();
                self.log(StreetEvent::SliceStarted { index, name });
            }
        }
    }

    /// Stage 3: what an agent works out for itself: the movement coming up (and how it
    /// means to behave there), distraction, and the old blame forgotten.
    fn prepare_stage(&mut self) {
        let tick = self.tick;
        for agent in self.agents.values_mut() {
            let route = &self.routes[agent.route];
            let rear = agent.d - agent.length;
            if agent.approach.as_ref().is_some_and(|a| rear >= a.end_d) {
                flush_zones(agent, &mut self.zone_exits, tick);
                agent.approach = None;
            }
            if agent.approach.is_none() {
                let start = route.lane_index(rear.max(0.0));
                let found = (start..route.lanes.len()).find(|&j| {
                    self.moves.contains_key(&route.lanes[j]) && route.lane_end(j) > rear
                });
                if let Some(j) = found {
                    let movement = route.lanes[j];
                    let info = &self.moves[&movement];
                    let t = agent.traits;
                    let p = ((1.0 - t.compliance) * (1.0 + 2.0 * agent.frustration)).min(1.0);
                    let obeys_signal = chance(&mut agent.rng, 1.0 - p);
                    let yields = chance(&mut agent.rng, 1.0 - 0.5 * p);
                    let full_stop = chance(&mut agent.rng, 1.0 - p);
                    let keeps_box = chance(&mut agent.rng, t.courtesy);
                    let mut unseen = Vec::new();
                    for segment in &self.approaches[&info.node] {
                        if *segment != info.from_segment
                            && !chance(&mut agent.rng, 1.0 - 0.5 * (1.0 - t.attention))
                        {
                            unseen.push(*segment);
                        }
                    }
                    agent.approach = Some(Approach {
                        movement,
                        line_d: route.offsets[j],
                        end_d: route.lane_end(j),
                        obeys_signal,
                        yields,
                        full_stop,
                        keeps_box,
                        unseen,
                        stopped_at_sign: false,
                        go: false,
                        committed: false,
                    });
                }
            }
            // The zebra coming up: two draws when it comes up (pivot plan Phase 3 stage C,
            // `yielding`), after the junction's and before the per-tick ones.
            if agent
                .crossing
                .as_ref()
                .is_some_and(|crossing| rear >= crossing.exit_d)
            {
                agent.crossing = None;
            }
            if agent.crossing.is_none() {
                let next = self.route_areas[agent.route].iter().find(|area| {
                    area.exit > rear
                        && matches!(
                            self.walk_space.areas[area.area].kind,
                            walk::AreaKind::Crossing { .. }
                        )
                });
                if let Some(area) = next {
                    let t = agent.traits;
                    let p = ((1.0 - t.compliance) * (1.0 + 2.0 * agent.frustration)).min(1.0);
                    let yields = chance(&mut agent.rng, 1.0 - 0.5 * p);
                    let looks = chance(&mut agent.rng, 1.0 - 0.5 * (1.0 - t.attention));
                    agent.crossing = Some(yielding::CrossingApproach {
                        area: area.area,
                        enter_d: area.enter,
                        exit_d: area.exit,
                        yields,
                        looks,
                        entered: false,
                        pending_yield: Vec::new(),
                        worst_fast: None,
                    });
                }
            }
            // Both draws every tick, so the stream does not depend on the outcome.
            let looks_away = chance(
                &mut agent.rng,
                (1.0 - agent.traits.attention) * DISTRACTION_RATE * DT,
            );
            let duration = f64::from(agent.rng.randf_range(1.0, 2.5));
            if looks_away && !agent.crashed() && tick >= agent.distracted_until {
                agent.distracted_until = tick + (duration / DT).round() as u64;
                agent.recent.push((tick, CauseKind::Distracted));
            }
            agent
                .recent
                .retain(|(t, _)| (tick - t) as f64 * DT <= RECENT_S);
        }
    }

    /// Stage 4: every decision is taken from the past, before any agent moves.
    fn decide_stage(&self) -> Vec<Decision> {
        let walk_lights: IndexMap<usize, bool> = self
            .walk_signals
            .keys()
            .map(|area| (*area, self.walk_light(*area).unwrap_or(true)))
            .collect();
        let peds = yielding::PedWorld {
            space: &self.walk_space,
            route_areas: &self.route_areas,
            sight: &self.sight,
            kerb_points: &self.kerb_points,
            walk_lights: &walk_lights,
            perception: Some(yielding::Perception {
                history: &self.history,
                tick: self.tick,
            }),
        };
        let world = World {
            street: &self.street,
            routes: &self.routes,
            moves: &self.moves,
            plans: &self.plans,
            street_speeds: &self.street_speeds,
            time_s: self.tick as f64 * DT,
            peds: Some(&peds),
        };
        self.agents
            .values()
            .map(|agent| {
                let ticks = ((agent.traits.reaction_s / DT).round() as usize).max(1);
                let back = ticks.min(self.history.len());
                let snapshot = &self.history[self.history.len() - back];
                drive::decide(agent, snapshot, &world, self.tick)
            })
            .collect()
    }

    /// Stage 5: apply the decisions and move.
    fn integrate_stage(&mut self, decisions: Vec<Decision>) {
        let tick = self.tick;
        let mut brakes: Vec<(AgentId, AgentId, f64)> = Vec::new();
        for (agent, decision) in self.agents.values_mut().zip(decisions) {
            if agent.crashed() {
                agent.prev_pos = agent.pos;
                agent.prev_dir = agent.dir;
                continue;
            }
            if let Some(approach) = &mut agent.approach {
                if let Some(go) = decision.go {
                    approach.go = go;
                }
                approach.stopped_at_sign |= decision.stopped_at_sign;
                approach.committed |= decision.committed;
            }
            agent.held_by_walkers = decision.held_by_walkers;
            agent.gaze_task = decision.task;
            if decision.held_by_walkers {
                agent.last_held = Some(tick);
            }
            if let Some(crossing) = &mut agent.crossing {
                crossing.entered |= decision.crossing_entered;
                if let Some((not_yielding, too_fast)) = decision.crossing_pending {
                    // Kept until the front enters the zebra: close to it a careful driver
                    // could no longer stop, but it would have stopped earlier.
                    for walker in not_yielding {
                        if !crossing.pending_yield.contains(&walker) {
                            crossing.pending_yield.push(walker);
                        }
                    }
                    if let Some((speed, limit)) = too_fast
                        && crossing
                            .worst_fast
                            .is_none_or(|(s, l)| speed - limit > s - l)
                    {
                        crossing.worst_fast = Some((speed, limit));
                    }
                }
            }
            for cause in decision.causes {
                agent.recent.push((tick, cause));
            }
            if let Some((other, rate)) = decision.hard_brake {
                agent.last_hard_brake = Some(tick);
                brakes.push((agent.id, other, rate));
            }
            agent.accel = drive::brake_build_up(agent.accel, decision.accel, agent.max_brake);
            agent.speed = (agent.speed + agent.accel * DT).max(0.0);
            agent.d += agent.speed * DT;
            agent.prev_pos = agent.pos;
            agent.prev_dir = agent.dir;
            (agent.pos, agent.dir) = spawn::pose(
                &self.street,
                &self.routes[agent.route],
                agent.d,
                agent.length,
                agent.dir,
            );
            if agent.speed < STOPPED_SPEED {
                agent.wait_ticks += 1;
                agent.frustration = (agent.frustration + DT / agent.traits.patience_s).min(1.0);
            } else {
                agent.wait_ticks = 0;
                agent.frustration = (agent.frustration - DT / 60.0).max(0.0);
            }
        }
        for (braker, other, rate) in brakes {
            let position = self.agents[&braker].pos;
            let grade = Self::brake_grade(rate);
            self.log_conflict(
                ConflictKind::HardBrake,
                grade,
                (braker, other),
                position,
                rate,
            );
        }
    }

    /// Stage 8: trips that end, vehicles cleared after a contact, agents who give up.
    fn removal_stage(&mut self) {
        let tick = self.tick;
        let ids: Vec<AgentId> = self.agents.keys().copied().collect();
        for id in ids {
            let agent = &self.agents[&id];
            let route = &self.routes[agent.route];
            let event = if agent.d - agent.length >= route.length {
                let time_s = (tick - agent.spawn_tick) as f64 * DT;
                let delay_s = (time_s - agent.free_time_s).max(0.0);
                StreetEvent::TripEnded {
                    agent: id,
                    time_s: rounded(time_s, 100.0),
                    delay_s: rounded(delay_s, 100.0),
                    annoyance: rounded(delay_s / agent.traits.patience_s, 100.0),
                }
            } else if agent.crashed_until.is_some_and(|until| until <= tick) {
                StreetEvent::Cleared { agent: id }
            } else if !agent.crashed() && f64::from(agent.wait_ticks) * DT > GIVE_UP_S {
                StreetEvent::GaveUp {
                    agent: id,
                    waited_s: rounded(f64::from(agent.wait_ticks) * DT, 100.0),
                }
            } else {
                continue;
            };
            let mut gone = self.agents.shift_remove(&id).expect("the agent is there");
            flush_zones(&mut gone, &mut self.zone_exits, tick);
            self.touching.retain(|(a, b)| *a != id && *b != id);
            self.log(event);
        }
    }

    /// Stage 9: what the agents will see next tick.
    fn snapshot_stage(&mut self) {
        let mut grid = Grid::new(self.street.bounds);
        let mut seen = Vec::with_capacity(self.agents.len());
        for agent in self.agents.values() {
            let route = &self.routes[agent.route];
            let index = route.lane_index(agent.d);
            let (movement, s_on_movement, in_junction) = match &agent.approach {
                Some(a) => (
                    Some(a.movement),
                    agent.d - a.line_d,
                    agent.d >= a.line_d && agent.d - agent.length < a.end_d,
                ),
                None => (None, 0.0, false),
            };
            grid.insert(seen.len() as u32, agent.pos);
            seen.push(Seen {
                id: agent.id,
                pos: agent.pos,
                dir: agent.dir,
                length: agent.length,
                width: agent.width,
                speed: agent.speed,
                lane: route.lanes[index],
                s: agent.d - route.offsets[index],
                movement,
                s_on_movement,
                in_junction,
                wait_ticks: agent.wait_ticks,
                crashed: agent.crashed(),
                hides: matches!(agent.mode, Mode::Van | Mode::Bus),
                held_by_walkers: agent.held_by_walkers,
                level: perceive::sound_level(agent.mode, agent.speed, agent.electric),
            });
        }
        let (walkers, walker_grid) = self.walker_snapshot();
        let moving = seen.iter().filter(|s| s.speed > 1.0).count();
        self.history.push_back(Snapshot {
            seen,
            grid,
            walkers,
            walker_grid,
            tick: self.tick,
            noise: perceive::background_noise(moving),
        });
        while self.history.len() > HISTORY {
            self.history.pop_front();
        }
    }

    /// Stage 10: a death ends the run (its event comes last in the tick's log); so does
    /// the day being over with the street empty, or the grace time running out.
    fn end_stage(&mut self) {
        if self.ended == Some(EndReason::Death) {
            self.log(StreetEvent::RunEnded {
                reason: EndReason::Death,
            });
            return;
        }
        let total = self.total_ticks();
        let grace = total + (GRACE_S / DT).round() as u64;
        let waiting = self
            .flow_state
            .iter()
            .flatten()
            .any(|flow| !flow.backlog.is_empty());
        let empty = self.agents.is_empty() && self.walkers.is_empty();
        if self.tick >= total && ((empty && !waiting) || self.tick >= grace) {
            self.ended = Some(EndReason::Completed);
            self.log_unfinished();
            self.log(StreetEvent::RunEnded {
                reason: EndReason::Completed,
            });
        }
    }

    /// At the end of a completed run, every person who did not get through is logged as
    /// `Unfinished`: those still on the street, and those still waiting to enter (the
    /// backlog). Without it the score would only see who arrived or gave up, so a design
    /// that jams would lose its slowest people from the delay mean and score better than
    /// one that does not (a jam must cost points). A backlog wish of pedestrians counts
    /// `MEAN_GROUP_SIZE` people, rounded (the group size is drawn at entry). Order: the
    /// vehicles, the walkers, then the backlog by slice and flow, so the log is the same
    /// on every run.
    fn log_unfinished(&mut self) {
        let tick = self.tick;
        let mut events = Vec::new();
        for agent in self.agents.values() {
            if agent.crashed() {
                continue;
            }
            let time_s = (tick - agent.spawn_tick) as f64 * DT;
            events.push(StreetEvent::Unfinished {
                agent: Some(agent.id),
                mode: agent.mode,
                delay_s: rounded((time_s - agent.free_time_s).max(0.0), 100.0),
                entered: true,
            });
        }
        for walker in self.walkers.values() {
            if walker.hit() {
                continue;
            }
            let time_s = (tick - walker.spawn_tick) as f64 * DT;
            events.push(StreetEvent::Unfinished {
                agent: Some(walker.id),
                mode: Mode::Pedestrian,
                delay_s: rounded((time_s - walker.free_time_s).max(0.0), 100.0),
                entered: true,
            });
        }
        for (s, slice) in self.slices.iter().enumerate() {
            for (f, flow) in slice.flows.iter().enumerate() {
                let people = if flow.mode == Mode::Pedestrian {
                    walker::MEAN_GROUP_SIZE.round().max(1.0) as usize
                } else {
                    1
                };
                for &wished in &self.flow_state[s][f].backlog {
                    for _ in 0..people {
                        events.push(StreetEvent::Unfinished {
                            agent: None,
                            mode: flow.mode,
                            delay_s: rounded((tick - wished) as f64 * DT, 100.0),
                            entered: false,
                        });
                    }
                }
            }
        }
        for event in events {
            self.log(event);
        }
    }

    /// FNV-1a 64 over the JSON of every event and a newline: the replay hash.
    pub fn log_hash(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for event in &self.log {
            let line = serde_json::to_string(event).expect("an event always serializes");
            for byte in line.bytes().chain(std::iter::once(b'\n')) {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
        hash
    }

    pub fn summary(&self) -> Summary {
        let mut s = Summary {
            spawned: 0,
            arrived: 0,
            gave_up: 0,
            unfinished: 0,
            not_entered: 0,
            conflicts_light: 0,
            conflicts_serious: 0,
            contacts: 0,
            light_injuries: 0,
            serious_injuries: 0,
            deaths: 0,
            mean_delay_s: 0.0,
            total_annoyance: 0.0,
            ticks: self.tick,
            ended: self.ended,
            walkers: WalkSummary::default(),
        };
        let mut delay = 0.0;
        let (mut walk_delay, mut walk_crowded, mut kerb_wait) = (0.0, 0.0, 0.0);
        s.walkers.still_walking = self.walkers.len() as u32;
        for logged in &self.log {
            match &logged.event {
                StreetEvent::WalkerSpawned { .. } => s.walkers.spawned += 1,
                StreetEvent::CrossingStarted {
                    crossing, on_red, ..
                } => {
                    s.walkers.crossings += 1;
                    s.walkers.off_crossing += u32::from(crossing.is_none());
                    s.walkers.on_red += u32::from(*on_red);
                }
                StreetEvent::WalkEnded {
                    delay_s,
                    crowded_s,
                    kerb_wait_s,
                    annoyance,
                    ..
                } => {
                    s.walkers.arrived += 1;
                    kerb_wait += kerb_wait_s;
                    walk_delay += delay_s;
                    walk_crowded += crowded_s;
                    s.walkers.total_annoyance += annoyance;
                }
                StreetEvent::GaveUp { agent, .. } if is_walker(*agent) => {
                    s.walkers.gave_up += 1;
                }
                StreetEvent::Bump { .. } => s.walkers.bumps += 1,
                StreetEvent::Spawned { .. } => s.spawned += 1,
                StreetEvent::TripEnded {
                    delay_s, annoyance, ..
                } => {
                    s.arrived += 1;
                    delay += delay_s;
                    s.total_annoyance += annoyance;
                }
                StreetEvent::GaveUp { .. } => s.gave_up += 1,
                StreetEvent::Unfinished { mode, entered, .. } => {
                    let walker = *mode == Mode::Pedestrian;
                    match (walker, *entered) {
                        (true, true) => s.walkers.unfinished += 1,
                        (true, false) => s.walkers.not_entered += 1,
                        (false, true) => s.unfinished += 1,
                        (false, false) => s.not_entered += 1,
                    }
                }
                StreetEvent::Conflict { grade, .. } => match grade {
                    Grade::Light => s.conflicts_light += 1,
                    Grade::Serious => s.conflicts_serious += 1,
                },
                StreetEvent::Contact { b, outcome, .. } if is_walker(*b) => {
                    s.walkers.contacts += 1;
                    match outcome {
                        Outcome::LightInjury => s.walkers.light_injuries += 1,
                        Outcome::SeriousInjury => s.walkers.serious_injuries += 1,
                        Outcome::Death => s.walkers.deaths += 1,
                        Outcome::NoInjury => {}
                    }
                }
                StreetEvent::Contact { outcome, .. } => {
                    s.contacts += 1;
                    match outcome {
                        Outcome::LightInjury => s.light_injuries += 1,
                        Outcome::SeriousInjury => s.serious_injuries += 1,
                        Outcome::Death => s.deaths += 1,
                        Outcome::NoInjury => {}
                    }
                }
                _ => {}
            }
        }
        if s.arrived > 0 {
            s.mean_delay_s = delay / f64::from(s.arrived);
        }
        if s.walkers.arrived > 0 {
            s.walkers.mean_delay_s = walk_delay / f64::from(s.walkers.arrived);
            s.walkers.mean_crowded_s = walk_crowded / f64::from(s.walkers.arrived);
            s.walkers.mean_kerb_wait_s = kerb_wait / f64::from(s.walkers.arrived);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(level: &LevelFile, population: Population, seed: Option<u64>) -> StreetRun {
        let design = Design::new(&level.id);
        let mut options = RunOptions::for_level(level);
        options.population = population;
        if let Some(seed) = seed {
            options.seed = seed;
        }
        let mut run = StreetRun::new(level, &design, options).expect("the level builds");
        run.run_to_end();
        run
    }

    fn level() -> LevelFile {
        LevelFile::load("carrefour-ecole").expect("the level loads")
    }

    #[test]
    fn the_same_options_give_the_same_log_and_another_seed_another() {
        let level = level();
        let a = run(&level, Population::Mixed, None);
        let b = run(&level, Population::Mixed, None);
        let c = run(&level, Population::Mixed, Some(level.seed + 1));
        assert!(!a.events().is_empty());
        assert_eq!(a.log_hash(), b.log_hash());
        assert_ne!(a.log_hash(), c.log_hash());
    }

    #[test]
    fn careful_drivers_all_arrive_without_a_contact() {
        let run = run(&level(), Population::Careful, None);
        let summary = run.summary();
        assert_eq!(summary.ended, Some(EndReason::Completed));
        assert!(summary.spawned > 5, "{summary:?}");
        assert_eq!(summary.arrived, summary.spawned, "{summary:?}");
        assert_eq!(summary.contacts, 0, "{summary:?}");
    }

    #[test]
    fn careful_drivers_also_cope_with_signals() {
        let mut level = level();
        level.junctions[0].control = JunctionControl::Signals;
        let run = run(&level, Population::Careful, None);
        let summary = run.summary();
        assert_eq!(summary.ended, Some(EndReason::Completed));
        assert_eq!(summary.contacts, 0, "{summary:?}");
        assert_eq!(summary.gave_up, 0, "{summary:?}");
        assert!(summary.spawned > 5, "{summary:?}");
    }
}
