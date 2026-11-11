//! `TransitSystem` (`sim/transit/transit_system.gd`): bus stops, bus lines, the buses
//! running them and the people riding them.
//! - Stops sit at the curb of a road, on its outermost lane (`BusStop`); a line loops
//!   through its stops in order (`BusLine`).
//! - Each line runs enough buses for about `TARGET_HEADWAY` between them, spawned one by
//!   one at its stops. Buses are vehicles driven by `VehicleSystem` (`Vehicle::in_service`):
//!   from stop to stop. Stops are request stops: where someone gets off or waits for its
//!   line, the bus dwells (in its lane, holding up the traffic behind, or in the stop's bay
//!   off it) while riders get off and on (`DWELL_BASE`, plus `BOARD_TIME` per rider);
//!   elsewhere it drives past.
//! - Riders (citizens on a bus leg, `wait_for_bus`) wait at their stop for a bus of their
//!   line, ride to their stop and get off (`RiderArrived`). If their stop or line goes, or
//!   no bus comes for too long, they give up (`RiderStranded`).
//! - Trip planning (`plan_ride`): the cheapest way by bus between two places, by
//!   generalized cost: walk to a stop, wait (half the headway, counted `WAIT_WEIGHT`
//!   times), ride (the times buses really take), change lines at the same stop or one a
//!   short walk away (a penalty each, at most `MAX_TRANSFERS`), walk from the last stop.
//!
//! The GDScript class holds the roads, router, vehicles, pedestrians and the shared
//! crosswalks; here they come in a `TransitCtx` built by `Simulation::transit_ctx`.
//! Riders live in one map (`riders`, by citizen id); stops and buses list citizen ids.

use std::collections::{HashMap, HashSet};

use glam::{IVec2, Vec2};
use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::events::{Outbox, SimEvent};
use crate::geom::GdVec2;
use crate::ids::{CitizenId, LaneId, LineId, StopId, VehicleId};
use crate::roads::{Lane, RoadNetwork, lane_builder};
use crate::routing::{Route, Router};
use crate::traffic::vehicle::{BUS_LENGTH, Kind, Vehicle};
use crate::traffic::vehicle_system::{DECELERATION, MIN_GAP, VehicleSystem};
use crate::transit::bus_line::{BusLine, Color};
use crate::transit::bus_stop::BusStop;
use crate::walking::PedestrianSystem;
use crate::walking::crosswalks::{Crosswalks, MID_BLOCK_BASE, MidBlockCrossing};

/// What the transit system works with besides its own state (the GDScript class holds
/// references to these).
pub struct TransitCtx<'a> {
    pub roads: &'a RoadNetwork,
    pub router: &'a mut Router,
    pub vehicles: &'a mut VehicleSystem,
    pub pedestrians: &'a mut PedestrianSystem,
    pub crosswalks: &'a mut Crosswalks,
}

/// `TransitSystem.Rider`: a citizen on a bus leg, waiting at a stop or riding a bus.
#[derive(Clone, Debug, PartialEq)]
pub struct Rider {
    pub citizen: CitizenId,
    pub line_id: LineId,
    pub to_stop_id: StopId,
    /// When they started waiting.
    pub since: f64,
    /// The stop they wait at, or the bus (vehicle id) they ride (`None` when not).
    pub stop_id: Option<StopId>,
    pub bus_id: Option<VehicleId>,
}

/// `TransitSystem.Bus`: a bus of a line, and the vehicle driving it.
#[derive(Clone, Debug, PartialEq)]
pub struct Bus {
    pub vehicle: VehicleId,
    pub line_id: LineId,
    /// Index, in the line's stops, of the stop it is heading to or dwelling at, and of
    /// the last stop it was at (`None`: none yet).
    pub stop_index: usize,
    pub last_index: Option<usize>,
    /// Citizen ids of the riders aboard.
    pub riders: Vec<CitizenId>,
    pub dwell_until: f64,
    /// When it arrived at its last stop.
    pub arrived_at: f64,
}

/// Where a stop for a point goes (the GDScript dictionary `{ "lane_id", "distance",
/// "position", "direction" }`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurbSpot {
    pub lane_id: LaneId,
    pub distance: f64,
    pub position: Vec2,
    pub direction: Vec2,
}

/// One leg of a planned bus trip (`Citizen.legs` in GDScript: dictionaries with a mode).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RideLeg {
    /// `Citizen.Mode.WALK`.
    Walk { from: Vec2, to: Vec2 },
    /// `Citizen.Mode.BUS`: ride `line` from stop `from` to stop `to`.
    Bus {
        line: LineId,
        from: StopId,
        to: StopId,
    },
}

/// What `plan_ride` returns: `{ "mode": BUS, "legs", "cost" }`.
#[derive(Clone, Debug, PartialEq)]
pub struct RidePlan {
    pub legs: Vec<RideLeg>,
    /// Generalized cost.
    pub cost: f64,
}

#[derive(Clone, Debug)]
pub struct TransitSystem {
    pub stops: IndexMap<StopId, BusStop>,
    pub lines: IndexMap<LineId, BusLine>,
    /// Vehicle id → Bus.
    pub buses: IndexMap<VehicleId, Bus>,
    time: f64,
    next_stop_id: i32,
    next_line_id: i32,
    /// Stops need finding on the roads again, and lines timing again (roads changed).
    dirty: bool,
    /// Citizen id → their ride.
    riders: IndexMap<CitizenId, Rider>,
    /// Line id → when its last bus was spawned, and how many spawns it has tried.
    last_spawn: IndexMap<LineId, f64>,
    spawn_attempts: IndexMap<LineId, i64>,
    /// Stop id → stops a short walk away (`stops_near_stop`).
    nearby_stops: IndexMap<StopId, IndexMap<StopId, f64>>,
}

