//! Port of `tests/unit/test_traffic.gd` (Phase 2b): many vehicles at once: following,
//! queuing and junction rules, with the shared safety checks.

mod common;

use common::roads::{G, local};
use common::traffic::{
    DT, connector_conflict_violations, lane_of, min_queue_gap, next_lane, on_road, road, spawn,
    step_vehicles, vehicle,
};
use glam::Vec2;
use tf_sim::Simulation;
use tf_sim::ids::VehicleId;
use tf_sim::roads::{JunctionControl, Turn};
use tf_sim::traffic::vehicle_system::{LOOK_SPEED_FRACTION, MIN_GAP};

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// A lane point, in grid units, nudged onto a lane (`Vector2(0, 7) / G`).
fn at(x: f32, y: f32, dx: f32, dy: f32) -> Vec2 {
    v(x, y) + v(dx, dy) / G
}

/// A grid of `count` x `count` crossing roads, `spacing` cells apart.
fn build_grid(sim: &mut Simulation, count: i32, spacing: i32) {
    let extent = ((count - 1) * spacing) as f32;
    for i in 0..count {
        let line = (1 + i * spacing) as f32;
        road(sim, v(1.0, line), v(1.0 + extent, line), local());
        road(sim, v(line, 1.0), v(line, 1.0 + extent), local());
    }
}

/// The order in which the vehicles first drove onto a connector (by owner).
fn junction_order(sim: &mut Simulation, vehicles: &[VehicleId], ticks: usize) -> Vec<i32> {
    let mut order = Vec::new();
    for _ in 0..ticks {
        step_vehicles(sim);
        for &id in vehicles {
            let owner = vehicle(sim, id).owner_id;
            if lane_of(sim, id).is_connector() && !order.contains(&owner) {
                order.push(owner);
            }
        }
    }
    order
}

#[test]
fn follower_queues_behind_a_stopping_leader() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 2.0), v(10.0, 2.0), local());
    // Both stop at the same point; the follower must wait behind the leader.
    let leader = spawn(&mut sim, at(3.0, 2.0, 0.0, 7.0), at(9.0, 2.0, 0.0, 7.0), 1).unwrap();
    let follower = spawn(&mut sim, at(1.0, 2.0, 0.0, 7.0), at(9.0, 2.0, 0.0, 7.0), 2).unwrap();
    let mut smallest = f64::INFINITY;
    for _ in 0..400 {
        step_vehicles(&mut sim);
        if on_road(&sim, leader) && on_road(&sim, follower) {
            let (a, b) = (vehicle(&sim, leader), vehicle(&sim, follower));
            smallest = smallest.min(a.distance - b.distance - (a.length + b.length) * 0.5);
        }
    }
    assert!(
        smallest >= MIN_GAP - 0.01,
        "never closer than the minimum gap"
    );
    assert_eq!(sim.vehicles.vehicles.len(), 0, "both eventually arrive");
}

#[test]
fn cannot_spawn_on_top_of_another_vehicle() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 2.0), v(10.0, 2.0), local());
    let first = spawn(&mut sim, at(3.0, 2.0, 0.0, 7.0), at(9.0, 2.0, 0.0, 7.0), 1).unwrap();
    let (lane, distance) = (
        vehicle(&sim, first).lane.unwrap(),
        vehicle(&sim, first).distance,
    );
    let max = tf_sim::traffic::vehicle::MAX_LENGTH;
    assert!(
        !sim.vehicles
            .can_spawn_at(&sim.roads, lane, distance + 5.0, max)
    );
    assert!(
        sim.vehicles
            .can_spawn_at(&sim.roads, lane, distance + 40.0, max)
    );
    assert!(spawn(&mut sim, at(3.0, 2.0, 5.0, 7.0), at(9.0, 2.0, 0.0, 7.0), 1).is_none());
}

