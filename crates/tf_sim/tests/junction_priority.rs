//! Port of `tests/unit/test_junction_priority.gd` (Phase 5c: junctions with many lanes:
//! parallel turns, road priority, protected lefts).

mod common;

use common::roads::{G, road_type};
use common::traffic::{lane_of, next_lane, on_road, road, spawn, step_vehicles, vehicle};
use glam::Vec2;
use tf_sim::Simulation;
use tf_sim::roads::{JunctionControl, Lane, RoadNetwork, Turn, lane_builder};
use tf_sim::traffic::traffic_lights::Light;
use tf_sim::traffic::vehicle_system::VehicleSystem;

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn at(x: f32, y: f32, dx: f32, dy: f32) -> Vec2 {
    v(x, y) + v(dx, dy) / G
}

fn connectors(sim: &Simulation, point: Vec2) -> Vec<Lane> {
    let node = sim.roads.find_node_at(point * G, 0).unwrap();
    sim.roads.nodes[&node]
        .connector_lane_ids
        .iter()
        .map(|id| sim.roads.lanes[id].clone())
        .collect()
}

#[test]
fn straight_movements_from_both_lanes_run_side_by_side() {
    let mut sim = Simulation::new();
    road(
        &mut sim,
        v(0.0, 4.0),
        v(10.0, 4.0),
        road_type("avenue_4lane"),
    );
    road(
        &mut sim,
        v(5.0, 0.0),
        v(5.0, 8.0),
        road_type("avenue_4lane"),
    );
    let straight: Vec<Lane> = connectors(&sim, v(5.0, 4.0))
        .into_iter()
        .filter(|c| c.turn == Turn::Straight)
        .collect();
    for a in &straight {
        for b in &straight {
            let same_direction = lane_builder::start_direction(a.curve())
                .dot(lane_builder::start_direction(b.curve()))
                > 0.9;
            if a.id != b.id && same_direction {
                assert!(
                    !a.conflicting_lane_ids.contains(&b.id),
                    "parallel lanes don't conflict"
                );
            }
        }
    }
}

#[test]
fn side_street_gives_way_to_the_avenue() {
    let mut sim = Simulation::new();
    road(
        &mut sim,
        v(0.0, 4.0),
        v(12.0, 4.0),
        road_type("avenue_4lane"),
    );
    road(&mut sim, v(6.0, 4.0), v(6.0, 9.0), road_type("local_2lane"));
    // The avenue driver comes along at speed; the side-street driver reaches the junction
    // first, but the avenue driver is less than a gap away, and has priority.
    let main = spawn(
        &mut sim,
        at(0.5, 4.0, 0.0, 21.0),
        at(11.0, 4.0, 0.0, 21.0),
        2,
    )
    .unwrap();
    while vehicle(&sim, main).position.x < 4.3 * G {
        step_vehicles(&mut sim);
    }
    let side = spawn(
        &mut sim,
        at(6.0, 5.2, 7.0, 0.0),
        at(11.0, 4.0, 0.0, 21.0),
        1,
    )
    .unwrap();
    let mut order = Vec::new();
    let mut slowest_main = f64::INFINITY;
    let mut moving = false;
    for _ in 0..600 {
        step_vehicles(&mut sim);
        for id in [side, main] {
            let owner = vehicle(&sim, id).owner_id;
            if on_road(&sim, id) && lane_of(&sim, id).is_connector() && !order.contains(&owner) {
                order.push(owner);
            }
        }
        if on_road(&sim, main) && order.len() < 2 {
            moving = moving || vehicle(&sim, main).speed > 85.0;
            if moving {
                slowest_main = slowest_main.min(vehicle(&sim, main).speed);
            }
        }
    }
    assert_eq!(order, vec![2, 1], "the avenue driver goes first");
    assert!(
        slowest_main > 80.0,
        "and doesn't slow down to look: it has priority"
    );
}