impl Default for TransitSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl TransitSystem {
    pub const OWNER_ID: i32 = -4;
    pub const CAPACITY: usize = 40;
    /// Dwell at a stop: opening and closing the doors, plus per rider getting off or on.
    pub const DWELL_BASE: f64 = 1.0;
    pub const BOARD_TIME: f64 = 0.2;
    /// How far behind the bus's front the door is (px).
    pub const DOOR_SETBACK: f64 = 3.0;
    /// Buses per line: one for each `TARGET_HEADWAY` of a trip round the line, up to a cap.
    pub const TARGET_HEADWAY: f64 = 10.0;
    pub const MAX_BUSES_PER_LINE: i32 = 12;
    /// New buses join a line at most this often (sim seconds), at different stops.
    pub const SPAWN_INTERVAL: f64 = 1.0;
    /// A rider gives up after waiting this many headways (and at least `WAIT_GIVE_UP_MIN`).
    pub const WAIT_GIVE_UP_HEADWAYS: f64 = 3.0;
    pub const WAIT_GIVE_UP_MIN: f64 = 30.0;
    /// How fast measured times between stops replace the estimate (per bus run).
    pub const MEASURE_BLEND: f64 = 0.3;
    /// A bus decides to drive past a stop nobody needs within this much more than its
    /// braking distance of it.
    pub const SKIP_DECISION_MARGIN: f64 = 20.0;
    /// Riders' costs: waiting counts double (it feels longer than riding), and every
    /// change of bus costs extra.
    pub const WAIT_WEIGHT: f64 = 2.0;
    pub const TRANSFER_PENALTY: f64 = 5.0;
    pub const MAX_TRANSFERS: i32 = 2;
    /// Placing stops: how far from a road a click may be, and how close to an existing
    /// stop counts as that stop. After a road edit, a stop stays on a lane in its
    /// direction within `RESOLVE_DISTANCE` (moving out to the curb if the road got wider).
    pub const SNAP_DISTANCE: f64 = 24.0;
    pub const MERGE_DISTANCE: f64 = 20.0;
    pub const RESOLVE_DISTANCE: f64 = 24.0;
    /// Stop crosswalks: how far behind the bus they are, and how close two may be
    /// (closer ones, at stops facing each other, are one).
    pub const CROSSING_SETBACK: f64 = 6.0;
    pub const CROSSING_MERGE_DISTANCE: f64 = 48.0;

    pub fn new() -> Self {
        Self {
            stops: IndexMap::new(),
            lines: IndexMap::new(),
            buses: IndexMap::new(),
            time: 0.0,
            next_stop_id: 1,
            next_line_id: 1,
            dirty: false,
            riders: IndexMap::new(),
            last_spawn: IndexMap::new(),
            spawn_attempts: IndexMap::new(),
            nearby_stops: IndexMap::new(),
        }
    }

    /// Line colours, in turn: blue, pink, purple, yellow (none green or brown like cars
    /// and trucks): `Color("1e88e5")` and so on.
    pub fn line_color(line_id: LineId) -> Color {
        const COLORS: [(u8, u8, u8); 4] = [
            (0x1e, 0x88, 0xe5),
            (0xec, 0x40, 0x7a),
            (0x8e, 0x24, 0xaa),
            (0xfd, 0xd8, 0x35),
        ];
        let (r, g, b) = COLORS[(line_id.0 - 1).rem_euclid(4) as usize];
        [
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            1.0,
        ]
    }

    // --- Stops and lines ------------------------------------------------------------

    /// Adds a stop at the curb of the road nearest `point`, on the side `point` is on, or
    /// returns the stop already there (same side, within `MERGE_DISTANCE`). `None` if
    /// there's no road near, or it's too short. A new stop is a bus bay if `bay` (an
    /// existing one is left as it is: see `set_stop_bay`).
    pub fn add_stop(&mut self, ctx: &mut TransitCtx, point: Vec2, bay: bool) -> Option<StopId> {
        let spot = Self::find_curb_spot(ctx.roads, point)?;
        for stop in self.stops.values() {
            if f64::from(stop.position.gd_distance_to(spot.position)) <= Self::MERGE_DISTANCE
                && f64::from(stop.direction.dot(spot.direction)) > 0.9
            {
                return Some(stop.id);
            }
        }
        let mut stop = BusStop::new(StopId(self.next_stop_id), spot.position, spot.direction);
        stop.bay = bay;
        self.next_stop_id += 1;
        stop.lane_id = Some(spot.lane_id);
        stop.distance = spot.distance;
        let id = stop.id;
        self.stops.insert(id, stop);
        self.update_crossings(ctx);
        Some(id)
    }

    /// Makes a stop a bus bay, or a curbside stop.
    pub fn set_stop_bay(&mut self, stop_id: StopId, bay: bool) {
        if let Some(stop) = self.stops.get_mut(&stop_id) {
            stop.bay = bay;
        }
    }

    /// The stop nearest `point` within `radius` (`MERGE_DISTANCE` by default in Godot).
    pub fn find_stop(&self, point: Vec2, radius: f64) -> Option<StopId> {
        let mut best = None;
        let mut best_distance = radius;
        for stop in self.stops.values() {
            let distance = f64::from(stop.position.gd_distance_to(point));
            if distance <= best_distance {
                best = Some(stop.id);
                best_distance = distance;
            }
        }
        best
    }

    /// Where a stop for `point` goes: at the curb of the nearest road on point's side,
    /// where a whole bus fits; `None` if none. Static, so tools can preview it from road
    /// geometry alone.
    pub fn find_curb_spot(roads: &RoadNetwork, point: Vec2) -> Option<CurbSpot> {
        let near = roads.find_lane_position(point, Self::SNAP_DISTANCE, 0)?;
        let lane = Self::outermost(roads, &roads.lanes[&near.lane_id]);
        Self::spot_on(lane, f64::from(lane.curve().closest_offset(point)))
    }

