//! Port of `tests/unit/test_vehicles.gd`: vehicle motion along routes, driven tick by
//! tick.

mod common;

use common::roads::{G, local};
use common::traffic::{
    DT, lane_of, on_road, road, spawn_between_lots, step_vehicles, vehicle, vehicle_events,
};
use glam::{IVec2, Vec2};
use tf_sim::Simulation;
use tf_sim::city::LotType;
use tf_sim::ids::{SegmentId, VehicleId};
use tf_sim::roads::{RoadNetwork, Turn};
use tf_sim::traffic::vehicle_system::DECELERATION;

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn road_id(sim: &mut Simulation, from: Vec2, to: Vec2) -> SegmentId {
    let mut id = None;
    sim.call(|sim, out| id = sim.roads.add_road(from * G, to * G, local(), 0, out));
    id.unwrap()
}

/// Spawns a vehicle heading for the access point of a lot at `destination_cell`.
fn spawn_to_lot(sim: &mut Simulation, origin_cell: IVec2, destination_cell: IVec2) -> VehicleId {
    let mut lots = (None, None);
    sim.call(|sim, out| {
        lots.0 = sim
            .lots
            .add_lot(&sim.roads, origin_cell, LotType::Residential, out);
        lots.1 = sim
            .lots
            .add_lot(&sim.roads, destination_cell, LotType::Industrial, out);
    });
    spawn_between_lots(sim, lots.0.unwrap(), lots.1.unwrap(), 7).expect("spawned")
}

/// Steps until the vehicle arrives or fails; returns the number of ticks, or `None` on
/// timeout.
fn run_until_gone(
    sim: &mut Simulation,
    id: VehicleId,
    max_ticks: usize,
    mut on_tick: impl FnMut(&Simulation),
) -> Option<usize> {
    for tick in 0..max_ticks {
        step_vehicles(sim);
        if !on_road(sim, id) {
            return Some(tick);
        }
        on_tick(sim);
    }
    None
}

#[test]
fn vehicle_drives_around_a_corner_and_arrives() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(6.0, 4.0), local());
    road(&mut sim, v(6.0, 4.0), v(6.0, 10.0), local());
    let id = spawn_to_lot(&mut sim, IVec2::new(1, 4), IVec2::new(6, 8));
    sim.take_view_events();
    assert!(
        run_until_gone(&mut sim, id, 4000, |_| {}).is_some(),
        "arrived within the time limit"
    );
    assert_eq!(vehicle_events(&mut sim).0, vec![id]);
    // It stops in front of the lot on either side of the street (turning across the road
    // costs a wait, so it may prefer the near side via the dead end).
    let car = vehicle(&sim, id).clone();
    let doors = sim
        .lots
        .access_points(&sim.roads, car.destination_lot_id.unwrap());
    let stopped_at_a_door = doors
        .iter()
        .any(|door| car.lane == Some(door.lane_id) && (car.distance - door.distance).abs() < 0.001);
    assert!(stopped_at_a_door);
}

#[test]
fn motion_is_continuous_and_respects_speed_limits() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(6.0, 4.0), local());
    road(&mut sim, v(6.0, 4.0), v(6.0, 10.0), local());
    let id = spawn_to_lot(&mut sim, IVec2::new(1, 4), IVec2::new(6, 8));
    let (mut max_jump, mut over_limit, mut slowest_turn_speed) =
        (0.0f32, f64::NEG_INFINITY, f64::INFINITY);
    run_until_gone(&mut sim, id, 4000, |sim| {
        let car = vehicle(sim, id);
        let lane = lane_of(sim, id);
        max_jump = max_jump.max(car.position.distance(car.previous_position));
        over_limit = over_limit.max(car.speed - lane.speed_limit);
        if lane.turn == Turn::Right {
            slowest_turn_speed = slowest_turn_speed.min(car.speed);
        }
    });
    let fastest = local().speed_limit;
    assert!(
        f64::from(max_jump) < fastest * DT + 1.0,
        "no teleporting between ticks"
    );
    assert!(
        over_limit < DECELERATION * DT + 1.0,
        "at most one tick of braking above a lane limit"
    );
    assert!(
        slowest_turn_speed < f64::INFINITY,
        "drove through the right-turn connector"
    );
}

