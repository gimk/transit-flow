//! `VehicleSystem` (`sim/traffic/vehicle_system.gd`): moves vehicles along their routes
//! each tick, in two passes:
//! 1. Prepare: for every vehicle, find the gap to whatever is ahead: the vehicle in front
//!    on its lane or, for the front-most vehicle, along the next lanes of its route, and
//!    any junction it hasn't been allowed into yet (a stop line). Vehicles reaching a stop
//!    line ask to enter their junction.
//! 2. Move: pick a speed (lane limits, slowing for turns and for the stop, following the
//!    vehicle ahead) and move, never closer than MIN_GAP to the vehicle ahead and never
//!    past a stop line. Gaps come from the start of the tick and vehicles only move
//!    forward, so no two vehicles can overlap.
//!
//! Junctions. Conflicting connectors (paths that cross or merge) only share a short
//! stretch, their conflict zone. A vehicle at a stop line may enter its connector if:
//! - the junction's traffic light, if it has one, is green for its lane,
//! - everyone on or allowed onto a conflicting connector will have left the shared zone
//!   before this vehicle could reach it (`conflicts_clear`), so movements that don't meet
//!   flow in parallel and a waiting vehicle starts before the junction is empty,
//! - traffic it must give way to (`must_yield`) isn't coming too close (`gap_accepted`),
//! - the lane it leads to has room for the whole vehicle ("don't block the box"),
//! - no vehicle that has waited longer at the same junction, and could go, wants a
//!   conflicting move.
//!
//! At junctions without lights, drivers also slow down to look before the stop line
//! (`entry_speed`). Entry, once granted, is never taken back. Inside the junction, a
//! vehicle still stops short of a conflict zone that an earlier entrant hasn't cleared
//! (`yield_in_junction`), in case the timing prediction was off.
//!
//! After any road or lot change every vehicle re-routes from where it is; a vehicle that
//! can no longer reach its destination fails its trip.
//!
//! The port is split by subject, as the GDScript file is by section: this module (state,
//! public API, the tick, movement, bookkeeping, re-routing), `junctions`, `lane_changes`,
//! `driveways`, `buses` (bays, passing stopped buses) and `crosswalks`. Rust specifics:
//! - Lane queues live here (`occupied`: the non-empty queues, by lane id, in the order the
//!   lanes became occupied, as Godot's `_occupied_lanes`), not on `Lane`.
//! - Vehicles hold lane ids; a lane the network has removed is still found with
//!   `RoadNetwork::lane_any` (retired lanes), which `prune_retired_lanes` drops once no
//!   vehicle is on it. Godot clears a removed lane's `outgoing_lanes` (objects) but keeps
//!   its ids, so code that walked `outgoing_lanes` uses `live_outgoing` here.
//! - Vehicles that arrive or fail leave `vehicles` for `finished` until the tick ends, so
//!   the listeners of their events can read them (`vehicle_any`).

mod buses;
mod crosswalks;
mod driveways;
mod junctions;
mod lane_changes;

use std::collections::HashMap;

use glam::Vec2;
use indexmap::IndexMap;

use crate::city::LotSystem;
use crate::events::{Outbox, SimEvent};
use crate::geom::GdVec2;
use crate::ids::{LaneId, LotId, NodeId, VehicleId};
use crate::roads::{ChangeSet, Lane, RoadNetwork, Turn};
use crate::routing::{LanePoint, Route, Router};
use crate::traffic::TrafficLights;
use crate::traffic::vehicle::{self, Kind, Vehicle};
use crate::walking::Crosswalks;