    pub fn remove_stop(&mut self, ctx: &mut TransitCtx, stop_id: StopId, out: &mut Outbox) {
        let Some(stop) = self.stops.get(&stop_id) else {
            return;
        };
        for citizen in stop.waiting.clone() {
            self.strand(citizen, out);
        }
        self.stops.shift_remove(&stop_id);
        self.update_crossings(ctx);
        let line_ids: Vec<LineId> = self.lines.keys().copied().collect();
        for line_id in line_ids {
            let line = &mut self.lines[&line_id];
            let Some(at) = line.stop_ids.iter().position(|&id| id == stop_id) else {
                continue;
            };
            line.stop_ids.remove(at);
            let stop_count = line.stop_ids.len();
            if stop_count < 2 {
                self.remove_line(ctx, line_id, out);
                continue;
            }
            let bus_ids: Vec<VehicleId> = self.buses.keys().copied().collect();
            for bus_id in bus_ids {
                if self.buses[&bus_id].line_id != line_id {
                    continue;
                }
                for citizen in self.buses[&bus_id].riders.clone() {
                    if self.riders[&citizen].to_stop_id == stop_id {
                        self.strand(citizen, out);
                    }
                }
                let bus = &mut self.buses[&bus_id];
                bus.stop_index = bus.stop_index.min(stop_count - 1);
                bus.last_index = None;
            }
            self.update_line(ctx, line_id);
        }
    }

    /// Adds a line through `stop_ids` (in order; it loops back to the first). Returns its
    /// id, or `None` if it doesn't have two different existing stops.
    pub fn add_line(&mut self, ctx: &mut TransitCtx, stop_ids: &[StopId]) -> Option<LineId> {
        let mut ids: Vec<StopId> = Vec::new();
        for &stop_id in stop_ids {
            if self.stops.contains_key(&stop_id) && !ids.contains(&stop_id) {
                ids.push(stop_id);
            }
        }
        if ids.len() < 2 {
            return None;
        }
        let mut line = BusLine::new(LineId(self.next_line_id), ids);
        self.next_line_id += 1;
        line.color = Self::line_color(line.id);
        let id = line.id;
        self.lines.insert(id, line);
        self.nearby_stops.clear();
        self.update_line(ctx, id);
        let line = &mut self.lines[&id];
        line.bus_count = if line.loop_time() < f64::INFINITY {
            ((line.loop_time() / Self::TARGET_HEADWAY).ceil() as i64)
                .clamp(1, i64::from(Self::MAX_BUSES_PER_LINE)) as i32
        } else {
            1
        };
        Some(id)
    }

    pub fn remove_line(&mut self, ctx: &mut TransitCtx, line_id: LineId, out: &mut Outbox) {
        if !self.lines.contains_key(&line_id) {
            return;
        }
        let bus_ids: Vec<VehicleId> = self.buses.keys().copied().collect();
        for bus_id in bus_ids {
            if self.buses[&bus_id].line_id == line_id {
                self.retire(ctx, bus_id, out);
            }
        }
        let stop_ids: Vec<StopId> = self.stops.keys().copied().collect();
        for stop_id in stop_ids {
            for citizen in self.stops[&stop_id].waiting.clone() {
                if self.riders[&citizen].line_id == line_id {
                    self.strand(citizen, out);
                }
            }
        }
        self.lines.shift_remove(&line_id);
        self.nearby_stops.clear();
        self.last_spawn.shift_remove(&line_id);
    }

    /// Ids of the lines calling at a stop.
    pub fn lines_at(&self, stop_id: StopId) -> Vec<LineId> {
        self.lines
            .values()
            .filter(|line| line.stop_ids.contains(&stop_id))
            .map(|line| line.id)
            .collect()
    }

    /// The colour of the line a bus runs on (white if it's not a bus of ours).
    pub fn bus_color(&self, vehicle_id: VehicleId) -> Color {
        let line = self
            .buses
            .get(&vehicle_id)
            .and_then(|bus| self.lines.get(&bus.line_id));
        line.map_or(BusLine::WHITE, |line| line.color)
    }

    /// The curb side of a bus's front door (riders step off and on here).
    pub fn door_position(vehicle: &Vehicle) -> Vec2 {
        let heading = Vec2::from_angle(vehicle.rotation as f32);
        let right = Vec2::new(-heading.y, heading.x);
        vehicle.position
            + heading * (vehicle.length * 0.5 - Self::DOOR_SETBACK) as f32
            + right * (vehicle.width * 0.5 + 1.0) as f32
    }

    // --- Riders ---------------------------------------------------------------------

    /// A citizen at `from_stop_id` waits for a bus of `line_id` to ride to `to_stop_id`.
    /// Returns false if that isn't possible (the stop or line is gone).
    pub fn wait_for_bus(
        &mut self,
        citizen: CitizenId,
        line_id: LineId,
        from_stop_id: StopId,
        to_stop_id: StopId,
    ) -> bool {
        let (Some(stop), Some(line)) =
            (self.stops.get_mut(&from_stop_id), self.lines.get(&line_id))
        else {
            return false;
        };
        if !stop.is_active() || !line.stop_ids.contains(&to_stop_id) {
            return false;
        }
        stop.waiting.push(citizen);
        self.riders.insert(
            citizen,
            Rider {
                citizen,
                line_id,
                to_stop_id,
                since: self.time,
                stop_id: Some(from_stop_id),
                bus_id: None,
            },
        );
        true
    }

    /// Takes a citizen off the network without reporting anything (e.g. their home is
    /// gone).
    pub fn remove_rider(&mut self, citizen: CitizenId) {
        self.detach_rider(citizen);
    }

    /// Free-flow riding time on `line_id` from one of its stops to another.
    pub fn ride_ideal(&self, line_id: LineId, from_stop_id: StopId, to_stop_id: StopId) -> f64 {
        let Some(line) = self.lines.get(&line_id) else {
            return 0.0;
        };
        let mut i = line.stop_ids.iter().position(|&id| id == from_stop_id);
        let mut time = 0.0;
        for _ in 0..line.stop_ids.len() {
            let Some(at) = i else { break };
            if line.stop_ids[at] == to_stop_id {
                break;
            }
            let free_flow = f64::from(line.free_flow_times[at]);
            time += if free_flow < f64::INFINITY {
                free_flow
            } else {
                0.0
            };
            i = Some(line.next_index(at));
        }
        time
    }

