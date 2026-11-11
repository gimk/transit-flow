//! `TrafficLights` (`sim/traffic/traffic_lights.gd`): vehicle-actuated traffic lights.
//! Each light detects vehicles near its stop lines and:
//! - rests on green for as long as nobody is waiting on another phase (so a single busy
//!   approach simply keeps its green),
//! - otherwise holds green at least MIN_GREEN, keeps it while its own vehicles keep
//!   moving through, and gives way once its approach is empty, blocked (its vehicles
//!   stand still on green, e.g. because the road beyond is full) or after MAX_GREEN
//!   (shorter at junctions with protected left turns), but not while people who stepped
//!   out on its walk light are still crossing (nor for TURN_WINDOW after),
//! - then shows amber for CLEARANCE_TIME (no new entries) before the next phase that has
//!   someone waiting turns green; phases nobody waits for are skipped.
//!
//! Phases belong to the junction's turns (`Lane::signal_phase` on connectors): the two
//! road axes, plus their left turns when the junction has protected left turns.
//! Pedestrians waiting call the phase they cross with, and may start only early in its
//! green (`is_walk_green`). Light state isn't saved: after loading, lights start over on
//! phase 0.
//!
//! The roads, vehicles and crosswalks are passed into the calls that need them.

use glam::IVec2;
use indexmap::IndexMap;

use crate::events::Outbox;
use crate::ids::{NodeId, SegmentId};
use crate::roads::{ChangeSet, RoadNetwork, RoadNode, lane_builder};
use crate::traffic::VehicleSystem;
use crate::walking::Crosswalks;

/// Godot's values (the snapshot stores them as ints).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Light {
    Green = 0,
    Amber = 1,
    Red = 2,
}

/// Sim seconds.
pub const MIN_GREEN: f64 = 3.0;
pub const MAX_GREEN: f64 = 12.0;
/// Protected left-turn phases are short: a few cars each cycle.
pub const MAX_LEFT_GREEN: f64 = 5.0;
/// At junctions with protected left turns (four phases) the through phases are shorter,
/// so a whole cycle stays about as long as at other lights.
pub const MAX_THROUGH_GREEN_4_PHASES: f64 = 8.0;
/// Green kept after the last pedestrian has left its crosswalks, for the turning traffic
/// they held up.
pub const TURN_WINDOW: f64 = 2.0;
pub const CLEARANCE_TIME: f64 = 1.0;
/// Pedestrians may start crossing only this long into their green (see `is_walk_green`).
pub const WALK_INTERVAL: f64 = 4.0;
/// On a green that rests (nobody waiting on another phase), the walk light comes round
/// again this long after it last came on, if pedestrians are waiting; turning traffic
/// gets the time between.
pub const WALK_RECALL: f64 = 12.0;
/// A vehicle this close to its stop line (px, front-most on its lane) is detected.
pub const DETECTION_DISTANCE: f64 = 60.0;
/// A detected vehicle standing still this long on green counts as blocked, not flowing.
pub const BLOCKED_TIME: f64 = 1.0;

#[derive(Clone, Debug)]
pub struct LightState {
    pub phase: i32,
    pub next_phase: i32,
    pub clearing: bool,
    pub elapsed: f64,
    /// The phases it was set up for; a different set (lights changed) starts over.
    pub phase_count: i32,
    /// When people were last seen on a crosswalk that walks with the current phase.
    pub walkers_seen_at: f64,
    /// The phase that was green before this one (-1: none yet).
    pub last_phase: i32,
}