pub const ACCELERATION: f64 = 60.0; // px/s²
/// Comfortable braking, used to plan speeds ahead of turns, stops and queues.
pub const DECELERATION: f64 = 100.0; // px/s²
/// Hardest braking a vehicle may actually do when the plan changes suddenly.
pub const MAX_DECELERATION: f64 = 300.0; // px/s²
/// Bumper-to-bumper distance kept to the vehicle ahead when stopped.
pub const MIN_GAP: f64 = 4.0;
/// Extra following distance per unit of speed, in seconds.
pub const TIME_HEADWAY: f64 = 0.8;
/// Vehicles never crawl slower than this toward their own stop, so they always reach it.
pub const MIN_SPEED: f64 = 4.0;
pub const ARRIVAL_EPSILON: f64 = 0.5;
/// Extra look-ahead beyond braking distance.
pub const LOOKAHEAD_MARGIN: f64 = 20.0;
/// How far a vehicle may be from a rebuilt lane to be moved onto it.
pub const RESNAP_DISTANCE: f64 = 2.0;
/// Below this a vehicle counts as stopped (for stuck detection).
pub const STOPPED_SPEED: f64 = 1.0;
/// Below this fraction of the lane's limit a vehicle counts as held up (for re-routing).
pub const SLOW_FRACTION: f64 = 0.3;
/// Patience. A trip's yardstick is its free-flow time plus a little for starting and
/// stopping; past IMPATIENT_FACTOR x that + IMPATIENT_GRACE the driver is impatient (red),
/// past GIVE_UP_FACTOR x that + GIVE_UP_GRACE they give up and the trip fails.
pub const START_STOP_ALLOWANCE: f64 = 2.0; // Sim seconds.
pub const IMPATIENT_FACTOR: f64 = 2.0;
pub const IMPATIENT_GRACE: f64 = 15.0;
pub const GIVE_UP_FACTOR: f64 = 4.0;
pub const GIVE_UP_GRACE: f64 = 60.0;
/// Safety margin between one vehicle leaving a conflict zone and the next reaching it.
pub const CONFLICT_MARGIN: f64 = 0.5; // Sim seconds.
/// Below this speed a vehicle's progress through a junction can't be predicted.
pub const MIN_PREDICT_SPEED: f64 = 2.0;
/// A vehicle that has waited this much longer for the same nearly full exit lane gets
/// the next space in it (see `can_enter`).
pub const EXIT_FAIRNESS_WAIT: f64 = 5.0;
/// Driveways (`pull_out`, `final_speed`): speed when turning off the road into a lot, how
/// far back from the curb a waiting vehicle is drawn, the width kept clear when crossing
/// a lane, and how long a vehicle waits to turn across the road before traffic makes room
/// (COURTESY_WAIT) or it gives up and goes another way (DRIVEWAY_GIVE_UP).
pub const DRIVEWAY_SPEED: f64 = 15.0;
pub const TRUCK_DRIVEWAY_SPEED: f64 = 10.0;
pub const DRIVEWAY_SETBACK: f64 = 6.0;
pub const DRAWN_DRIVEWAY_QUEUE: i32 = 3;
pub const CROSSING_HALF_WIDTH: f64 = 8.0;
pub const COURTESY_WAIT: f64 = 3.0;
pub const QUICK_COURTESY_WAIT: f64 = 0.5;
pub const DRIVEWAY_GIVE_UP: f64 = 8.0;
/// Waiting on the road to turn in holds up the traffic behind, so drivers give up sooner.
pub const TURN_IN_GIVE_UP: f64 = 5.0;
/// Lane changes (`change_lanes`): a vehicle that still has to change lanes stops this far
/// short of its lane's end, and within MERGE_DISTANCE of it (or once stopped) it counts
/// as merging, so the traffic beside it makes room.
pub const LANE_CHANGE_END_MARGIN: f64 = 2.0;
pub const MERGE_DISTANCE: f64 = 60.0;
/// A merging vehicle stopped at the end of its lane this long gives up the lane change,
/// goes on from the lane it's in, and re-routes (sim seconds).
pub const MISSED_LANE_WAIT: f64 = 2.0;
/// A vehicle that changed lanes is drawn steering across, along an S-curve as long as
/// this many times the distance across (as it drives: no strafing), with a nose angle
/// that follows the curve. Slow or stopped vehicles still finish within
/// LANE_CHANGE_MAX_TIME (sim seconds), so they don't sit across two lanes.
pub const LANE_CHANGE_LENGTH_RATIO: f64 = 5.0;
pub const LANE_CHANGE_MIN_LENGTH: f64 = 20.0;
pub const LANE_CHANGE_MAX_TIME: f64 = 2.0;
pub const LANE_CHANGE_MAX_YAW: f64 = 0.45;
/// A waiting driver accepts ever smaller gaps, down to none after this long.
pub const GAP_SHRINK_TIME: f64 = 10.0; // Sim seconds.
/// At a junction without lights, drivers slow to this fraction of the speed limit by the
/// stop line to look both ways, even when the junction is empty.
pub const LOOK_SPEED_FRACTION: f64 = 0.4;
/// Bus bays: how far beside its lane a bus in a bay is (drawn), and how long it waits to
/// pull out before the traffic lets it.
pub const BAY_OFFSET: f64 = 12.0;
pub const BAY_COURTESY_WAIT: f64 = 1.0;
/// Drivers this close behind a bus stopped at its stop (beyond their own length) pull out
/// round it (`pass_stopped_buses`).
pub const PASS_DISTANCE: f64 = 20.0;
/// Half the depth of a mid-block crosswalk (drivers stop this far short of its middle).
pub const MID_BLOCK_HALF_WIDTH: f64 = 5.0;
/// Without lights, a driver who has waited this long at a busy crosswalk gets to go once
/// it is empty (`crosswalks_clear`).
pub const CROSSWALK_PATIENCE: f64 = 4.0;
/// At lights, a turning driver who has waited this long may go on amber
/// (`control_allows`).
pub const SNEAK_WAIT: f64 = 5.0;
pub const SNEAK_GRACE: f64 = 2.0;

/// Gap acceptance (`gap_accepted`): how far away, in seconds, approaching conflicting
/// traffic must be before a driver enters, by turn. (Vehicles here drive about 2.5x
/// faster in car lengths per second than real ones, so these are below real-world gaps.)
pub fn critical_gap(turn: Turn) -> f64 {
    match turn {
        Turn::Left | Turn::UTurn => 2.5,
        Turn::Right => 1.5,
        // STRAIGHT, and the default for anything else (CRITICAL_GAP.get(turn, STRAIGHT)).
        Turn::Straight | Turn::None => 2.0,
    }
}

/// Who gives way at a junction (`must_yield`): higher ranks yield to lower ones.
fn yield_rank(turn: Turn) -> i32 {
    match turn {
        Turn::Straight => 0,
        Turn::Right => 1,
        Turn::Left | Turn::UTurn => 2,
        Turn::None => 0,
    }
}

/// What the vehicle system reads and changes besides its own state, for one call.
pub struct TrafficCtx<'a> {
    pub roads: &'a RoadNetwork,
    pub router: &'a mut Router,
    pub lots: &'a mut LotSystem,
    pub signals: &'a TrafficLights,
    pub crosswalks: &'a Crosswalks,
}