    /// The cheapest way by bus from `from` to `to` (world positions, e.g. lot centers):
    /// walk, bus..., walk legs and the generalized cost, or `None` if there's none. Stops
    /// count if they are at most `max_walk` px away on foot; walking costs `walk_weight`
    /// times its duration. (May strand riders of stops that lost their road: it makes
    /// sure stops are on the roads first, so it takes `out`.)
    pub fn plan_ride(
        &mut self,
        ctx: &mut TransitCtx,
        from: Vec2,
        to: Vec2,
        max_walk: f64,
        walk_weight: f64,
        out: &mut Outbox,
    ) -> Option<RidePlan> {
        self.ensure_resolved(ctx, out);
        if self.lines.is_empty() {
            return None;
        }
        let access = self.stops_in_reach(ctx, from, max_walk);
        if access.is_empty() {
            return None;
        }
        let egress = self.stops_in_reach(ctx, to, max_walk);
        if egress.is_empty() {
            return None;
        }
        // Dijkstra over "aboard a bus of line y at stop x".
        let mut search = RideSearch::default();
        for (&stop_id, &walk) in &access {
            for line_id in self.lines_at(stop_id) {
                let wait = self.lines[&line_id].headway() * 0.5 * Self::WAIT_WEIGHT;
                search.relax(
                    (stop_id, line_id),
                    Self::walk_time(walk) * walk_weight + wait,
                    0,
                    0,
                    None,
                );
            }
        }
        let mut best_total = f64::INFINITY;
        let mut best_key: Option<RideKey> = None;
        let mut settled: HashSet<RideKey> = HashSet::new();
        while !search.open.is_empty() {
            let mut index = 0;
            for i in 1..search.open.len() {
                if search.cost[&search.open[i]] < search.cost[&search.open[index]] {
                    index = i;
                }
            }
            let key = search.open.remove(index);
            if settled.contains(&key) {
                continue;
            }
            settled.insert(key);
            let here = search.cost[&key];
            if here >= best_total {
                break;
            }
            let line = &self.lines[&key.1];
            if egress.contains_key(&key.0) && search.ridden[&key] > 0 {
                let total = here + Self::walk_time(egress[&key.0]) * walk_weight;
                if total < best_total {
                    best_total = total;
                    best_key = Some(key);
                }
            }
            let i = line
                .stop_ids
                .iter()
                .position(|&id| id == key.0)
                .expect("a state's stop is on its line");
            let next_stop = &self.stops[&line.stop_ids[line.next_index(i)]];
            if f64::from(line.segment_times[i]) < f64::INFINITY && next_stop.is_active() {
                search.relax(
                    (next_stop.id, line.id),
                    here + f64::from(line.segment_times[i]),
                    search.transfers[&key],
                    search.ridden[&key] + 1,
                    Some(key),
                );
            }
            if search.ridden[&key] > 0 && search.transfers[&key] < Self::MAX_TRANSFERS {
                // Change lines here, or at a stop a short walk away.
                let line_id = line.id;
                let nearby = self.stops_near_stop(ctx, key.0, max_walk);
                for (&stop_id, &walk) in &nearby {
                    let walk_cost = if stop_id != key.0 {
                        Self::walk_time(walk) * walk_weight
                    } else {
                        0.0
                    };
                    for other_id in self.lines_at(stop_id) {
                        if other_id != line_id {
                            let wait = self.lines[&other_id].headway() * 0.5 * Self::WAIT_WEIGHT;
                            search.relax(
                                (stop_id, other_id),
                                here + walk_cost + Self::TRANSFER_PENALTY + wait,
                                search.transfers[&key] + 1,
                                search.ridden[&key],
                                Some(key),
                            );
                        }
                    }
                }
            }
        }
        let best_key = best_key?;
        // Back from the last stop: consecutive states on the same line make one bus leg;
        // a change of line at another stop walks there.
        let mut chain: Vec<RideKey> = Vec::new();
        let mut key = Some(best_key);
        while let Some(k) = key {
            chain.push(k);
            key = search.parent[&k];
        }
        chain.reverse();
        let mut legs = vec![RideLeg::Walk {
            from,
            to: self.stops[&chain[0].0].position,
        }];
        for state in &chain {
            match *legs.last().expect("a first leg") {
                RideLeg::Bus { line, .. } if line == state.1 => {
                    if let Some(RideLeg::Bus { to, .. }) = legs.last_mut() {
                        *to = state.0;
                    }
                    continue;
                }
                RideLeg::Bus { to, .. } if to != state.0 => {
                    legs.push(RideLeg::Walk {
                        from: self.stops[&to].position,
                        to: self.stops[&state.0].position,
                    });
                }
                _ => {}
            }
            legs.push(RideLeg::Bus {
                line: state.1,
                from: state.0,
                to: state.0,
            });
        }
        legs.push(RideLeg::Walk {
            from: self.stops[&chain[chain.len() - 1].0].position,
            to,
        });
        Some(RidePlan {
            legs,
            cost: best_total,
        })
    }

    // --- Each tick ------------------------------------------------------------------

    pub fn step(&mut self, time: f64, ctx: &mut TransitCtx, out: &mut Outbox) {
        self.time = time;
        self.ensure_resolved(ctx, out);
        let line_ids: Vec<LineId> = self.lines.keys().copied().collect();
        for line_id in line_ids {
            self.spawn_bus(ctx, line_id);
        }
        let bus_ids: Vec<VehicleId> = self.buses.keys().copied().collect();
        for bus_id in bus_ids {
            if !ctx.vehicles.vehicles[&bus_id].dwelling {
                self.skip_if_nobody_there(ctx, bus_id);
                continue;
            }
            // Riders who reached the stop while it waits get on too.
            self.board(bus_id);
            if time >= self.buses[&bus_id].dwell_until {
                self.send_on(ctx, bus_id);
            }
        }
        self.give_up_waiting(out);
    }

    /// Saved: stops and lines (buses and riders restart).
    pub fn to_dict(&self) -> Value {
        let stops: Vec<Value> = self
            .stops
            .values()
            .map(|stop| {
                json!({
                    "id": stop.id.0,
                    "position": [stop.position.x, stop.position.y],
                    "direction": [stop.direction.x, stop.direction.y],
                    "bay": stop.bay,
                })
            })
            .collect();
        let lines: Vec<Value> = self
            .lines
            .values()
            .map(|line| {
                let ids: Vec<i32> = line.stop_ids.iter().map(|id| id.0).collect();
                json!({ "id": line.id.0, "stops": ids, "buses": line.bus_count })
            })
            .collect();
        json!({
            "stops": stops,
            "lines": lines,
            "next_stop_id": self.next_stop_id,
            "next_line_id": self.next_line_id,
        })
    }

