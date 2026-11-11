//! Helpers for tests that drive vehicles, and `tests/traffic_checks.gd` (the shared
//! safety checks: no overlaps, never two vehicles in a shared conflict zone).

use glam::Vec2;
use tf_sim::Simulation;
use tf_sim::events::SimEvent;
use tf_sim::ids::{LaneId, LotId, VehicleId};
use tf_sim::roads::RoadType;
use tf_sim::traffic::vehicle::{Kind, Vehicle};

use super::roads::G;

pub const DT: f64 = Simulation::TICK_DT;

pub fn road(sim: &mut Simulation, from: Vec2, to: Vec2, road_type: &'static RoadType) {
    sim.call(|sim, out| {
        sim.roads.add_road(from * G, to * G, road_type, 0, out);
    });
}

/// `sim.vehicles.step(DT)` in GDScript: the vehicles only (no lights, no citizens). The
/// sim's own clock doesn't move.
pub fn step_vehicles(sim: &mut Simulation) {
    sim.call(|sim, out| {
        let (vehicles, mut ctx) = sim.traffic();
        vehicles.step_begin(DT, &mut ctx, out);
    });
    sim.call(|sim, out| {
        let (vehicles, mut ctx) = sim.traffic();
        vehicles.step_rest(DT, &mut ctx, out);
    });
}

/// Spawns a vehicle between two points that lie on lanes (in grid units).
pub fn spawn(sim: &mut Simulation, from: Vec2, to: Vec2, owner: i32) -> Option<VehicleId> {
    let a = sim
        .roads
        .find_lane_position(from * G, 4.0, 0)
        .expect("test setup: points are on lanes");
    let b = sim
        .roads
        .find_lane_position(to * G, 4.0, 0)
        .expect("test setup: points are on lanes");
    let route = sim
        .router
        .find_route(
            &sim.roads, a.lane_id, a.distance, b.lane_id, b.distance, false,
        )
        .expect("test setup: route exists");
    sim.vehicles
        .spawn(&sim.roads, route, None, owner, Kind::Car)
}

/// Spawns a vehicle from one lot's door to another's (`vehicles.spawn(route, lot, owner)`).
pub fn spawn_between_lots(
    sim: &mut Simulation,
    from: LotId,
    to: LotId,
    owner: i32,
) -> Option<VehicleId> {
    let a = sim
        .lots
        .access(&sim.roads, from)
        .expect("test setup: lot on a road");
    let b = sim
        .lots
        .access(&sim.roads, to)
        .expect("test setup: lot on a road");
    let route = sim
        .router
        .find_route(
            &sim.roads, a.lane_id, a.distance, b.lane_id, b.distance, false,
        )
        .expect("test setup: route exists");
    sim.vehicles
        .spawn(&sim.roads, route, Some(to), owner, Kind::Car)
}

pub fn vehicle(sim: &Simulation, id: VehicleId) -> &Vehicle {
    sim.vehicles
        .vehicle_any(id)
        .expect("a vehicle the test knows")
}

pub fn on_road(sim: &Simulation, id: VehicleId) -> bool {
    sim.vehicles.vehicles.contains_key(&id)
}

pub fn lane_of(sim: &Simulation, id: VehicleId) -> &tf_sim::roads::Lane {
    sim.roads.lane_any(vehicle(sim, id).lane.unwrap()).unwrap()
}

pub fn next_lane(sim: &Simulation, id: VehicleId) -> &tf_sim::roads::Lane {
    sim.roads.lane_any(vehicle(sim, id).next_lanes[0]).unwrap()
}

/// Arrivals and failures among the events delivered since the last call.
pub fn vehicle_events(sim: &mut Simulation) -> (Vec<VehicleId>, Vec<VehicleId>) {
    let mut arrived = Vec::new();
    let mut failed = Vec::new();
    for event in sim.take_view_events() {
        match event {
            SimEvent::VehicleArrived(id) => arrived.push(id),
            SimEvent::VehicleFailed(id) => failed.push(id),
            _ => {}
        }
    }
    (arrived, failed)
}

/// Smallest bumper-to-bumper gap between consecutive vehicles on any lane.
pub fn min_queue_gap(sim: &Simulation) -> f64 {
    let mut smallest = f64::INFINITY;
    let lanes: Vec<LaneId> = sim.vehicles.occupied_lanes().collect();
    for lane_id in lanes {
        let queue = sim.vehicles.queue(lane_id);
        for pair in queue.windows(2) {
            let (a, b) = (
                &sim.vehicles.vehicles[&pair[0]],
                &sim.vehicles.vehicles[&pair[1]],
            );
            smallest = smallest.min(a.distance - b.distance - (a.length + b.length) * 0.5);
        }
    }
    smallest
}

/// Pairs of vehicles that are both inside the stretch two conflicting connectors share.
pub fn connector_conflict_violations(sim: &Simulation) -> usize {
    let on_connectors: Vec<&Vehicle> = sim
        .vehicles
        .vehicles
        .values()
        .filter(|v| sim.roads.lane_any(v.lane.unwrap()).unwrap().is_connector())
        .collect();
    let in_zone = |v: &Vehicle, zone: Vec2| {
        v.distance + v.length * 0.5 >= f64::from(zone.x)
            && v.distance - v.length * 0.5 <= f64::from(zone.y)
    };
    let mut violations = 0;
    for a in &on_connectors {
        for b in &on_connectors {
            let (lane_a, lane_b) = (
                sim.roads.lane_any(a.lane.unwrap()).unwrap(),
                sim.roads.lane_any(b.lane.unwrap()).unwrap(),
            );
            if a.id < b.id
                && let (Some(&zone_a), Some(&zone_b)) = (
                    lane_a.conflict_zones.get(&lane_b.id),
                    lane_b.conflict_zones.get(&lane_a.id),
                )
                && in_zone(a, zone_a)
                && in_zone(b, zone_b)
            {
                violations += 1;
            }
        }
    }
    violations
}
