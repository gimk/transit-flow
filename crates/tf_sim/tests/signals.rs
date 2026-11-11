//! Port of `tests/unit/test_signals.gd` (traffic lights and junction control): the tests
//! that don't need moving vehicles, then those with traffic. Then the
//! light controller on its own, with vehicles placed in queues by hand.

mod common;

use common::roads::{G, changes, end_direction, local};
use glam::Vec2;
use tf_sim::Simulation;
use tf_sim::events::Outbox;
use tf_sim::ids::{LaneId, NodeId};
use tf_sim::roads::{JunctionControl, RoadNetwork};
use tf_sim::traffic::traffic_lights::{self, Light};

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn road(sim: &mut Simulation, from: Vec2, to: Vec2) {
    sim.call(|sim, out| {
        sim.roads.add_road(from * G, to * G, local(), 0, out);
    });
}

fn four_way(sim: &mut Simulation) -> NodeId {
    road(sim, v(0.0, 4.0), v(8.0, 4.0));
    road(sim, v(4.0, 0.0), v(4.0, 8.0));
    sim.roads.find_node_at(v(4.0, 4.0) * G, 0).unwrap()
}

fn set_control(sim: &mut Simulation, node_id: NodeId, control: JunctionControl) -> bool {
    let mut done = false;
    sim.call(|sim, out| done = sim.roads.set_node_control(node_id, control, out));
    done
}

#[test]
fn four_way_phases_follow_the_road_axes() {
    let mut sim = Simulation::new();
    let node_id = four_way(&mut sim);
    for segment_id in &sim.roads.nodes[&node_id].segment_ids {
        for lane_id in &sim.roads.segments[segment_id].lane_ids {
            let lane = &sim.roads.lanes[lane_id];
            if lane.to_node_id == node_id {
                let east_west = end_direction(lane).x.abs() > 0.5;
                assert_eq!(lane.signal_phase, if east_west { 0 } else { 1 });
            }
        }
    }
    assert_eq!(sim.roads.nodes[&node_id].signal_phase_count, 2);
}

#[test]
fn only_junctions_can_be_controlled() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(4.0, 4.0));
    road(&mut sim, v(4.0, 4.0), v(4.0, 8.0));
    let corner = sim.roads.find_node_at(v(4.0, 4.0) * G, 0).unwrap();
    assert!(!set_control(
        &mut sim,
        corner,
        JunctionControl::TrafficLight
    ));
    let node_id = four_way(&mut sim);
    let mut out = Outbox::new();
    assert!(
        sim.roads
            .set_node_control(node_id, JunctionControl::TrafficLight, &mut out)
    );
    assert_eq!(changes(&out).len(), 1);
}

#[test]
fn control_is_saved_and_dropped_when_the_junction_goes_away() {
    let mut sim = Simulation::new();
    let node_id = four_way(&mut sim);
    set_control(&mut sim, node_id, JunctionControl::TrafficLight);
    let loaded =
        RoadNetwork::from_dict(&serde_json::from_str(&sim.roads.to_dict().to_string()).unwrap());
    let loaded_node = loaded.find_node_at(v(4.0, 4.0) * G, 0).unwrap();
    assert_eq!(
        loaded.nodes[&loaded_node].control,
        JunctionControl::TrafficLight
    );
    // Down to a corner: no junction left to control.
    let to_remove: Vec<_> = sim.roads.nodes[&node_id]
        .segment_ids
        .iter()
        .copied()
        .filter(|segment_id| {
            let other = sim.roads.segments[segment_id].other_node_id(node_id);
            let position = sim.roads.nodes[&other].position;
            position.x > 4.0 * G || position.y < 4.0 * G
        })
        .collect();
    for segment_id in to_remove {
        sim.call(|sim, out| {
            sim.roads.remove_segment(segment_id, out);
        });
    }
    assert_eq!(sim.roads.nodes[&node_id].control, JunctionControl::None);
}

fn spawn(sim: &mut Simulation, from: Vec2, to: Vec2, owner: i32) -> Option<tf_sim::ids::VehicleId> {
    common::traffic::spawn(sim, from, to, owner)
}

fn at(x: f32, y: f32, dx: f32, dy: f32) -> Vec2 {
    v(x, y) + v(dx, dy) / G
}

#[test]
fn vehicle_sharing_the_green_does_not_stop() {
    use common::traffic::{lane_of, step_vehicles, vehicle};
    let mut sim = Simulation::new();
    let node_id = four_way(&mut sim);
    set_control(&mut sim, node_id, JunctionControl::TrafficLight);
    let east = spawn(&mut sim, at(1.0, 4.0, 0.0, 7.0), at(7.0, 4.0, 0.0, 7.0), 1).unwrap();
    let mut entry_speed = -1.0;
    for _ in 0..Simulation::TICK_RATE * 6 {
        step_vehicles(&mut sim);
        sim.tick_count += 1;
        if entry_speed < 0.0
            && sim.vehicles.vehicle_any(east).is_some()
            && lane_of(&sim, east).is_connector()
        {
            entry_speed = vehicle(&sim, east).speed;
        }
    }
    assert!(
        entry_speed > 20.0,
        "drove into the junction without stopping"
    );
}