    pub fn restore(&mut self, ctx: &mut TransitCtx, data: &Value) {
        let int = |value: &Value| value.as_f64().unwrap_or(0.0) as i32;
        let float = |value: &Value| value.as_f64().unwrap_or(0.0) as f32;
        for entry in data["stops"].as_array().into_iter().flatten() {
            let mut stop = BusStop::new(
                StopId(int(&entry["id"])),
                Vec2::new(float(&entry["position"][0]), float(&entry["position"][1])),
                Vec2::new(float(&entry["direction"][0]), float(&entry["direction"][1])),
            );
            stop.bay = entry["bay"].as_bool().unwrap_or(false);
            let id = stop.id;
            self.stops.insert(id, stop);
            self.nearby_stops.clear();
            Self::resolve(&mut self.stops[&id], ctx.roads);
        }
        for entry in data["lines"].as_array().into_iter().flatten() {
            let ids: Vec<StopId> = entry["stops"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|id| StopId(int(id)))
                .collect();
            let mut line = BusLine::new(LineId(int(&entry["id"])), ids);
            line.color = Self::line_color(line.id);
            line.bus_count = entry.get("buses").map_or(1, int);
            let id = line.id;
            self.lines.insert(id, line);
            self.update_line(ctx, id);
        }
        self.next_stop_id = data
            .get("next_stop_id")
            .map_or(self.stops.len() as i32 + 1, int);
        self.next_line_id = data
            .get("next_line_id")
            .map_or(self.lines.len() as i32 + 1, int);
        self.update_crossings(ctx);
    }

    pub fn dispose(&mut self) {
        self.buses.clear();
        self.riders.clear();
        for stop in self.stops.values_mut() {
            stop.waiting.clear();
        }
    }

    // --- Buses ----------------------------------------------------------------------

    /// Puts the line's next bus on the road at one of its stops, if it is short of buses
    /// (one every `SPAWN_INTERVAL`, at a different stop each time, where there's room).
    fn spawn_bus(&mut self, ctx: &mut TransitCtx, line_id: LineId) {
        let last = self
            .last_spawn
            .get(&line_id)
            .copied()
            .unwrap_or(f64::NEG_INFINITY);
        if self.time - last < Self::SPAWN_INTERVAL {
            return;
        }
        let line = &self.lines[&line_id];
        let running = self
            .buses
            .values()
            .filter(|bus| bus.line_id == line_id)
            .count() as i32;
        if running >= line.bus_count {
            return;
        }
        let attempt = self.spawn_attempts.get(&line_id).copied().unwrap_or(0);
        self.spawn_attempts.insert(line_id, attempt + 1);
        let n = line.stop_ids.len() as i64;
        let index = ((attempt * (n / i64::from(line.bus_count.max(1))).max(1)) % n) as usize;
        let stop = &self.stops[&line.stop_ids[index]];
        let next = self.next_reachable(ctx, line, index, stop.lane_id, stop.distance);
        let Some((next_index, route)) = next else {
            return;
        };
        if !stop.is_active() {
            return;
        }
        let Some(vehicle_id) =
            ctx.vehicles
                .spawn(ctx.roads, route, None, Self::OWNER_ID, Kind::Bus)
        else {
            return; // No room at that stop right now: another one next time.
        };
        let vehicle = &mut ctx.vehicles.vehicles[&vehicle_id];
        vehicle.in_service = true;
        vehicle.scored_by_owner = true; // Flow scores the riders' trips, not the bus runs.
        self.last_spawn.insert(line_id, self.time);
        self.buses.insert(
            vehicle_id,
            Bus {
                vehicle: vehicle_id,
                line_id,
                stop_index: next_index,
                last_index: Some(index),
                riders: Vec::new(),
                dwell_until: 0.0,
                arrived_at: self.time,
            },
        );
    }

    /// The next stop after index `index` that a bus at (`lane_id`, `distance`) can reach:
    /// its index and the route there, or `None`. Buses keep to their line's way
    /// (`BusLine::routes`) from the stop they are at; they only look for another way
    /// round a stop that's gone or after a road edit moved them off it.
    fn next_reachable(
        &self,
        ctx: &mut TransitCtx,
        line: &BusLine,
        index: usize,
        lane_id: Option<LaneId>,
        distance: f64,
    ) -> Option<(usize, Route)> {
        // No lane: Godot's -1 matches no route and no router lane.
        let lane_id = lane_id?;
        let mut next = line.next_index(index);
        if let Some(Some(own)) = line.routes.get(index)
            && own.lane_ids[0] == lane_id
            && self.stops[&line.stop_ids[next]].is_active()
        {
            return Some((
                next,
                Route::new(
                    own.lane_ids.clone(),
                    distance,
                    own.end_distance,
                    own.travel_time,
                ),
            ));
        }
        next = index;
        for _ in 0..line.stop_ids.len() - 1 {
            next = line.next_index(next);
            let stop = &self.stops[&line.stop_ids[next]];
            let Some(stop_lane) = stop.lane_id else {
                continue;
            };
            if let Some(route) =
                ctx.router
                    .find_route(ctx.roads, lane_id, distance, stop_lane, stop.distance, true)
            {
                return Some((next, route));
            }
        }
        None
    }