#[test]
fn vehicle_brakes_before_the_turn() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(8.0, 4.0), local());
    road(&mut sim, v(8.0, 4.0), v(8.0, 10.0), local());
    let id = spawn_to_lot(&mut sim, IVec2::new(0, 4), IVec2::new(8, 8));
    let mut entry_speed = -1.0;
    run_until_gone(&mut sim, id, 4000, |sim| {
        if entry_speed < 0.0 && lane_of(sim, id).turn == Turn::Right {
            entry_speed = vehicle(sim, id).speed;
        }
    });
    let turn_limit = local().speed_limit * RoadNetwork::turn_speed_factor(Turn::Right);
    assert!(
        (0.0..=turn_limit + DECELERATION * DT + 1.0).contains(&entry_speed),
        "entered the turn at turn speed ({entry_speed})"
    );
}

#[test]
fn reroutes_when_a_road_on_the_route_is_bulldozed() {
    // Two ways from the corner (4,4) to the corner (10,4): direct along y=4, or around
    // via y=8.
    let mut sim = Simulation::new();
    road(&mut sim, v(1.0, 4.0), v(4.0, 4.0), local());
    let direct = road_id(&mut sim, v(4.0, 4.0), v(10.0, 4.0));
    road(&mut sim, v(4.0, 4.0), v(4.0, 8.0), local());
    road(&mut sim, v(4.0, 8.0), v(10.0, 8.0), local());
    road(&mut sim, v(10.0, 8.0), v(10.0, 4.0), local());
    road(&mut sim, v(10.0, 4.0), v(13.0, 4.0), local());
    let id = spawn_to_lot(&mut sim, IVec2::new(1, 4), IVec2::new(12, 4));
    for _ in 0..10 {
        step_vehicles(&mut sim);
    }
    sim.call(|sim, out| {
        sim.roads.remove_segment(direct, out);
    });
    sim.take_view_events();
    assert!(run_until_gone(&mut sim, id, 4000, |_| {}).is_some());
    let (arrived, failed) = vehicle_events(&mut sim);
    assert_eq!(arrived, vec![id]);
    assert!(failed.is_empty());
}

#[test]
fn fails_cleanly_when_no_route_remains() {
    let mut sim = Simulation::new();
    let only = road_id(&mut sim, v(0.0, 4.0), v(8.0, 4.0));
    let id = spawn_to_lot(&mut sim, IVec2::new(1, 4), IVec2::new(6, 4));
    step_vehicles(&mut sim);
    sim.call(|sim, out| {
        sim.roads.remove_segment(only, out);
    });
    sim.take_view_events();
    step_vehicles(&mut sim);
    assert_eq!(vehicle_events(&mut sim).1, vec![id]);
    assert_eq!(sim.vehicles.vehicles.len(), 0);
}

#[test]
fn finishes_a_connector_that_was_rebuilt_under_it() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(6.0, 4.0), local());
    road(&mut sim, v(6.0, 4.0), v(6.0, 10.0), local());
    let id = spawn_to_lot(&mut sim, IVec2::new(1, 4), IVec2::new(6, 8));
    while !lane_of(&sim, id).is_connector() {
        step_vehicles(&mut sim);
    }
    // Building another road at the same junction regenerates its connectors.
    road(&mut sim, v(6.0, 4.0), v(10.0, 4.0), local());
    let lane = vehicle(&sim, id).lane.unwrap();
    assert!(
        !sim.roads.lanes.contains_key(&lane),
        "the connector it is on was replaced"
    );
    sim.take_view_events();
    assert!(run_until_gone(&mut sim, id, 4000, |_| {}).is_some());
    assert_eq!(vehicle_events(&mut sim).0, vec![id]);
}

#[test]
fn keeps_driving_when_its_road_is_split_by_a_new_crossing() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(10.0, 4.0), local());
    let id = spawn_to_lot(&mut sim, IVec2::new(0, 4), IVec2::new(9, 4));
    for _ in 0..20 {
        step_vehicles(&mut sim);
    }
    let lane_before = vehicle(&sim, id).lane.unwrap();
    let position_before = vehicle(&sim, id).position;
    // Cross ahead of the car, and also right where the car is.
    road(&mut sim, v(6.0, 0.0), v(6.0, 8.0), local());
    let x = (position_before.x / G).floor();
    road(&mut sim, v(x, 0.0), v(x, 8.0), local());
    assert!(
        !sim.roads.lanes.contains_key(&lane_before),
        "the lane it was on was replaced"
    );
    sim.take_view_events();
    step_vehicles(&mut sim);
    assert!(vehicle_events(&mut sim).1.is_empty());
    assert!(
        vehicle(&sim, id).position.distance(position_before) < 10.0,
        "no jump"
    );
    assert!(run_until_gone(&mut sim, id, 4000, |_| {}).is_some());
    assert_eq!(vehicle_events(&mut sim).0, vec![id]);
}