#[test]
fn crossing_movements_take_turns() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(8.0, 4.0), local());
    road(&mut sim, v(4.0, 0.0), v(4.0, 8.0), local());
    // Eastbound and southbound, same distance from the junction: they'd collide.
    let east = spawn(&mut sim, at(1.0, 4.0, 0.0, 7.0), at(7.0, 4.0, 0.0, 7.0), 1).unwrap();
    let south = spawn(
        &mut sim,
        at(4.0, 1.0, -7.0, 0.0),
        at(4.0, 7.0, -7.0, 0.0),
        2,
    )
    .unwrap();
    let mut someone_waited = false;
    let mut both_inside = 0;
    for _ in 0..600 {
        step_vehicles(&mut sim);
        someone_waited = someone_waited
            || vehicle(&sim, east).requested_connector.is_some()
            || vehicle(&sim, south).requested_connector.is_some();
        both_inside += connector_conflict_violations(&sim);
    }
    assert!(someone_waited, "one of them had to wait at the stop line");
    assert_eq!(both_inside, 0, "never both where their paths cross");
    assert_eq!(sim.vehicles.vehicles.len(), 0, "both got through");
}

#[test]
fn first_to_arrive_goes_first() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(8.0, 4.0), local());
    road(&mut sim, v(4.0, 0.0), v(4.0, 8.0), local());
    let east = spawn(&mut sim, at(2.0, 4.0, 0.0, 7.0), at(7.0, 4.0, 0.0, 7.0), 1).unwrap();
    let south = spawn(
        &mut sim,
        at(4.0, 0.5, -7.0, 0.0),
        at(4.0, 7.0, -7.0, 0.0),
        2,
    )
    .unwrap();
    assert_eq!(
        junction_order(&mut sim, &[east, south], 600),
        vec![1, 2],
        "the closer vehicle crosses first"
    );
}

/// Lowest speed a vehicle reaches while driving its whole trip alone.
fn slowest_speed_on_trip(sim: &mut Simulation, id: VehicleId) -> f64 {
    let mut slowest = f64::INFINITY;
    let mut moving = false;
    while on_road(sim, id) {
        sim.step(); // Lights too.
        if !on_road(sim, id) {
            break;
        }
        let car = vehicle(sim, id);
        moving = moving || car.speed > 85.0; // Up to cruising speed.
        if moving && !car.next_lanes.is_empty() {
            slowest = slowest.min(car.speed);
        }
    }
    slowest
}

#[test]
fn drivers_slow_down_to_look_at_junctions_without_lights() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(12.0, 4.0), local());
    road(&mut sim, v(6.0, 0.0), v(6.0, 8.0), local());
    let junction = sim.roads.find_node_at(v(6.0, 4.0) * G, 0).unwrap();
    let through = spawn(&mut sim, at(1.0, 4.0, 0.0, 7.0), at(11.0, 4.0, 0.0, 7.0), 1).unwrap();
    let slowest = slowest_speed_on_trip(&mut sim, through);
    assert!(
        (slowest - 90.0 * LOOK_SPEED_FRACTION).abs() <= 3.0,
        "eases off to look, even with nobody around ({slowest})"
    );
    sim.call(|sim, out| {
        sim.roads
            .set_node_control(junction, JunctionControl::TrafficLight, out);
    });
    let through = spawn(&mut sim, at(1.0, 4.0, 0.0, 7.0), at(11.0, 4.0, 0.0, 7.0), 1).unwrap();
    assert!(
        slowest_speed_on_trip(&mut sim, through) > 80.0,
        "a green light needs no looking"
    );
}

#[test]
fn left_turn_waits_for_a_gap_in_oncoming_traffic() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(10.0, 4.0), local());
    road(&mut sim, v(5.0, 0.0), v(5.0, 8.0), local());
    // The westbound driver turning left (south) is nearer the junction, but the oncoming
    // eastbound driver going straight has priority and is close: the turn waits for it.
    let turning = spawn(&mut sim, at(6.6, 4.0, 0.0, -7.0), at(5.0, 7.0, 7.0, 0.0), 1).unwrap();
    let oncoming = spawn(&mut sim, at(2.6, 4.0, 0.0, 7.0), at(9.0, 4.0, 0.0, 7.0), 2).unwrap();
    assert_eq!(
        next_lane(&sim, turning).turn,
        Turn::Left,
        "test setup: a left turn"
    );
    assert_eq!(
        junction_order(&mut sim, &[turning, oncoming], 600),
        vec![2, 1],
        "oncoming traffic first, then the left turn"
    );
}