impl Default for LightState {
    fn default() -> Self {
        Self {
            phase: 0,
            next_phase: 0,
            clearing: false,
            elapsed: 0.0,
            phase_count: 0,
            walkers_seen_at: f64::NEG_INFINITY,
            last_phase: -1,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TrafficLights {
    /// Node id → state, for every junction with traffic lights.
    states: IndexMap<NodeId, LightState>,
}

impl TrafficLights {
    pub fn new(roads: &RoadNetwork) -> Self {
        let mut lights = Self::default();
        let node_ids: Vec<NodeId> = roads.nodes.keys().copied().collect();
        lights.on_roads_changed(
            roads,
            &ChangeSet {
                node_ids,
                segment_ids: Vec::new(),
            },
            &mut Outbox::new(),
        );
        lights
    }

    pub fn state(&self, node_id: NodeId) -> Option<&LightState> {
        self.states.get(&node_id)
    }

    pub fn step(
        &mut self,
        dt: f64,
        roads: &RoadNetwork,
        vehicles: &VehicleSystem,
        crosswalks: &Crosswalks,
    ) {
        let node_ids: Vec<NodeId> = self.states.keys().copied().collect();
        for node_id in node_ids {
            let node = &roads.nodes[&node_id];
            let mut state = self.states[&node_id].clone();
            self.update(node, &mut state, dt, roads, vehicles, crosswalks);
            self.states[&node_id] = state;
        }
    }

    pub fn light(&self, node: &RoadNode, phase: i32) -> Light {
        if node.signal_phase_count <= 1 {
            return Light::Green;
        }
        let Some(state) = self.states.get(&node.id) else {
            return if phase == 0 { Light::Green } else { Light::Red };
        };
        if state.clearing {
            return if phase == state.phase {
                Light::Amber
            } else {
                Light::Red
            };
        }
        if phase == state.phase {
            Light::Green
        } else {
            Light::Red
        }
    }

    /// Did `phase` turn red at `node` less than `within` sim seconds ago (amber
    /// included)?
    pub fn recently_ended(&self, node: &RoadNode, phase: i32, within: f64) -> bool {
        let Some(state) = self.states.get(&node.id) else {
            return false;
        };
        if state.clearing {
            return state.phase == phase;
        }
        state.last_phase == phase && state.elapsed < within
    }

    /// Walk light for the crosswalk across road `segment_id` at `node`: pedestrians cross
    /// alongside the traffic of the other road axis, during the first WALK_INTERVAL of
    /// its through phase's green (then "don't walk" flashes: those on the crosswalk
    /// finish, nobody new starts, so it clears for turning traffic before the green
    /// ends). Always on where the lights can't alternate.
    pub fn is_walk_green(
        &self,
        roads: &RoadNetwork,
        node: &RoadNode,
        segment_id: SegmentId,
    ) -> bool {
        let Some(phase) = Self::walk_phase(roads, node, segment_id) else {
            return true;
        };
        let state = self.states.get(&node.id);
        self.light(node, phase) == Light::Green
            && state.is_none_or(|state| state.elapsed < WALK_INTERVAL)
    }

    /// The light phase that lets pedestrians cross road `segment_id` at `node`, or `None`
    /// if any does (Godot: -1).
    pub fn walk_phase(roads: &RoadNetwork, node: &RoadNode, segment_id: SegmentId) -> Option<i32> {
        if node.signal_phase_count <= 1 {
            return None;
        }
        let segment = roads.segments.get(&segment_id)?;
        let axis = segment
            .lane_ids
            .iter()
            .map(|id| &roads.lanes[id])
            .find(|lane| lane.to_node_id == node.id)
            .map(|lane| lane.signal_phase)
            .unwrap_or_else(|| {
                // A one-way road leaving the junction: its axis from its direction.
                let direction =
                    lane_builder::start_direction(roads.lanes[&segment.lane_ids[0]].curve());
                if direction.x.abs() >= direction.y.abs() {
                    0
                } else {
                    1
                }
            });
        let alongside = 1 - axis;
        Some(if node.signal_phase_count == 4 {
            alongside * 2
        } else {
            alongside
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn update(
        &self,
        node: &RoadNode,
        state: &mut LightState,
        dt: f64,
        roads: &RoadNetwork,
        vehicles: &VehicleSystem,
        crosswalks: &Crosswalks,
    ) {
        state.elapsed += dt;
        if state.clearing {
            if state.elapsed >= CLEARANCE_TIME {
                state.clearing = false;
                state.last_phase = state.phase;
                state.phase = state.next_phase;
                state.elapsed = 0.0;
                state.walkers_seen_at = f64::NEG_INFINITY;
            }
            return;
        }
        let count = node.signal_phase_count;
        if count <= 1 {
            return;
        }
        let demand = Self::detect_demand(node, false, roads, vehicles, crosswalks);
        let waiting_phase = (1..count)
            .map(|offset| (state.phase + offset) % count)
            .find(|&phase| demand[phase as usize]);
        if waiting_phase.is_none()
            && state.elapsed >= WALK_RECALL
            && Self::pedestrian_demand(node, roads, crosswalks)[state.phase as usize]
        {
            // People arrived after the walk light went off, and nobody else waits: walk
            // again (a new green, as far as timing goes).
            state.walkers_seen_at = f64::NEG_INFINITY;
            state.elapsed = 0.0;
            return;
        }
        let Some(waiting_phase) = waiting_phase else {
            return;
        };
        if state.elapsed < MIN_GREEN {
            return;
        }
        // People who stepped out on the walk light finish crossing on this green, and
        // turning drivers get TURN_WINDOW after them (up to the phase's longest green).
        if Self::walkers_on(node, state.phase, roads, crosswalks) {
            state.walkers_seen_at = state.elapsed;
        }
        let max_green = Self::max_green(node, state.phase);
        if state.elapsed < state.walkers_seen_at + TURN_WINDOW && state.elapsed < max_green {
            return;
        }
        if Self::detect_demand(node, true, roads, vehicles, crosswalks)[state.phase as usize]
            && state.elapsed < max_green
        {
            return;
        }
        state.next_phase = waiting_phase;
        state.clearing = true;
        state.elapsed = 0.0;
    }

    /// Per phase: is a vehicle about to take a turn of that phase (the front-most vehicle
    /// on an incoming lane, close to its stop line, by the connector it is heading for)?
    /// With `moving_only`, vehicles standing still for BLOCKED_TIME don't count.
    fn detect_demand(
        node: &RoadNode,
        moving_only: bool,
        roads: &RoadNetwork,
        vehicles: &VehicleSystem,
        crosswalks: &Crosswalks,
    ) -> Vec<bool> {
        // Pedestrians waiting at a crosswalk call its walk phase (they never extend a
        // green).
        let mut demand = if moving_only {
            Self::no_demand(node)
        } else {
            Self::pedestrian_demand(node, roads, crosswalks)
        };
        for segment_id in &node.segment_ids {
            for lane_id in &roads.segments[segment_id].lane_ids {
                let lane = &roads.lanes[lane_id];
                let Some(front_id) = vehicles.queue(lane.id).first() else {
                    continue;
                };
                if lane.to_node_id != node.id {
                    continue;
                }
                let front = &vehicles.vehicles[front_id];
                if moving_only && front.stopped_time >= BLOCKED_TIME {
                    continue;
                }
                let Some(next) = front.next_lanes.first().and_then(|id| roads.lane_any(*id)) else {
                    continue;
                };
                if !next.is_connector() {
                    continue;
                }
                if lane.length - front.distance <= DETECTION_DISTANCE {
                    demand[next.signal_phase as usize] = true;
                }
            }
        }
        demand
    }

    pub fn on_roads_changed(&mut self, roads: &RoadNetwork, change: &ChangeSet, _out: &mut Outbox) {
        for node_id in &change.node_ids {
            match roads.nodes.get(node_id) {
                Some(node) if node.has_lights() => {
                    let stale = self
                        .states
                        .get(node_id)
                        .is_none_or(|state| state.phase_count != node.signal_phase_count);
                    if stale {
                        let state = LightState {
                            phase_count: node.signal_phase_count,
                            ..LightState::default()
                        };
                        self.states.insert(*node_id, state);
                    }
                }
                _ => {
                    self.states.shift_remove(node_id);
                }
            }
        }
    }

    /// Per phase: are pedestrians waiting to cross with it?
    fn pedestrian_demand(
        node: &RoadNode,
        roads: &RoadNetwork,
        crosswalks: &Crosswalks,
    ) -> Vec<bool> {
        let mut demand = Self::no_demand(node);
        for crosswalk in crosswalks.waiting.keys() {
            if crosswalk.x == node.id.0
                && let Some(phase) = Self::walk_phase(roads, node, SegmentId(crosswalk.y))
            {
                demand[phase as usize] = true;
            }
        }
        demand
    }

    fn no_demand(node: &RoadNode) -> Vec<bool> {
        vec![false; node.signal_phase_count.max(0) as usize]
    }

    /// Longest green a phase holds while its traffic keeps coming (shorter for protected
    /// left turns: odd phases where lights have them, see
    /// `RoadNetwork::assign_signal_phases`).
    fn max_green(node: &RoadNode, phase: i32) -> f64 {
        if node.signal_phase_count == 4 {
            return if phase % 2 == 1 {
                MAX_LEFT_GREEN
            } else {
                MAX_THROUGH_GREEN_4_PHASES
            };
        }
        MAX_GREEN
    }

    /// Is anyone on a crosswalk at `node` that walks with `phase`?
    fn walkers_on(
        node: &RoadNode,
        phase: i32,
        roads: &RoadNetwork,
        crosswalks: &Crosswalks,
    ) -> bool {
        crosswalks.walkers.keys().any(|crosswalk: &IVec2| {
            crosswalk.x == node.id.0
                && Self::walk_phase(roads, node, SegmentId(crosswalk.y)) == Some(phase)
        })
    }
}