/// Steps the whole simulation (signals included) for `seconds`, calling `each_tick`.
fn run(sim: &mut Simulation, seconds: f64, mut each_tick: impl FnMut(&mut Simulation)) {
    for _ in 0..(seconds * f64::from(Simulation::TICK_RATE)) as i32 {
        sim.step();
        each_tick(sim);
    }
}

fn spawn_eastbound_stream(sim: &mut Simulation, count: i32) {
    for i in 0..count {
        spawn(
            sim,
            at(0.3 + i as f32 * 0.4, 4.0, 0.0, 7.0),
            at(7.5, 4.0, 0.0, 7.0),
            100 + i,
        );
    }
}

#[test]
fn busy_approach_without_competition_keeps_its_green() {
    let mut sim = Simulation::new();
    let node_id = four_way(&mut sim);
    set_control(&mut sim, node_id, JunctionControl::TrafficLight);
    spawn_eastbound_stream(&mut sim, 6);
    let mut saw_red = false;
    run(&mut sim, 20.0, |sim| {
        saw_red = saw_red || sim.signals.light(&sim.roads.nodes[&node_id], 0) != Light::Green;
    });
    assert!(!saw_red, "nobody else waits, so east-west never turns red");
    assert_eq!(
        sim.vehicles.vehicles.len(),
        0,
        "the whole stream got through"
    );
}

#[test]
fn light_turns_for_a_lone_waiting_vehicle() {
    use common::traffic::{lane_of, on_road};
    let mut sim = Simulation::new();
    let node_id = four_way(&mut sim);
    set_control(&mut sim, node_id, JunctionControl::TrafficLight);
    // East-west starts green with nobody on it; one car arrives southbound.
    let south = spawn(
        &mut sim,
        at(4.0, 2.5, -7.0, 0.0),
        at(4.0, 7.0, -7.0, 0.0),
        1,
    )
    .unwrap();
    let mut entered_at = -1.0;
    run(&mut sim, 10.0, |sim| {
        if entered_at < 0.0 && on_road(sim, south) && lane_of(sim, south).is_connector() {
            entered_at = sim.sim_time();
        }
    });
    assert!(
        entered_at >= traffic_lights::MIN_GREEN,
        "waited for the minimum green to end"
    );
    assert!(
        entered_at < traffic_lights::MIN_GREEN + traffic_lights::CLEARANCE_TIME + 2.0,
        "then got green quickly"
    );
}

#[test]
fn continuous_stream_yields_after_max_green() {
    use common::traffic::{lane_of, on_road};
    let mut sim = Simulation::new();
    let node_id = four_way(&mut sim);
    set_control(&mut sim, node_id, JunctionControl::TrafficLight);
    let south = spawn(
        &mut sim,
        at(4.0, 2.5, -7.0, 0.0),
        at(4.0, 7.0, -7.0, 0.0),
        1,
    )
    .unwrap();
    let mut entered_at = -1.0;
    let mut spawned = 0;
    run(&mut sim, 25.0, |sim| {
        // Keep feeding east-west so its approach is never empty.
        if sim.tick_count % 10 == 0
            && spawn(sim, at(0.3, 4.0, 0.0, 7.0), at(7.5, 4.0, 0.0, 7.0), 50).is_some()
        {
            spawned += 1;
        }
        if entered_at < 0.0 && on_road(sim, south) && lane_of(sim, south).is_connector() {
            entered_at = sim.sim_time();
        }
    });
    assert!(spawned > 10, "east-west really was busy");
    assert!(
        entered_at >= traffic_lights::MIN_GREEN,
        "east-west kept its green a while"
    );
    assert!(
        entered_at < traffic_lights::MAX_GREEN + traffic_lights::CLEARANCE_TIME + 2.0,
        "but not past the maximum"
    );
}

#[test]
fn older_saves_with_numeric_controls_still_load() {
    let mut sim = Simulation::new();
    let node_id = four_way(&mut sim);
    set_control(&mut sim, node_id, JunctionControl::TrafficLight);
    let mut data = sim.roads.to_dict();
    let nodes = data["nodes"].as_array_mut().unwrap();
    assert!(
        nodes.iter().any(|n| n["control"] == "traffic_light"),
        "saved by name"
    );
    let junction = nodes.iter().position(|n| n["id"] == node_id.0).unwrap();
    // 1 was a stop sign, 2 traffic lights.
    for (legacy, expected) in [
        (2, JunctionControl::TrafficLight),
        (1, JunctionControl::None),
    ] {
        data["nodes"][junction]["control"] = serde_json::json!(legacy);
        let loaded = RoadNetwork::from_dict(&data);
        assert_eq!(loaded.nodes[&node_id].control, expected);
    }
}