    /// Stops are request stops: a bus coming up to one where nobody gets off and nobody
    /// waits for its line drives on to the next without stopping (decided once it is on
    /// the stop's lane, about to brake for it).
    fn skip_if_nobody_there(&mut self, ctx: &mut TransitCtx, bus_id: VehicleId) {
        let vehicle = &ctx.vehicles.vehicles[&bus_id];
        if !vehicle.is_on_last_lane() {
            return;
        }
        let lane_id = vehicle.lane.expect("on the road");
        let Some(lane) = ctx.roads.lanes.get(&lane_id) else {
            return;
        };
        let to_stop = vehicle.remaining_on_lane(lane);
        if to_stop
            > vehicle.speed * vehicle.speed / (2.0 * DECELERATION) + Self::SKIP_DECISION_MARGIN
        {
            return;
        }
        let distance = vehicle.distance;
        let bus = &self.buses[&bus_id];
        let line = &self.lines[&bus.line_id];
        let Some(stop) = self.stops.get(&line.stop_ids[bus.stop_index]) else {
            return;
        };
        if self.has_business(bus, stop) {
            return;
        }
        let Some((next_index, route)) =
            self.next_reachable(ctx, line, bus.stop_index, Some(lane_id), distance)
        else {
            return;
        };
        // Passing the stop counts as calling at it, for the timings.
        let line_id = bus.line_id;
        self.record_arrival(bus_id, line_id);
        self.buses[&bus_id].stop_index = next_index;
        ctx.vehicles.set_route(ctx.roads, bus_id, &route);
    }

    /// Does anyone get off the bus at `stop`, or wait there for its line?
    fn has_business(&self, bus: &Bus, stop: &BusStop) -> bool {
        bus.riders
            .iter()
            .any(|citizen| self.riders[citizen].to_stop_id == stop.id)
            || stop
                .waiting
                .iter()
                .any(|citizen| self.riders[citizen].line_id == bus.line_id)
    }

    /// The bus reached (or passed) its stop: times the run from the stop before.
    fn record_arrival(&mut self, bus_id: VehicleId, line_id: LineId) {
        let time = self.time;
        let bus = &mut self.buses[&bus_id];
        let line = &mut self.lines[&line_id];
        if let Some(last) = bus.last_index
            && line.next_index(last) == bus.stop_index
        {
            let measured = time - bus.arrived_at;
            let old = f64::from(line.segment_times[last]);
            line.segment_times[last] = if old < f64::INFINITY {
                old + (measured - old) * Self::MEASURE_BLEND
            } else {
                measured
            } as f32;
        }
        bus.last_index = Some(bus.stop_index);
        bus.arrived_at = time;
    }

    /// A dwelling bus whose time is up drives on to its next stop.
    fn send_on(&mut self, ctx: &mut TransitCtx, bus_id: VehicleId) {
        let vehicle = &ctx.vehicles.vehicles[&bus_id];
        let lane_id = vehicle.lane.expect("on the road");
        if !ctx.roads.lanes.contains_key(&lane_id) {
            return; // Its road was just rebuilt: VehicleSystem moves it onto the new lanes first.
        }
        let distance = vehicle.distance;
        let bus = &self.buses[&bus_id];
        let line = &self.lines[&bus.line_id];
        let Some((next_index, route)) =
            self.next_reachable(ctx, line, bus.stop_index, Some(lane_id), distance)
        else {
            return; // Nowhere to go for now: it waits where it is.
        };
        self.buses[&bus_id].stop_index = next_index;
        ctx.vehicles.set_route(ctx.roads, bus_id, &route);
    }

    /// `VehicleSystem.vehicle_arrived`: a bus reached its stop.
    pub fn on_vehicle_arrived(
        &mut self,
        vehicle_id: VehicleId,
        ctx: &mut TransitCtx,
        out: &mut Outbox,
    ) {
        let Some(vehicle) = ctx.vehicles.vehicle_any(vehicle_id) else {
            return;
        };
        if vehicle.owner_id != Self::OWNER_ID || !self.buses.contains_key(&vehicle_id) {
            return;
        }
        let line_id = self.buses[&vehicle_id].line_id;
        self.record_arrival(vehicle_id, line_id);
        let bus = &self.buses[&vehicle_id];
        let stop_id = self.lines[&line_id].stop_ids[bus.stop_index];
        let alighting: Vec<CitizenId> = bus
            .riders
            .iter()
            .copied()
            .filter(|citizen| self.riders[citizen].to_stop_id == stop_id)
            .collect();
        self.buses[&vehicle_id].dwell_until =
            self.time + Self::DWELL_BASE + Self::BOARD_TIME * alighting.len() as f64;
        if self.stops[&stop_id].bay {
            ctx.vehicles.pull_into_bay(ctx.roads, vehicle_id);
        }
        let door = Self::door_position(ctx.vehicles.vehicle_any(vehicle_id).expect("a bus"));
        for citizen in alighting {
            self.detach_rider(citizen);
            out.push(SimEvent::RiderArrived { citizen, door });
        }
        self.board(vehicle_id);
    }

    /// Riders waiting for the bus's line at its stop get on, while there's room.
    fn board(&mut self, bus_id: VehicleId) {
        let bus = &self.buses[&bus_id];
        let Some(line) = self.lines.get(&bus.line_id) else {
            return;
        };
        let stop_id = line.stop_ids[bus.stop_index];
        let Some(stop) = self.stops.get_mut(&stop_id) else {
            return;
        };
        if stop.waiting.is_empty() {
            return;
        }
        for citizen in stop.waiting.clone() {
            let bus = &mut self.buses[&bus_id];
            if bus.riders.len() >= Self::CAPACITY {
                return;
            }
            let rider = &mut self.riders[&citizen];
            if rider.line_id != bus.line_id {
                continue;
            }
            if let Some(at) = stop.waiting.iter().position(|&c| c == citizen) {
                stop.waiting.remove(at);
            }
            rider.stop_id = None;
            rider.bus_id = Some(bus_id);
            bus.riders.push(citizen);
            stop.boarded += 1;
            bus.dwell_until += Self::BOARD_TIME;
        }
    }

    /// `VehicleSystem.vehicle_failed`: a bus that failed strands its riders.
    pub fn on_vehicle_failed(
        &mut self,
        vehicle_id: VehicleId,
        ctx: &mut TransitCtx,
        out: &mut Outbox,
    ) {
        let Some(vehicle) = ctx.vehicles.vehicle_any(vehicle_id) else {
            return;
        };
        if vehicle.owner_id != Self::OWNER_ID || !self.buses.contains_key(&vehicle_id) {
            return;
        }
        for citizen in self.buses[&vehicle_id].riders.clone() {
            self.strand(citizen, out);
        }
        self.buses.shift_remove(&vehicle_id);
    }

