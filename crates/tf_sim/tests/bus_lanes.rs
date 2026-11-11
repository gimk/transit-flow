//! Port of `tests/unit/test_bus_lanes.gd`: bus lanes (the curb lane each way of an
//! "Avenue with Bus Lanes").

mod common;

use std::collections::HashMap;

use common::roads::{G, road_type};
use common::traffic::{connector_conflict_violations, min_queue_gap, road as add_road};
use common::transit::{add_line, add_stop};
use glam::Vec2;
use tf_sim::Simulation;
use tf_sim::roads::{Lane, RoadNetwork, Turn};
use tf_sim::traffic::vehicle::Kind;
use tf_sim::traffic::vehicle_system::MIN_GAP;

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn road(sim: &mut Simulation, from: Vec2, to: Vec2, type_id: &str) {
    add_road(sim, from, to, road_type(type_id));
}

fn lane_at(roads: &RoadNetwork, point: Vec2) -> &Lane {
    let near = roads
        .find_lane_position(point, 4.0, 0)
        .expect("a lane there");
    &roads.lanes[&near.lane_id]
}

#[test]
fn the_curb_lanes_are_bus_lanes() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(12.0, 4.0), "avenue_4lane_bus");
    let segment = sim.roads.segments.values().next().unwrap();
    let mut bus_lanes = 0;
    for lane_id in &segment.lane_ids {
        let lane = &sim.roads.lanes[lane_id];
        assert_eq!(lane.bus_only, lane.index_from_center == 1);
        bus_lanes += i32::from(lane.bus_only);
    }
    assert_eq!(bus_lanes, 2, "one each way");
}

/// Crossing a local street: cars turn right from the lane next to the bus lane; the bus
/// lane goes straight on (into the next bus lane, buses only) or turns right (anyone:
/// cars may use the bus lane to turn right).
#[test]
fn turns_at_a_junction_keep_cars_out_of_bus_lanes() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(12.0, 4.0), "avenue_4lane_bus");
    road(&mut sim, v(6.0, 0.0), v(6.0, 8.0), "local_2lane");
    let roads = &sim.roads;
    let node = &roads.nodes[&roads.find_node_at(v(6.0, 4.0) * G, 0).unwrap()];
    let eastbound_general = lane_at(roads, v(3.0, 4.0) * G + v(0.0, 7.0));
    let eastbound_bus = lane_at(roads, v(3.0, 4.0) * G + v(0.0, 21.0));
    assert!(!eastbound_general.bus_only);
    assert!(eastbound_bus.bus_only);
    let mut general_turns: HashMap<Turn, bool> = HashMap::new();
    for id in &eastbound_general.outgoing_lane_ids {
        let connector = &roads.lanes[id];
        general_turns.insert(connector.turn, connector.bus_only);
    }
    assert_eq!(
        general_turns,
        HashMap::from([
            (Turn::Straight, false),
            (Turn::Right, false),
            (Turn::Left, false)
        ]),
        "the inner lane does everything (one general lane each way)"
    );
    for id in &eastbound_bus.outgoing_lane_ids {
        let connector = &roads.lanes[id];
        assert_eq!(
            connector.bus_only,
            connector.turn != Turn::Right,
            "cars may use the bus lane to turn right"
        );
        assert!(connector.turn == Turn::Straight || connector.turn == Turn::Right);
        if connector.turn == Turn::Straight {
            assert!(
                roads.lanes[&connector.outgoing_lane_ids[0]].bus_only,
                "straight on into the next bus lane"
            );
        }
    }
    for id in &node.connector_lane_ids {
        let connector = &roads.lanes[id];
        let from_bus_lane = roads.lanes[&connector.feeder_lane_id.unwrap()].bus_only
            && connector.turn != Turn::Right;
        assert_eq!(
            connector.bus_only,
            from_bus_lane || roads.lanes[&connector.outgoing_lane_ids[0]].bus_only
        );
    }
}