// --- The controller on its own ------------------------------------------------------

/// A four-way junction with lights, and a car standing at the north approach's stop line
/// heading straight through (phase 1, the north-south axis).
fn waiting_southbound(sim: &mut Simulation) -> NodeId {
    let node_id = four_way(sim);
    set_control(sim, node_id, JunctionControl::TrafficLight);
    let roads = &sim.roads;
    let approach = roads
        .lanes
        .values()
        .find(|lane| {
            !lane.is_connector() && lane.to_node_id == node_id && end_direction(lane).y > 0.9
        })
        .unwrap()
        .id;
    let through: LaneId = roads.lanes[&approach]
        .outgoing_lane_ids
        .iter()
        .copied()
        .find(|id| roads.lanes[id].turn == tf_sim::roads::Turn::Straight)
        .unwrap();
    assert_eq!(roads.lanes[&through].signal_phase, 1);
    let length = roads.lanes[&approach].length;
    let exit = roads.lanes[&through].outgoing_lane_ids[0];
    let route =
        tf_sim::routing::Route::new(vec![approach, through, exit], length - 10.0, 20.0, 1.0);
    let id = sim
        .vehicles
        .spawn(
            &sim.roads,
            route,
            None,
            1,
            tf_sim::traffic::vehicle::Kind::Car,
        )
        .unwrap();
    sim.vehicles.vehicles[&id].stopped_time = 5.0;
    node_id
}

fn step_lights(sim: &mut Simulation, seconds: f64) {
    for _ in 0..(seconds * f64::from(Simulation::TICK_RATE)).round() as i32 {
        sim.signals.step(
            Simulation::TICK_DT,
            &sim.roads,
            &sim.vehicles,
            &sim.crosswalks,
        );
    }
}

#[test]
fn lights_rest_on_phase_zero_until_someone_waits_elsewhere() {
    let mut sim = Simulation::new();
    let node_id = four_way(&mut sim);
    set_control(&mut sim, node_id, JunctionControl::TrafficLight);
    step_lights(&mut sim, 30.0);
    let node = &sim.roads.nodes[&node_id];
    assert_eq!(sim.signals.light(node, 0), Light::Green);
    assert_eq!(sim.signals.light(node, 1), Light::Red);
}

#[test]
fn a_waiting_vehicle_gets_green_after_min_green_and_clearance() {
    let mut sim = Simulation::new();
    let node_id = waiting_southbound(&mut sim);
    let light = |sim: &Simulation, phase| sim.signals.light(&sim.roads.nodes[&node_id], phase);
    step_lights(&mut sim, traffic_lights::MIN_GREEN - 0.1);
    assert_eq!(light(&sim, 0), Light::Green, "the minimum green first");
    step_lights(&mut sim, 0.2);
    assert_eq!(light(&sim, 0), Light::Amber, "then amber");
    assert_eq!(light(&sim, 1), Light::Red);
    step_lights(&mut sim, traffic_lights::CLEARANCE_TIME);
    assert_eq!(light(&sim, 1), Light::Green, "then the waiting phase");
    assert!(
        sim.signals
            .recently_ended(&sim.roads.nodes[&node_id], 0, 1.0)
    );
}

#[test]
fn waiting_pedestrians_call_their_walk_phase() {
    let mut sim = Simulation::new();
    let node_id = four_way(&mut sim);
    set_control(&mut sim, node_id, JunctionControl::TrafficLight);
    // Crossing the east-west road walks with the north-south traffic (phase 1).
    let east_west = sim.roads.nodes[&node_id]
        .segment_ids
        .iter()
        .copied()
        .find(|id| {
            let segment = &sim.roads.segments[id];
            let a = sim.roads.nodes[&segment.start_node_id].position;
            let b = sim.roads.nodes[&segment.end_node_id].position;
            (a.y - b.y).abs() < 1.0
        })
        .unwrap();
    let node = sim.roads.nodes[&node_id].clone();
    assert_eq!(
        traffic_lights::TrafficLights::walk_phase(&sim.roads, &node, east_west),
        Some(1)
    );
    assert!(!sim.signals.is_walk_green(&sim.roads, &node, east_west));
    sim.crosswalks
        .add_waiting(glam::IVec2::new(node_id.0, east_west.0));
    step_lights(
        &mut sim,
        traffic_lights::MIN_GREEN + traffic_lights::CLEARANCE_TIME + 0.2,
    );
    assert!(sim.signals.is_walk_green(&sim.roads, &node, east_west));
    step_lights(&mut sim, traffic_lights::WALK_INTERVAL);
    assert!(
        !sim.signals.is_walk_green(&sim.roads, &node, east_west),
        "only early in the green"
    );
}