    /// Takes a bus off the road; its riders are stranded.
    fn retire(&mut self, ctx: &mut TransitCtx, bus_id: VehicleId, out: &mut Outbox) {
        for citizen in self.buses[&bus_id].riders.clone() {
            self.strand(citizen, out);
        }
        self.buses.shift_remove(&bus_id);
        ctx.vehicles.remove(ctx.roads, bus_id);
    }

    fn give_up_waiting(&mut self, out: &mut Outbox) {
        let stop_ids: Vec<StopId> = self.stops.keys().copied().collect();
        for stop_id in stop_ids {
            for citizen in self.stops[&stop_id].waiting.clone() {
                let rider = &self.riders[&citizen];
                let line = self.lines.get(&rider.line_id);
                let mut patience = Self::WAIT_GIVE_UP_MIN;
                if let Some(line) = line
                    && line.headway() < f64::INFINITY
                {
                    patience = patience.max(line.headway() * Self::WAIT_GIVE_UP_HEADWAYS);
                }
                if line.is_none() || self.time - rider.since > patience {
                    self.strand(citizen, out);
                }
            }
        }
    }

    fn strand(&mut self, citizen: CitizenId, out: &mut Outbox) {
        self.detach_rider(citizen);
        out.push(SimEvent::RiderStranded(citizen));
    }

    fn detach_rider(&mut self, citizen: CitizenId) {
        let Some(rider) = self.riders.shift_remove(&citizen) else {
            return;
        };
        if let Some(stop) = rider.stop_id.and_then(|id| self.stops.get_mut(&id))
            && let Some(at) = stop.waiting.iter().position(|&c| c == citizen)
        {
            stop.waiting.remove(at);
        }
        if let Some(bus) = rider.bus_id.and_then(|id| self.buses.get_mut(&id))
            && let Some(at) = bus.riders.iter().position(|&c| c == citizen)
        {
            bus.riders.remove(at);
        }
    }

    // --- Keeping up with road changes -----------------------------------------------

    /// `RoadNetwork.changed`: stops need finding again.
    pub fn on_roads_changed(&mut self, _change: &crate::roads::ChangeSet, _out: &mut Outbox) {
        self.dirty = true;
        self.nearby_stops.clear();
    }