#[derive(Clone, Debug)]
pub struct VehicleSystem {
    /// Every vehicle on the road (not those waiting in driveways), by id.
    pub vehicles: IndexMap<VehicleId, Vehicle>,
    /// Vehicles that arrived or failed this tick, kept for their events' listeners.
    pub finished: IndexMap<VehicleId, Vehicle>,
    next_id: i32,
    reroute_pending: bool,
    time: f64,
    /// Lanes with at least one vehicle → their queue, front-most first (Godot keeps the
    /// queue on `Lane.queue` and the lanes in `_occupied_lanes`).
    occupied: IndexMap<LaneId, Vec<VehicleId>>,
    /// Connector id → vehicles on it or allowed onto it.
    connector_vehicles: IndexMap<LaneId, Vec<VehicleId>>,
    /// Lane id → vehicles allowed through a junction into this lane that haven't reached
    /// it.
    reserved_entries: HashMap<LaneId, i32>,
    /// Junction node id → vehicles waiting at its stop lines.
    waiting: IndexMap<NodeId, Vec<VehicleId>>,
    /// Increases with every junction entry granted; orders vehicles for
    /// `yield_in_junction`.
    entry_counter: i64,
    /// Vehicles waiting in driveways to pull out (not on the road yet), oldest first.
    departures: Vec<Vehicle>,
    /// (connector, conflicting connector) → must give way to it (`must_yield`, cached;
    /// connector ids change whenever their junction is rebuilt, so cleared on road
    /// changes).
    yields: HashMap<(LaneId, LaneId), bool>,
    /// Connector id → `entry_speed` (cached the same way).
    entry_speeds: HashMap<LaneId, f64>,
}

impl Default for VehicleSystem {
    fn default() -> Self {
        Self {
            vehicles: IndexMap::new(),
            finished: IndexMap::new(),
            next_id: 1,
            reroute_pending: false,
            time: 0.0,
            occupied: IndexMap::new(),
            connector_vehicles: IndexMap::new(),
            reserved_entries: HashMap::new(),
            waiting: IndexMap::new(),
            entry_counter: 0,
            departures: Vec::new(),
            yields: HashMap::new(),
            entry_speeds: HashMap::new(),
        }
    }
}

/// A lane by id, live or retired (vehicles may still be on a retired one).
fn lane(roads: &RoadNetwork, id: LaneId) -> &Lane {
    roads.lane_any(id).expect("a lane vehicles know of")
}

/// The lanes a lane leads to, as Godot's `outgoing_lanes` (objects): none once the lane
/// has been removed from the network.
fn live_outgoing(roads: &RoadNetwork, lane: &Lane) -> Vec<LaneId> {
    if roads.lanes.contains_key(&lane.id) {
        lane.outgoing_lane_ids.clone()
    } else {
        Vec::new()
    }
}

/// Godot's `move_toward`.
fn move_toward(from: f64, to: f64, delta: f64) -> f64 {
    if (to - from).abs() <= delta {
        to
    } else {
        from + (to - from).signum() * delta
    }
}

/// Unit vector to the right of a direction (y grows downward).
fn right_of(direction: Vec2) -> Vec2 {
    Vec2::new(-direction.y, direction.x).gd_normalized()
}

/// Buses route with bus lanes (Router `bus` arguments).
fn is_bus(vehicle: &Vehicle) -> bool {
    vehicle.kind == Kind::Bus
}