#[test]
fn cars_route_beside_the_bus_lane_and_buses_along_it() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(20.0, 4.0), "avenue_4lane_bus");
    road(&mut sim, v(10.0, 0.0), v(10.0, 8.0), "local_2lane");
    let from = lane_at(&sim.roads, v(2.0, 4.0) * G + v(0.0, 21.0)).id;
    let to = lane_at(&sim.roads, v(18.0, 4.0) * G + v(0.0, 21.0)).id;
    let car = sim
        .router
        .find_route(&sim.roads, from, 20.0, to, 60.0, false)
        .expect("a car may start and end at the curb...");
    let bus = sim
        .router
        .find_route(&sim.roads, from, 20.0, to, 60.0, true)
        .expect("a bus route");
    for (i, lane_id) in car.lane_ids.iter().enumerate() {
        let lane = &sim.roads.lanes[lane_id];
        if lane.bus_only {
            assert!(
                !lane.is_connector() && (i == 0 || i == car.lane_ids.len() - 1),
                "...but only crosses the bus lane there"
            );
        }
    }
    for lane_id in &bus.lane_ids {
        assert!(
            sim.roads.lanes[lane_id].bus_only,
            "a bus stays in the bus lane"
        );
    }
}

/// Cars and buses together on a bus-lane avenue grid: no car ever takes a turn into or
/// out of a bus lane, and the safety rules hold.
#[test]
fn cars_never_turn_through_bus_lanes() {
    let mut sim = Simulation::new();
    for y in [2.0, 6.0] {
        road(&mut sim, v(0.0, y), v(16.0, y), "avenue_4lane_bus");
    }
    for x in [2.0, 8.0, 14.0] {
        road(&mut sim, v(x, 0.0), v(x, 8.0), "local_2lane");
    }
    let mut stops = Vec::new();
    for x in [5.0, 11.0] {
        stops.push(add_stop(&mut sim, v(x, 2.0) * G + v(0.0, 25.0), false).unwrap());
    }
    for x in [11.0, 5.0] {
        stops.push(add_stop(&mut sim, v(x, 2.0) * G + v(0.0, -25.0), false).unwrap());
    }
    assert!(add_line(&mut sim, &stops).is_some());
    sim.stress.target_count = 60;
    let mut violations = 0;
    let mut worst_gap = f64::INFINITY;
    let mut bus_on_bus_lane = 0;
    let mut bus_samples = 0;
    for _ in 0..Simulation::TICK_RATE * 40 {
        sim.step();
        worst_gap = worst_gap.min(min_queue_gap(&sim));
        violations += connector_conflict_violations(&sim);
        for vehicle in sim.vehicles.vehicles.values() {
            let lane = sim.roads.lane_any(vehicle.lane.unwrap()).unwrap();
            let on_avenue = !lane.is_connector()
                && sim.roads.segments[&lane.segment_id.unwrap()]
                    .road_type
                    .bus_lanes;
            if vehicle.kind == Kind::Bus {
                if on_avenue {
                    bus_samples += 1;
                    bus_on_bus_lane += i32::from(lane.bus_only);
                }
            } else {
                assert!(
                    !(lane.is_connector() && lane.bus_only),
                    "car on a bus-only turn"
                );
            }
        }
    }
    let percent = (100.0 * f64::from(bus_on_bus_lane) / f64::from(bus_samples.max(1))).round();
    // Godot prints "buses in the bus lane 79% of the time" for this run.
    assert_eq!(percent, 79.0, "{bus_on_bus_lane} of {bus_samples} samples");
    assert_eq!(violations, 0);
    assert!(worst_gap >= MIN_GAP - 0.01);
    assert!(
        f64::from(bus_on_bus_lane) > f64::from(bus_samples) * 0.7,
        "on the avenue, buses keep to their lane (out of it only to turn left)"
    );
    assert_eq!(sim.vehicles.stuck_count(30.0), 0);
}

#[test]
#[ignore = "needs citizens, Flow and Scenarios (M7.1, M7.3): test_bus_lanes_on_a_congested_corridor_raise_flow"]
fn bus_lanes_on_a_congested_corridor_raise_flow() {}