    fn ensure_resolved(&mut self, ctx: &mut TransitCtx, out: &mut Outbox) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let stop_ids: Vec<StopId> = self.stops.keys().copied().collect();
        for stop_id in stop_ids {
            Self::resolve(&mut self.stops[&stop_id], ctx.roads);
            if !self.stops[&stop_id].is_active() {
                for citizen in self.stops[&stop_id].waiting.clone() {
                    self.strand(citizen, out);
                }
            }
        }
        let line_ids: Vec<LineId> = self.lines.keys().copied().collect();
        for line_id in line_ids {
            self.update_line(ctx, line_id);
        }
        self.update_crossings(ctx);
    }

    /// Finds the stop's lane again: the nearest lane going its way within
    /// `RESOLVE_DISTANCE`, then that road's curb lane (the road may have been widened).
    fn resolve(stop: &mut BusStop, roads: &RoadNetwork) {
        stop.lane_id = None;
        let mut best: Option<&Lane> = None;
        let mut best_distance = Self::RESOLVE_DISTANCE;
        for lane in roads.lanes.values() {
            if lane.is_connector() {
                continue;
            }
            let offset = lane.curve().closest_offset(stop.position);
            let (origin, angle) = lane.sample_pose(f64::from(offset));
            let distance = f64::from(origin.gd_distance_to(stop.position));
            if distance <= best_distance
                && f64::from(Vec2::from_angle(angle).gd_normalized().dot(stop.direction)) > 0.9
            {
                best = Some(lane);
                best_distance = distance;
            }
        }
        let Some(best) = best else {
            return;
        };
        let lane = Self::outermost(roads, best);
        let Some(spot) = Self::spot_on(lane, f64::from(lane.curve().closest_offset(stop.position)))
        else {
            return;
        };
        stop.lane_id = Some(spot.lane_id);
        stop.distance = spot.distance;
        stop.position = spot.position;
        stop.direction = spot.direction;
    }

    /// The lane at the curb on the same road and going the same way as `lane`.
    fn outermost<'a>(roads: &'a RoadNetwork, lane: &'a Lane) -> &'a Lane {
        let Some(segment) = lane.segment_id.and_then(|id| roads.segments.get(&id)) else {
            return lane;
        };
        let mut lane = lane;
        for lane_id in &segment.lane_ids {
            let other = &roads.lanes[lane_id];
            if other.from_node_id == lane.from_node_id
                && other.index_from_center > lane.index_from_center
            {
                lane = other;
            }
        }
        lane
    }

    /// A stop spot `distance` along `lane`, moved so that a whole bus fits on the lane;
    /// `None` if it never does.
    fn spot_on(lane: &Lane, distance: f64) -> Option<CurbSpot> {
        let margin = BUS_LENGTH * 0.5 + MIN_GAP;
        if lane.length < margin * 2.0 {
            return None;
        }
        let distance = distance.clamp(margin, lane.length - margin);
        let (position, angle) = lane.sample_pose(distance);
        Some(CurbSpot {
            lane_id: lane.id,
            distance,
            position,
            direction: Vec2::from_angle(angle).gd_normalized(),
        })
    }

    /// Re-times a line on the current roads (keeping measured times where it still runs)
    /// and redraws its path.
    fn update_line(&mut self, ctx: &mut TransitCtx, line_id: LineId) {
        let line = &self.lines[&line_id];
        let n = line.stop_ids.len();
        let previous = line.segment_times.clone();
        let mut free_flow_times = vec![0.0_f32; n];
        let mut segment_times = vec![0.0_f32; n];
        let mut routes: Vec<Option<Route>> = vec![None; n];
        let mut path: Vec<Vec2> = Vec::new();
        for i in 0..n {
            let a = &self.stops[&line.stop_ids[i]];
            let b = &self.stops[&line.stop_ids[line.next_index(i)]];
            let mut route = None;
            if let (Some(a_lane), Some(b_lane)) = (a.lane_id, b.lane_id) {
                route = ctx
                    .router
                    .find_route(ctx.roads, a_lane, a.distance, b_lane, b.distance, true);
            }
            let Some(route) = route else {
                free_flow_times[i] = f32::INFINITY;
                segment_times[i] = f32::INFINITY;
                continue;
            };
            free_flow_times[i] = route.free_flow_time(ctx.roads) as f32;
            let estimate = f64::from(free_flow_times[i]) + Self::DWELL_BASE;
            let measured = previous.get(i).map_or(f64::INFINITY, |&t| f64::from(t));
            segment_times[i] = if measured < f64::INFINITY && measured > 0.0 {
                measured
            } else {
                estimate
            } as f32;
            path.extend(route.polyline(ctx.roads));
            routes[i] = Some(route);
        }
        let line = &mut self.lines[&line_id];
        line.free_flow_times = free_flow_times;
        line.segment_times = segment_times;
        line.routes = routes;
        line.path = path;
    }

    /// Active stops on some line within `max_walk` px on foot of `point`: stop id →
    /// walking distance.
    fn stops_in_reach(
        &self,
        ctx: &mut TransitCtx,
        point: Vec2,
        max_walk: f64,
    ) -> IndexMap<StopId, f64> {
        let mut result = IndexMap::new();
        for stop in self.stops.values() {
            if !stop.is_active()
                || f64::from(stop.position.gd_distance_to(point)) > max_walk
                || self.lines_at(stop.id).is_empty()
            {
                continue;
            }
            let distance =
                ctx.pedestrians
                    .network
                    .distance(ctx.roads, ctx.crosswalks, point, stop.position);
            if distance <= max_walk {
                result.insert(stop.id, distance);
            }
        }
        result
    }

    /// Every stop gets a crosswalk across its road, just behind where the bus stands (so
    /// a stopped bus doesn't cover it), for riders coming from the other side; stops
    /// facing each other share one. Shared through `Crosswalks::mid_block` with
    /// pedestrians and drivers.
    fn update_crossings(&mut self, ctx: &mut TransitCtx) {
        self.nearby_stops.clear();
        let roads = ctx.roads;
        let mut crossings: IndexMap<IVec2, MidBlockCrossing> = IndexMap::new();
        for stop in self.stops.values() {
            let Some(lane_id) = stop.lane_id else {
                continue;
            };
            let lane = &roads.lanes[&lane_id];
            let segment = &roads.segments[&lane.segment_id.expect("a road lane")];
            let behind = (stop.distance - BUS_LENGTH * 0.5 - Self::CROSSING_SETBACK).max(0.0);
            let centerline = lane_builder::get_polyline(segment.centerline());
            let along =
                lane_builder::distance_along_polyline(&centerline, lane.sample_pose(behind).0);
            let length = lane_builder::polyline_length(&centerline);
            if along < roads.junction_radius(segment.start_node_id) + Self::CROSSING_SETBACK
                || along
                    > length - roads.junction_radius(segment.end_node_id) - Self::CROSSING_SETBACK
            {
                continue; // Too close to a junction, which has crosswalks of its own.
            }
            let mut shared = false;
            for crossing in crossings.values() {
                shared = shared
                    || (crossing.segment_id == segment.id
                        && (crossing.distance - along).abs() < Self::CROSSING_MERGE_DISTANCE);
            }
            if shared {
                continue;
            }
            let center = lane_builder::sub_polyline(&centerline, along, along)[0];
            let ahead_at = (along + 1.0).min(length);
            let ahead = lane_builder::sub_polyline(&centerline, ahead_at, ahead_at)[0];
            let lanes: Vec<(LaneId, f64)> = segment
                .lane_ids
                .iter()
                .map(|&id| {
                    (
                        id,
                        f64::from(roads.lanes[&id].curve().closest_offset(center)),
                    )
                })
                .collect();
            let direction = (ahead - center).gd_normalized();
            crossings.insert(
                IVec2::new(MID_BLOCK_BASE - stop.id.0, segment.id.0),
                MidBlockCrossing {
                    segment_id: segment.id,
                    distance: along,
                    center,
                    across: Vec2::new(direction.y, -direction.x),
                    half_width: segment.road_type.total_width() * 0.5,
                    lanes,
                },
            );
        }
        ctx.crosswalks.set_mid_block(crossings);
    }

    /// Stops (on some line) within `max_walk` px on foot of stop `stop_id`, itself
    /// included: stop id → walking distance. Cached until roads or stops change.
    fn stops_near_stop(
        &mut self,
        ctx: &mut TransitCtx,
        stop_id: StopId,
        max_walk: f64,
    ) -> IndexMap<StopId, f64> {
        if !self.nearby_stops.contains_key(&stop_id) {
            let mut near = self.stops_in_reach(ctx, self.stops[&stop_id].position, max_walk);
            near.insert(stop_id, 0.0);
            self.nearby_stops.insert(stop_id, near);
        }
        self.nearby_stops[&stop_id].clone()
    }

    fn walk_time(length: f64) -> f64 {
        length / PedestrianSystem::WALK_SPEED + PedestrianSystem::START_STOP_ALLOWANCE
    }
}

/// A state of the ride search: aboard a bus of line `.1` at stop `.0`.
type RideKey = (StopId, LineId);

/// `plan_ride`'s Dijkstra state (the GDScript keeps these in local dictionaries).
#[derive(Default)]
struct RideSearch {
    cost: HashMap<RideKey, f64>,
    transfers: HashMap<RideKey, i32>,
    ridden: HashMap<RideKey, i32>,
    parent: HashMap<RideKey, Option<RideKey>>,
    open: Vec<RideKey>,
}

impl RideSearch {
    fn relax(
        &mut self,
        key: RideKey,
        value: f64,
        transfer_count: i32,
        rides: i32,
        from_key: Option<RideKey>,
    ) {
        if value < self.cost.get(&key).copied().unwrap_or(f64::INFINITY) {
            self.cost.insert(key, value);
            self.transfers.insert(key, transfer_count);
            self.ridden.insert(key, rides);
            self.parent.insert(key, from_key);
            self.open.push(key);
        }
    }
}