#[test]
fn must_yield_follows_road_priority_first() {
    let mut sim = Simulation::new();
    road(
        &mut sim,
        v(0.0, 4.0),
        v(12.0, 4.0),
        road_type("avenue_4lane"),
    );
    road(&mut sim, v(6.0, 0.0), v(6.0, 9.0), road_type("local_2lane"));
    let mut avenue_left = None;
    let mut side_straight = None;
    for c in connectors(&sim, v(6.0, 4.0)) {
        let feeder = &sim.roads.lanes[&c.feeder_lane_id.unwrap()];
        let from_avenue = sim.roads.segments[&feeder.segment_id.unwrap()]
            .road_type
            .priority
            > 0;
        if from_avenue && c.turn == Turn::Left {
            avenue_left = Some(c.clone());
        }
        if !from_avenue && c.turn == Turn::Straight {
            side_straight = Some(c);
        }
    }
    let (avenue_left, side_straight) = (avenue_left.unwrap(), side_straight.unwrap());
    assert!(
        VehicleSystem::must_yield(&sim.roads, &side_straight, &avenue_left),
        "even a left turn from the avenue goes before the side street"
    );
    assert!(!VehicleSystem::must_yield(
        &sim.roads,
        &avenue_left,
        &side_straight
    ));
}

#[test]
fn protected_left_turns_get_their_own_phase() {
    let mut sim = Simulation::new();
    road(
        &mut sim,
        v(0.0, 4.0),
        v(10.0, 4.0),
        road_type("avenue_4lane"),
    );
    road(
        &mut sim,
        v(5.0, 0.0),
        v(5.0, 8.0),
        road_type("avenue_4lane"),
    );
    let node_id = sim.roads.find_node_at(v(5.0, 4.0) * G, 0).unwrap();
    sim.call(|sim, out| {
        sim.roads
            .set_node_control(node_id, JunctionControl::TrafficLightProtectedLeft, out);
    });
    assert_eq!(sim.roads.nodes[&node_id].signal_phase_count, 4);
    for c in connectors(&sim, v(5.0, 4.0)) {
        let left = c.turn == Turn::Left;
        assert_eq!(
            c.signal_phase % 2,
            if left { 1 } else { 0 },
            "left turns on odd phases"
        );
    }
    // A westbound left turner and an eastbound driver going straight: never green
    // together, and the left turn gets through on its own phase.
    let turning = spawn(&mut sim, at(8.0, 4.0, 0.0, -7.0), at(5.0, 7.0, 7.0, 0.0), 1).unwrap();
    let oncoming = spawn(&mut sim, at(2.0, 4.0, 0.0, 7.0), at(9.0, 4.0, 0.0, 7.0), 2).unwrap();
    assert_eq!(
        next_lane(&sim, turning).turn,
        Turn::Left,
        "test setup: a left turn"
    );
    let mut both_green = false;
    for _ in 0..1200 {
        sim.step();
        let node = &sim.roads.nodes[&node_id];
        if on_road(&sim, turning) && on_road(&sim, oncoming) {
            let (t, o) = (vehicle(&sim, turning), vehicle(&sim, oncoming));
            if let (Some(&tn), Some(&on)) = (t.next_lanes.first(), o.next_lanes.first()) {
                let (tn, on) = (
                    sim.roads.lane_any(tn).unwrap(),
                    sim.roads.lane_any(on).unwrap(),
                );
                if tn.is_connector() && on.is_connector() {
                    both_green = both_green
                        || (sim.signals.light(node, tn.signal_phase) == Light::Green
                            && sim.signals.light(node, on.signal_phase) == Light::Green);
                }
            }
        }
    }
    assert!(
        !both_green,
        "the left turn never shares a green with oncoming traffic"
    );
    assert!(!on_road(&sim, turning), "the left turner got through");
    assert!(!on_road(&sim, oncoming));
}

#[test]
fn protected_left_lights_survive_a_save() {
    let mut sim = Simulation::new();
    road(
        &mut sim,
        v(0.0, 4.0),
        v(10.0, 4.0),
        road_type("avenue_4lane"),
    );
    road(
        &mut sim,
        v(5.0, 0.0),
        v(5.0, 8.0),
        road_type("avenue_4lane"),
    );
    let node_id = sim.roads.find_node_at(v(5.0, 4.0) * G, 0).unwrap();
    sim.call(|sim, out| {
        sim.roads
            .set_node_control(node_id, JunctionControl::TrafficLightProtectedLeft, out);
    });
    let loaded =
        RoadNetwork::from_dict(&serde_json::from_str(&sim.roads.to_dict().to_string()).unwrap());
    let node = &loaded.nodes[&loaded.find_node_at(v(5.0, 4.0) * G, 0).unwrap()];
    assert_eq!(node.control, JunctionControl::TrafficLightProtectedLeft);
    assert_eq!(node.signal_phase_count, 4);
}