impl VehicleSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn time(&self) -> f64 {
        self.time
    }

    /// A vehicle on the road or one that finished this tick (event listeners).
    pub fn vehicle_any(&self, id: VehicleId) -> Option<&Vehicle> {
        self.vehicles.get(&id).or_else(|| self.finished.get(&id))
    }

    /// Vehicles on a lane, front-most first.
    pub fn queue(&self, lane_id: LaneId) -> &[VehicleId] {
        self.occupied
            .get(&lane_id)
            .map_or(&[], |queue| queue.as_slice())
    }

    // --- Public API -----------------------------------------------------------------

    /// Puts a new vehicle at the start of `route` right away (debug traffic, tests).
    /// `None` if there's no room there. `destination_lot_id` may be `None` for trips to a
    /// plain lane position. City trips use `depart` instead.
    pub fn spawn(
        &mut self,
        roads: &RoadNetwork,
        route: Route,
        destination_lot_id: Option<LotId>,
        owner_id: i32,
        kind: Kind,
    ) -> Option<VehicleId> {
        let vehicle = self.new_vehicle(roads, owner_id, kind, destination_lot_id, &route);
        if !roads.lanes.contains_key(&route.lane_ids[0])
            || !self.can_spawn_at(
                roads,
                route.lane_ids[0],
                route.start_distance,
                vehicle.length,
            )
        {
            return None;
        }
        let id = vehicle.id;
        self.put_on_road(roads, vehicle, route);
        Some(id)
    }

    /// Starts a trip from a driveway. The vehicle waits off the road, at its lot, until
    /// it can pull out into a gap (see `pull_out`): `origin_lot_id`'s driveway, or
    /// (`None`) the lane positions `origin_points`, e.g. a highway entering the map. It
    /// goes to `destination_lot_id`'s driveway, or (`None`) the quickest of
    /// `destination_points`. `None` if there's no way there. The trip's clock starts now:
    /// waiting to pull out counts.
    #[allow(clippy::too_many_arguments)]
    pub fn depart(
        &mut self,
        ctx: &mut TrafficCtx,
        origin_lot_id: Option<LotId>,
        mut origin_points: Vec<LanePoint>,
        destination_lot_id: Option<LotId>,
        mut destination_points: Vec<LanePoint>,
        owner_id: i32,
        kind: Kind,
    ) -> Option<VehicleId> {
        if let Some(lot) = origin_lot_id {
            origin_points = ctx.lots.access_points(ctx.roads, lot);
        }
        if origin_lot_id.is_some() && origin_lot_id == destination_lot_id {
            return None; // Already there.
        }
        if let Some(lot) = destination_lot_id {
            destination_points = ctx.lots.access_points(ctx.roads, lot);
        }
        let route =
            ctx.router
                .find_best_route(ctx.roads, &origin_points, &destination_points, false)?;
        let mut vehicle = self.new_vehicle(ctx.roads, owner_id, kind, destination_lot_id, &route);
        vehicle.origin_lot_id = origin_lot_id;
        vehicle.origin_points = origin_points;
        vehicle.driveway_wait_since = self.time;
        Self::plan_departure(ctx, &mut vehicle, route);
        let id = vehicle.id;
        self.departures.push(vehicle);
        self.place_waiting_departures(ctx);
        Some(id)
    }

    /// Vehicles waiting in driveways to pull out, in the order they set off.
    pub fn waiting_departures(&self) -> &[Vehicle] {
        &self.departures
    }

    /// Sends a dwelling bus on: `route` must start where it is (its lane, at or behind its
    /// position). Its trip clock restarts.
    pub fn set_route(&mut self, roads: &RoadNetwork, id: VehicleId, route: &Route) {
        let time = self.time;
        let vehicle = &mut self.vehicles[&id];
        vehicle.next_lanes = route.lane_ids[1..].to_vec();
        vehicle.end_distance = route.end_distance;
        vehicle.destination_lane_id = route.lane_ids.last().copied();
        vehicle.destination_distance = route.end_distance;
        vehicle.trip_start_time = time;
        vehicle.ideal_trip_time = route.free_flow_time(roads) + START_STOP_ALLOWANCE;
        if vehicle.in_bay {
            // It waits in the bay for a gap to pull out (leave_bays).
            vehicle.leaving_bay_since = time;
        } else {
            vehicle.dwelling = false;
        }
    }

    fn new_vehicle(
        &mut self,
        roads: &RoadNetwork,
        owner_id: i32,
        kind: Kind,
        destination_lot_id: Option<LotId>,
        route: &Route,
    ) -> Vehicle {
        let mut vehicle = Vehicle::default();
        vehicle.set_kind(kind);
        vehicle.id = VehicleId(self.next_id);
        self.next_id += 1;
        vehicle.owner_id = owner_id;
        vehicle.destination_lot_id = destination_lot_id;
        vehicle.destination_lane_id = route.lane_ids.last().copied();
        vehicle.destination_distance = route.end_distance;
        vehicle.trip_start_time = self.time;
        vehicle.ideal_trip_time = route.free_flow_time(roads) + START_STOP_ALLOWANCE;
        vehicle
    }

    fn put_on_road(&mut self, roads: &RoadNetwork, mut vehicle: Vehicle, route: Route) {
        vehicle.lane = Some(route.lane_ids[0]);
        vehicle.distance = route.start_distance;
        vehicle.next_lanes = route.lane_ids[1..].to_vec();
        vehicle.end_distance = route.end_distance;
        vehicle.route = None;
        vehicle.crossing_lanes.clear();
        vehicle.crossing_distances.clear();
        vehicle.arrival_checked = false;
        vehicle.driveway_wait_since = f64::INFINITY;
        let drawn_at = vehicle.position;
        let drawn_rotation = vehicle.rotation;
        Self::update_pose(roads, &mut vehicle);
        let from_lot = vehicle.origin_lot_id.is_some();
        vehicle.previous_position = if from_lot { drawn_at } else { vehicle.position };
        vehicle.previous_rotation = if from_lot {
            drawn_rotation
        } else {
            vehicle.rotation
        };
        let id = vehicle.id;
        self.vehicles.insert(id, vehicle);
        self.enqueue(id);
    }

    /// True if a vehicle could appear at `distance` along the lane without touching
    /// another one, and with enough room for the vehicle behind to brake.
    pub fn can_spawn_at(
        &self,
        roads: &RoadNetwork,
        lane_id: LaneId,
        distance: f64,
        length: f64,
    ) -> bool {
        if !roads.lanes.contains_key(&lane_id) {
            return false;
        }
        for other_id in self.queue(lane_id) {
            let other = &self.vehicles[other_id];
            let ahead_by = other.distance - distance;
            let mut needed = (other.length + length) * 0.5 + MIN_GAP;
            if ahead_by < 0.0 {
                needed += other.speed * other.speed / (2.0 * MAX_DECELERATION);
            }
            if ahead_by.abs() < needed {
                return false;
            }
        }
        true
    }

    /// Removes a vehicle without reporting an arrival or failure (e.g. its owner is gone).
    pub fn remove(&mut self, roads: &RoadNetwork, id: VehicleId) {
        if self.vehicles.contains_key(&id) {
            self.detach(roads, id);
            self.vehicles.shift_remove(&id);
            return;
        }
        if let Some(i) = self.departures.iter().position(|waiting| waiting.id == id) {
            self.departures.remove(i);
        }
    }

    /// Lanes that currently have vehicles on them.
    pub fn occupied_lanes(&self) -> impl Iterator<Item = LaneId> + '_ {
        self.occupied.keys().copied()
    }

    /// Number of vehicles that haven't moved for at least `seconds`.
    pub fn stuck_count(&self, seconds: f64) -> usize {
        self.vehicles
            .values()
            .filter(|v| v.stopped_time >= seconds)
            .count()
    }

    /// Drops the vehicles that left the road once their events have been delivered, and
    /// the retired lanes no vehicle is on any more.
    pub fn clear_finished(&mut self) {
        self.finished.clear();
    }

    /// Lets the network drop retired lanes no vehicle is on, or about to drive, or
    /// waiting at.
    pub fn prune_retired_lanes(&self, roads: &mut RoadNetwork) {
        if roads.retired_lanes.is_empty() {
            return;
        }
        let mut used = std::collections::HashSet::new();
        for vehicle in self.vehicles.values().chain(self.departures.iter()) {
            used.extend(vehicle.lane);
            used.extend(vehicle.next_lanes.iter().copied());
            used.extend(vehicle.granted_connector);
            used.extend(vehicle.requested_connector);
            used.extend(vehicle.crossing_lanes.iter().copied());
        }
        roads.prune_retired_lanes(|id| used.contains(&id) || self.occupied.contains_key(&id));
    }

    /// The start of `step(dt)`: advance the clock and re-route everyone if the roads or
    /// lots changed. Split from `step_rest` because re-routing emits `VehicleFailed`,
    /// whose listeners run before the rest of the step in Godot.
    pub fn step_begin(&mut self, dt: f64, ctx: &mut TrafficCtx, out: &mut Outbox) {
        self.time += dt;
        if self.reroute_pending {
            self.reroute_pending = false;
            self.reroute_all(ctx, out);
        }
    }

    /// The rest of `step(dt)`: driveways, lane changes, movement, arrivals, give-ups.
    pub fn step_rest(&mut self, dt: f64, ctx: &mut TrafficCtx, out: &mut Outbox) {
        self.pull_out(ctx);
        self.leave_bays(ctx);
        self.pass_stopped_buses(ctx);
        self.change_lanes(ctx);
        self.prepare(ctx);
        self.make_room_for_mergers(ctx);
        self.keep_crossings_clear(ctx);
        self.let_buses_out(ctx);
        self.give_way_at_mid_block(ctx);
        let mut arrived = Vec::new();
        let mut gave_up = Vec::new();
        let ids: Vec<VehicleId> = self.vehicles.keys().copied().collect();
        for id in ids {
            {
                let vehicle = &mut self.vehicles[&id];
                vehicle.previous_position = vehicle.position;
                vehicle.previous_rotation = vehicle.rotation;
                if vehicle.dwelling {
                    // A bus at its stop, holding up the lane until it is sent on (or off it,
                    // in a bay, drawn beside the lane).
                    vehicle.speed = 0.0;
                    if vehicle.in_bay {
                        Self::start_slide(vehicle, BAY_OFFSET);
                    }
                    continue;
                }
                Self::slide(vehicle, dt);
            }
            self.update_speed(ctx, id, dt);
            if self.advance(ctx, id, dt) {
                arrived.push(id);
            }
            let time = self.time;
            let vehicle = &mut self.vehicles[&id];
            Self::update_pose(ctx.roads, vehicle);
            vehicle.stopped_time = if vehicle.speed < STOPPED_SPEED {
                vehicle.stopped_time + dt
            } else {
                0.0
            };
            let limit = lane(ctx.roads, vehicle.lane.expect("on the road")).speed_limit;
            let slow = vehicle.speed < SLOW_FRACTION * limit;
            vehicle.slow_time = if slow { vehicle.slow_time + dt } else { 0.0 };
            let elapsed = time - vehicle.trip_start_time;
            if elapsed > vehicle.ideal_trip_time * IMPATIENT_FACTOR + IMPATIENT_GRACE
                && !vehicle.in_service
            {
                vehicle.impatient = true;
                if elapsed > vehicle.ideal_trip_time * GIVE_UP_FACTOR + GIVE_UP_GRACE {
                    gave_up.push(id);
                }
            }
        }
        for id in arrived {
            let time = self.time;
            let vehicle = &mut self.vehicles[&id];
            vehicle.trip_end_time = time;
            if vehicle.in_service {
                vehicle.dwelling = true;
                vehicle.speed = 0.0;
            } else {
                self.detach(ctx.roads, id);
                let vehicle = self.vehicles.shift_remove(&id).expect("arrived");
                self.finished.insert(id, vehicle);
            }
            out.push(SimEvent::VehicleArrived(id));
        }
        for id in gave_up {
            if let Some(vehicle) = self.vehicles.get_mut(&id) {
                vehicle.gave_up = true;
                self.fail(ctx.roads, id, out);
            }
        }
    }

    // --- Pass 2: speed and movement -------------------------------------------------

    /// The fastest speed from which the vehicle can still brake, comfortably, down to
    /// every upcoming lane's limit, to its stop, to a stop line, and behind the vehicle
    /// ahead: v = sqrt(v_then² + 2·decel·gap). Gaps are measured from where the vehicle
    /// will be after this tick's move, otherwise braking starts one tick late.
    fn update_speed(&mut self, ctx: &mut TrafficCtx, id: VehicleId, dt: f64) {
        // At its destination it slows to driveway speed and turns off the road, or stops
        // if it has to wait for a gap to turn across the road.
        let final_speed = self.final_speed(ctx, id);
        let roads = ctx.roads;
        let vehicle = &self.vehicles[&id];
        let current = lane(roads, vehicle.lane.expect("on the road"));
        let travel = vehicle.speed * dt;
        let mut ahead = vehicle.remaining_on_lane(current) - travel;
        let mut target = current.speed_limit;
        if vehicle.is_on_last_lane() {
            let stop = (final_speed * final_speed + 2.0 * DECELERATION * ahead.max(0.0)).sqrt();
            target = target.min(stop.max(MIN_SPEED));
        } else {
            let horizon = vehicle.speed * vehicle.speed / (2.0 * DECELERATION) + LOOKAHEAD_MARGIN;
            let last_index = vehicle.next_lanes.len() - 1;
            for (i, &next_id) in vehicle.next_lanes.iter().enumerate() {
                if ahead > horizon {
                    break;
                }
                let next = lane(roads, next_id);
                let previous = if i == 0 {
                    current
                } else {
                    lane(roads, vehicle.next_lanes[i - 1])
                };
                if next.is_beside(previous) {
                    // A lane change: the lane beside starts level with the previous one's
                    // start.
                    ahead -= previous.length;
                } else {
                    let entry_speed = if next.is_connector() {
                        Self::entry_speed_cached(&mut self.entry_speeds, roads, next)
                    } else {
                        next.speed_limit
                    };
                    target = target.min(
                        (entry_speed * entry_speed + 2.0 * DECELERATION * ahead.max(0.0)).sqrt(),
                    );
                }
                if i == last_index {
                    let stop = (final_speed * final_speed
                        + 2.0 * DECELERATION * (ahead + vehicle.end_distance).max(0.0))
                    .sqrt();
                    target = target.min(stop.max(MIN_SPEED));
                }
                ahead += next.length;
            }
        }
        if vehicle.stop_gap < f64::INFINITY {
            target = target.min((2.0 * DECELERATION * (vehicle.stop_gap - travel).max(0.0)).sqrt());
        }
        if vehicle.leader_gap < f64::INFINITY {
            let gap = (vehicle.leader_gap - MIN_GAP - travel).max(0.0);
            let leader_speed = vehicle.leader_speed;
            target = target.min((leader_speed * leader_speed + 2.0 * DECELERATION * gap).sqrt());
            target = target.min(leader_speed + gap / TIME_HEADWAY);
        }
        let rate = if vehicle.speed < target {
            ACCELERATION
        } else {
            MAX_DECELERATION
        };
        let vehicle = &mut self.vehicles[&id];
        vehicle.speed = move_toward(vehicle.speed, target, rate * dt);
    }

    /// Moves the vehicle for one tick. Returns true when it reached its stop.
    fn advance(&mut self, ctx: &mut TrafficCtx, id: VehicleId, dt: f64) -> bool {
        let mut step = {
            let vehicle = &mut self.vehicles[&id];
            let mut step = vehicle.speed * dt;
            let limit = (vehicle.leader_gap - MIN_GAP).min(vehicle.stop_gap);
            if step > limit {
                step = limit.max(0.0);
                vehicle.speed = step / dt;
            }
            step
        };
        loop {
            let vehicle = &self.vehicles[&id];
            let current = lane(ctx.roads, vehicle.lane.expect("on the road"));
            let remaining = vehicle.remaining_on_lane(current);
            if vehicle.is_on_last_lane() && remaining <= ARRIVAL_EPSILON + step {
                if self.must_cross_to_arrive(ctx, id) && !self.can_turn_in(id) {
                    // Waiting for a gap to turn across the road into its driveway, holding
                    // up the traffic behind it. (Only as far as its move allows: it stays
                    // on the road.)
                    let time = self.time;
                    let vehicle = &mut self.vehicles[&id];
                    vehicle.distance += step.min(remaining.max(0.0));
                    vehicle.speed = 0.0;
                    if vehicle.driveway_wait_since == f64::INFINITY {
                        vehicle.driveway_wait_since = time;
                    }
                    return false;
                }
                let vehicle = &mut self.vehicles[&id];
                if vehicle.in_service {
                    // A bus stays on the road: no further than its move allows (the vehicle
                    // ahead may be closer than the stop).
                    vehicle.distance += step.min(remaining.max(0.0));
                } else {
                    vehicle.distance = vehicle.end_distance;
                }
                return true;
            }
            if step < remaining {
                self.vehicles[&id].distance += step;
                return false;
            }
            let next = vehicle.next_lanes[0];
            if lane(ctx.roads, next).is_beside(current) {
                // Out of road before it could change lanes (stop_gap should prevent this).
                let length = current.length;
                let vehicle = &mut self.vehicles[&id];
                vehicle.distance = length;
                vehicle.speed = 0.0;
                return false;
            }
            step -= remaining.max(0.0);
            self.vehicles[&id].next_lanes.remove(0);
            self.hop(ctx.roads, id, next);
        }
    }

    /// Moves the vehicle from the end of its lane onto the start of `next`.
    fn hop(&mut self, roads: &RoadNetwork, id: VehicleId, next: LaneId) {
        self.dequeue(id);
        let current = self.vehicles[&id].lane.expect("on the road");
        if lane(roads, current).is_connector() {
            self.release_connector(roads, id, current);
        }
        let vehicle = &mut self.vehicles[&id];
        if Some(next) == vehicle.granted_connector {
            // The grant becomes "using the connector": counts stay until it leaves.
            vehicle.granted_connector = None;
        }
        vehicle.lane = Some(next);
        vehicle.distance = 0.0;
        self.enqueue(id);
    }

    fn update_pose(roads: &RoadNetwork, vehicle: &mut Vehicle) {
        let (position, angle) =
            lane(roads, vehicle.lane.expect("on the road")).sample_pose(vehicle.distance);
        vehicle.position = position;
        vehicle.rotation = f64::from(angle);
        if vehicle.lateral_offset != 0.0 {
            vehicle.position += right_of(Vec2::from_angle(angle)) * vehicle.lateral_offset as f32;
            vehicle.rotation += vehicle.slide_yaw;
        }
    }

    // --- Bookkeeping ----------------------------------------------------------------

    /// Inserts the vehicle into its lane's queue, keeping front-most first.
    fn enqueue(&mut self, id: VehicleId) {
        let vehicle = &self.vehicles[&id];
        let distance = vehicle.distance;
        let queue = self
            .occupied
            .entry(vehicle.lane.expect("on the road"))
            .or_default();
        let mut index = queue.len();
        while index > 0 && self.vehicles[&queue[index - 1]].distance < distance {
            index -= 1;
        }
        queue.insert(index, id);
    }

    fn dequeue(&mut self, id: VehicleId) {
        let lane_id = self.vehicles[&id].lane.expect("on the road");
        let Some(queue) = self.occupied.get_mut(&lane_id) else {
            return;
        };
        if let Some(i) = queue.iter().position(|&other| other == id) {
            queue.remove(i);
        }
        if queue.is_empty() {
            self.occupied.shift_remove(&lane_id);
        }
    }

    /// Forgets everything the vehicle holds: its place in a queue, a connector it's on or
    /// was allowed into, a junction it's waiting at.
    fn detach(&mut self, roads: &RoadNetwork, id: VehicleId) {
        self.dequeue(id);
        let current = self.vehicles[&id].lane.expect("on the road");
        if lane(roads, current).is_connector() {
            self.release_connector(roads, id, current);
        }
        self.release_grant(roads, id);
        self.stop_waiting(id);
    }

    /// Records the vehicle as using (or about to use) the connector, and as heading into
    /// the lane after it.
    fn hold_connector(&mut self, roads: &RoadNetwork, id: VehicleId, connector: LaneId) {
        self.connector_vehicles
            .entry(connector)
            .or_default()
            .push(id);
        let exit = lane(roads, connector).outgoing_lane_ids[0];
        *self.reserved_entries.entry(exit).or_insert(0) += 1;
    }

    fn release_connector(&mut self, roads: &RoadNetwork, id: VehicleId, connector: LaneId) {
        if let Some(users) = self.connector_vehicles.get_mut(&connector) {
            if let Some(i) = users.iter().position(|&other| other == id) {
                users.remove(i);
            }
            if users.is_empty() {
                self.connector_vehicles.shift_remove(&connector);
            }
        }
        let exit = lane(roads, connector).outgoing_lane_ids[0];
        Self::decrement(&mut self.reserved_entries, exit);
    }

    fn release_grant(&mut self, roads: &RoadNetwork, id: VehicleId) {
        if let Some(granted) = self.vehicles[&id].granted_connector {
            self.release_connector(roads, id, granted);
            self.vehicles[&id].granted_connector = None;
        }
    }

    fn decrement(counts: &mut HashMap<LaneId, i32>, key: LaneId) {
        let value = counts.get(&key).copied().unwrap_or(0) - 1;
        if value > 0 {
            counts.insert(key, value);
        } else {
            counts.remove(&key);
        }
    }

    // --- Re-routing after network changes -------------------------------------------

    pub fn on_roads_changed(&mut self, _change: &ChangeSet, _out: &mut Outbox) {
        self.reroute_pending = true;
        self.yields.clear();
        self.entry_speeds.clear();
    }

    /// `LotSystem.changed` → `_request_reroute`.
    pub fn request_reroute(&mut self, _out: &mut Outbox) {
        self.reroute_pending = true;
    }

    fn reroute_all(&mut self, ctx: &mut TrafficCtx, out: &mut Outbox) {
        let ids: Vec<VehicleId> = self.vehicles.keys().copied().collect();
        for id in ids {
            self.reroute_vehicle(ctx, id, out);
        }
        // Waiting in driveways: plan again from the lot (or fail if there is no way now).
        let waiting: Vec<VehicleId> = self.departures.iter().map(|v| v.id).collect();
        for id in waiting {
            let Some(i) = self.departures.iter().position(|v| v.id == id) else {
                continue;
            };
            let mut vehicle = std::mem::take(&mut self.departures[i]);
            let points = match vehicle.origin_lot_id {
                Some(lot) => ctx.lots.access_points(ctx.roads, lot),
                None => vehicle.origin_points.clone(),
            };
            let destination = Self::destination(ctx, &vehicle);
            match ctx
                .router
                .find_best_route(ctx.roads, &points, &destination, is_bus(&vehicle))
            {
                None => {
                    self.departures.remove(i);
                    vehicle.trip_end_time = self.time;
                    self.finished.insert(id, vehicle);
                    out.push(SimEvent::VehicleFailed(id));
                }
                Some(route) => {
                    vehicle.origin_points = points;
                    Self::plan_departure(ctx, &mut vehicle, route);
                    self.departures[i] = vehicle;
                }
            }
        }
    }

    /// Vehicles that have been held up (crawling) for at least `min_slow_time`, up to
    /// `max_count` of them (not buses: they keep to their line).
    pub fn find_delayed(&self, max_count: usize, min_slow_time: f64) -> Vec<VehicleId> {
        self.vehicles
            .values()
            .filter(|v| v.slow_time >= min_slow_time && !v.in_service)
            .take(max_count)
            .map(|v| v.id)
            .collect()
    }

    /// Lets a held-up vehicle look for a better route (with the latest congestion costs).
    /// Skipped if the vehicle is gone or already allowed into a junction (it's committed).
    pub fn reroute_delayed_vehicle(
        &mut self,
        ctx: &mut TrafficCtx,
        id: VehicleId,
        out: &mut Outbox,
    ) {
        let Some(vehicle) = self.vehicles.get_mut(&id) else {
            return;
        };
        if vehicle.granted_connector.is_some() {
            return;
        }
        vehicle.slow_time = 0.0;
        self.reroute_vehicle(ctx, id, out);
    }

    /// Re-routes one vehicle from where it is, fixing up its queue and junction
    /// bookkeeping; a vehicle that can't reach its destination any more fails its trip.
    fn reroute_vehicle(&mut self, ctx: &mut TrafficCtx, id: VehicleId, out: &mut Outbox) {
        let (waited_for, waiting_since) = {
            let vehicle = &self.vehicles[&id];
            (vehicle.requested_connector, vehicle.waiting_since)
        };
        self.stop_waiting(id);
        let old_lane = self.vehicles[&id].lane.expect("on the road");
        let ok = self.reroute(ctx, id);
        // The way in at the end may have changed: work out again whether it turns across
        // the road there, and it is no longer waiting to turn in if it has further to go.
        {
            let vehicle = &mut self.vehicles[&id];
            vehicle.arrival_checked = false;
            if !vehicle.is_on_last_lane() {
                vehicle.driveway_wait_since = f64::INFINITY;
            }
        }
        if let Some(waited_for) = waited_for
            && ok
            && self.vehicles[&id].next_lanes.first() == Some(&waited_for)
            && ctx.roads.lanes.contains_key(&waited_for)
        {
            // Still waiting for the same turn: keep its place (and patience) in the line.
            self.start_waiting(ctx.roads, id, waited_for);
            self.vehicles[&id].waiting_since = waiting_since;
        }
        // (A bus in a bay isn't in any queue: it just takes the rebuilt lane.)
        let current = self.vehicles[&id].lane.expect("on the road");
        if current != old_lane && !self.vehicles[&id].in_bay {
            // Re-snapped onto a rebuilt lane: move it to that lane's queue.
            self.vehicles[&id].lane = Some(old_lane);
            self.dequeue(id);
            if lane(ctx.roads, old_lane).is_connector() {
                self.release_connector(ctx.roads, id, old_lane);
            }
            self.vehicles[&id].lane = Some(current);
            self.enqueue(id);
            if lane(ctx.roads, current).is_connector() {
                self.hold_connector(ctx.roads, id, current);
            }
        }
        // A grant for a junction no longer next on the route (or no longer existing) is
        // void.
        if let Some(granted) = self.vehicles[&id].granted_connector {
            let next = self.vehicles[&id].next_lanes.first().copied();
            if !ok || next != Some(granted) || !ctx.roads.lanes.contains_key(&granted) {
                self.release_grant(ctx.roads, id);
            }
        }
        if !ok {
            self.fail(ctx.roads, id, out);
        }
    }

    /// Ends a vehicle's trip unsuccessfully.
    fn fail(&mut self, roads: &RoadNetwork, id: VehicleId, out: &mut Outbox) {
        self.detach(roads, id);
        let mut vehicle = self
            .vehicles
            .shift_remove(&id)
            .expect("failing a vehicle on the road");
        vehicle.trip_end_time = self.time;
        self.finished.insert(id, vehicle);
        out.push(SimEvent::VehicleFailed(id));
    }

    /// Finds a fresh route to the destination from where the vehicle is now.
    fn reroute(&mut self, ctx: &mut TrafficCtx, id: VehicleId) -> bool {
        let destination = Self::destination(ctx, &self.vehicles[&id]);
        if destination.is_empty() {
            return false;
        }
        let current = self.vehicles[&id].lane.expect("on the road");
        let lane_gone = !ctx.roads.lanes.contains_key(&current);
        if lane_gone && !lane(ctx.roads, current).is_connector() && !self.resnap(ctx.roads, id) {
            return false;
        }
        let current = self.vehicles[&id].lane.expect("on the road");
        if ctx.roads.lanes.contains_key(&current) {
            return self.route_from_current_lane(ctx, id, &destination);
        }
        // On a connector that was just replaced: finish it, then continue from the lane it
        // feeds.
        if lane(ctx.roads, current).is_connector() {
            let exit = lane(ctx.roads, current).outgoing_lane_ids[0];
            if ctx.roads.lanes.contains_key(&exit) {
                let bus = is_bus(&self.vehicles[&id]);
                if let Some(route) = ctx.router.find_best_route(
                    ctx.roads,
                    &[LanePoint::new(exit, 0.0)],
                    &destination,
                    bus,
                ) {
                    let vehicle = &mut self.vehicles[&id];
                    vehicle.next_lanes = route.lane_ids;
                    vehicle.end_distance = route.end_distance;
                    return true;
                }
            }
            // Its exit was rebuilt too: try the new connector now under it.
            if self.resnap(ctx.roads, id) {
                return self.route_from_current_lane(ctx, id, &destination);
            }
        }
        false
    }

    /// Lane positions the vehicle may end its trip at: in front of its destination lot
    /// (either side of the road), else where its route was headed.
    fn destination(ctx: &mut TrafficCtx, vehicle: &Vehicle) -> Vec<LanePoint> {
        if let Some(lot) = vehicle.destination_lot_id {
            return ctx.lots.access_points(ctx.roads, lot);
        }
        match vehicle.destination_lane_id {
            Some(lane_id) if ctx.roads.lanes.contains_key(&lane_id) => {
                vec![LanePoint::new(lane_id, vehicle.destination_distance)]
            }
            _ => Vec::new(),
        }
    }

    fn route_from_current_lane(
        &mut self,
        ctx: &mut TrafficCtx,
        id: VehicleId,
        destination: &[LanePoint],
    ) -> bool {
        let vehicle = &self.vehicles[&id];
        let current = vehicle.lane.expect("on the road");
        let bus = is_bus(vehicle);
        let start = LanePoint::new(current, vehicle.distance);
        let Some(route) = ctx
            .router
            .find_best_route(ctx.roads, &[start], destination, bus)
        else {
            return false;
        };
        let current_lane = lane(ctx.roads, current);
        let starts_with_lane_change =
            route.lane_ids.len() > 1 && current_lane.is_beside(lane(ctx.roads, route.lane_ids[1]));
        if starts_with_lane_change
            && self.vehicles[&id].remaining_on_lane(current_lane) < MERGE_DISTANCE
        {
            // Too late to change lanes here: go on from this lane instead, if possible.
            if let Some(onward) = ctx
                .router
                .find_route_onward(ctx.roads, current, destination, bus)
            {
                let vehicle = &mut self.vehicles[&id];
                vehicle.next_lanes = onward.lane_ids;
                vehicle.end_distance = onward.end_distance;
                return true;
            }
        }
        let vehicle = &mut self.vehicles[&id];
        vehicle.next_lanes = route.lane_ids[1..].to_vec();
        vehicle.end_distance = route.end_distance;
        true
    }

    /// The road under the vehicle was rebuilt (e.g. split by a new crossing): move it onto
    /// whichever lane now runs right under it in the same direction, if any.
    fn resnap(&mut self, roads: &RoadNetwork, id: VehicleId) -> bool {
        let vehicle = &self.vehicles[&id];
        let heading = Vec2::from_angle(vehicle.rotation as f32);
        let position = vehicle.position;
        for candidate in roads.lanes.values() {
            let offset = candidate.curve().closest_offset(position);
            let (point, angle) = candidate.curve().sample_baked_with_rotation(offset);
            if f64::from(point.gd_distance_to(position)) <= RESNAP_DISTANCE
                && Vec2::from_angle(angle).gd_normalized().dot(heading) > 0.9
            {
                let vehicle = &mut self.vehicles[&id];
                vehicle.lane = Some(candidate.id);
                vehicle.distance = f64::from(offset);
                return true;
            }
        }
        false
    }
}

/// Re-exported for the light controller and tests.
pub use vehicle::MAX_LENGTH;