#[test]
fn priority_to_the_right_between_crossing_drivers() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(8.0, 4.0), local());
    road(&mut sim, v(4.0, 0.0), v(4.0, 8.0), local());
    // The southbound driver is nearer, but the eastbound one comes from its right.
    let south = spawn(
        &mut sim,
        at(4.0, 1.9, -7.0, 0.0),
        at(4.0, 7.0, -7.0, 0.0),
        1,
    )
    .unwrap();
    let east = spawn(&mut sim, at(1.5, 4.0, 0.0, 7.0), at(7.0, 4.0, 0.0, 7.0), 2).unwrap();
    assert_eq!(
        junction_order(&mut sim, &[south, east], 600),
        vec![2, 1],
        "the driver from the right goes first"
    );
}

/// 250 random trips on a 5 x 5 street grid for 2 sim minutes, with every junction under
/// `control`, checking the safety rules on every tick.
fn stress_grid(control: JunctionControl, godot: (usize, usize)) -> (usize, usize) {
    let mut sim = Simulation::new();
    build_grid(&mut sim, 5, 3);
    let junctions: Vec<_> = sim
        .roads
        .nodes
        .values()
        .filter(|n| n.degree() >= 3)
        .map(|n| n.id)
        .collect();
    for node in junctions {
        sim.call(|sim, out| {
            sim.roads.set_node_control(node, control, out);
        });
    }
    sim.stress.target_count = 250;
    let mut arrived = 0;
    let mut worst_gap = f64::INFINITY;
    let mut violations = 0;
    let mut longest_in_box = 0.0f64;
    for _ in 0..Simulation::TICK_RATE * 120 {
        sim.step();
        arrived += common::traffic::vehicle_events(&mut sim).0.len();
        worst_gap = worst_gap.min(min_queue_gap(&sim));
        violations += connector_conflict_violations(&sim);
        for car in sim.vehicles.vehicles.values() {
            if sim
                .roads
                .lane_any(car.lane.unwrap())
                .unwrap()
                .is_connector()
            {
                longest_in_box = longest_in_box.max(car.stopped_time);
            }
        }
    }
    println!(
        "[{control:?}] vehicles {}, arrivals {arrived}, stuck {}, smallest gap {worst_gap:.2}, longest stop in a junction {longest_in_box:.1} s",
        sim.vehicles.vehicles.len(),
        sim.vehicles.stuck_count(30.0)
    );
    assert!(sim.vehicles.vehicles.len() > 200, "traffic ramped up");
    assert!(worst_gap >= MIN_GAP - 0.01, "no overlaps in any queue");
    assert_eq!(
        violations, 0,
        "never two vehicles where conflicting paths meet"
    );
    assert!(
        longest_in_box < 10.0,
        "nobody gets stuck inside a junction (don't block the box)"
    );
    assert!(arrived > 250, "traffic keeps flowing");
    assert_eq!(sim.vehicles.stuck_count(30.0), 0, "no permanent deadlock");
    assert_eq!(
        (arrived, sim.vehicles.vehicles.len()),
        godot,
        "the same arrivals and vehicles as Godot (`migration/baseline.md`)"
    );
    let _ = DT;
    (arrived, sim.vehicles.vehicles.len())
}

#[test]
fn stress_grid_never_overlaps_or_deadlocks() {
    stress_grid(JunctionControl::None, (507, 250));
}

#[test]
fn stress_grid_with_traffic_lights() {
    stress_grid(JunctionControl::TrafficLight, (622, 249));
}
