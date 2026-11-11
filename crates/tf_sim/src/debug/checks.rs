//! The shared safety checks of the traffic tests (`tests/traffic_checks.gd`), plus the
//! crosswalk rule of `test_walking.gd`. They live in the library so the baseline runner
//! (`bin/run_baseline.rs`) can use them as well as the tests.

use glam::{IVec2, Vec2};

use crate::Simulation;
use crate::ids::LaneId;
use crate::traffic::vehicle::Vehicle;

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

/// Vehicles on a turn over a crosswalk with people on it: the rule is that nobody is ever
/// on a crosswalk while a car is on a turn over it (`test_walking.gd`).
pub fn crosswalk_violations(sim: &Simulation) -> usize {
    let mut violations = 0;
    for vehicle in sim.vehicles.vehicles.values() {
        let lane = sim.roads.lane_any(vehicle.lane.unwrap()).unwrap();
        if !lane.is_connector() {
            continue;
        }
        let feeder = &sim.roads.lanes[&lane.feeder_lane_id.unwrap()];
        let exit = &sim.roads.lanes[&lane.outgoing_lane_ids[0]];
        for segment_id in [feeder.segment_id.unwrap(), exit.segment_id.unwrap()] {
            if sim
                .crosswalks
                .has_walkers(IVec2::new(lane.from_node_id.0, segment_id.0))
            {
                violations += 1;
            }
        }
    }
    violations
}
