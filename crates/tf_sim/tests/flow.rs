//! Port of `tests/unit/test_flow.gd` (Phase 3c: patience, failed trips and the Flow
//! score).

mod common;

use common::roads::{G, local};
use common::traffic::{road as add_road, spawn, step_vehicles, vehicle, vehicle_events};
use glam::Vec2;
use tf_sim::Simulation;
use tf_sim::city::FlowMetric;
use tf_sim::ids::VehicleId;
use tf_sim::traffic::stress_traffic;
use tf_sim::traffic::vehicle::Vehicle;
use tf_sim::traffic::vehicle_system::{
    GIVE_UP_FACTOR, GIVE_UP_GRACE, IMPATIENT_FACTOR, IMPATIENT_GRACE,
};

fn road(sim: &mut Simulation, from: Vec2, to: Vec2) {
    add_road(sim, from, to, local());
}

/// A vehicle between two points in grid units.
fn spawn_trip(sim: &mut Simulation, from: Vec2, to: Vec2) -> VehicleId {
    spawn(sim, from, to, 1).expect("test setup: room to spawn")
}

fn trip(start: f64, ideal: f64, end: f64, owner: i32) -> Vehicle {
    Vehicle {
        owner_id: owner,
        trip_start_time: start,
        ideal_trip_time: ideal,
        trip_end_time: end,
        ..Vehicle::default()
    }
}

#[test]
fn score_is_ideal_over_actual_capped_at_one() {
    assert_eq!(FlowMetric::score(&trip(0.0, 10.0, 10.0, 1), true), 1.0);
    assert_eq!(
        FlowMetric::score(&trip(0.0, 10.0, 8.0, 1), true),
        1.0,
        "faster than ideal still caps at 1"
    );
    assert!((FlowMetric::score(&trip(0.0, 10.0, 40.0, 1), true) - 0.25).abs() < 0.0001);
    assert_eq!(
        FlowMetric::score(&trip(0.0, 10.0, 12.0, 1), false),
        0.0,
        "failed trips score 0"
    );
}

#[test]
fn flow_averages_trips_of_the_last_day() {
    let mut sim = Simulation::new();
    assert_eq!(sim.flow.flow_percent(0.0), -1.0, "no data yet");
    sim.flow.on_vehicle_arrived(&trip(0.0, 10.0, 10.0, 1));
    sim.flow.on_vehicle_arrived(&trip(0.0, 10.0, 20.0, 1));
    sim.flow.on_vehicle_failed(&trip(0.0, 10.0, 30.0, 1));
    assert!((sim.flow.flow_percent(30.0) - 50.0).abs() < 0.001);
    assert_eq!(sim.flow.trip_count(30.0), 3);
    // A day later, only newer trips count.
    sim.flow.on_vehicle_arrived(&trip(400.0, 10.0, 410.0, 1));
    assert!((sim.flow.flow_percent(410.0) - 100.0).abs() < 0.001);
    assert_eq!(sim.flow.trip_count(410.0), 1);
}

#[test]
fn stress_traffic_is_not_counted() {
    let mut sim = Simulation::new();
    sim.flow
        .on_vehicle_failed(&trip(0.0, 10.0, 30.0, stress_traffic::OWNER_ID));
    assert_eq!(sim.flow.trip_count(30.0), 0);
}

#[test]
fn free_flow_trip_scores_near_full_marks() {
    let mut sim = Simulation::new();
    road(&mut sim, Vec2::new(0.0, 4.0), Vec2::new(12.0, 4.0));
    let id = spawn_trip(
        &mut sim,
        Vec2::new(1.0, 4.0) + Vec2::new(0.0, 7.0) / G,
        Vec2::new(11.0, 4.0) + Vec2::new(0.0, 7.0) / G,
    );
    assert!(vehicle(&sim, id).ideal_trip_time > 0.0);
    while sim.vehicles.vehicles.contains_key(&id) {
        step_vehicles(&mut sim);
    }
    assert!(
        FlowMetric::score(vehicle(&sim, id), true) > 0.9,
        "an empty road is close to ideal"
    );
    assert!(!vehicle(&sim, id).impatient);
}

#[test]
fn late_drivers_turn_impatient_then_give_up() {
    let mut sim = Simulation::new();
    road(&mut sim, Vec2::new(0.0, 4.0), Vec2::new(12.0, 4.0));
    let id = spawn_trip(
        &mut sim,
        Vec2::new(1.0, 4.0) + Vec2::new(0.0, 7.0) / G,
        Vec2::new(11.0, 4.0) + Vec2::new(0.0, 7.0) / G,
    );
    let ideal = vehicle(&sim, id).ideal_trip_time;
    // Pretend the trip has dragged on.
    sim.vehicles.vehicles[&id].trip_start_time =
        -(ideal * IMPATIENT_FACTOR + IMPATIENT_GRACE + 1.0);
    step_vehicles(&mut sim);
    assert!(vehicle(&sim, id).impatient);
    assert!(sim.vehicles.vehicles.contains_key(&id), "still driving");
    sim.vehicles.vehicles[&id].trip_start_time = -(ideal * GIVE_UP_FACTOR + GIVE_UP_GRACE + 1.0);
    vehicle_events(&mut sim);
    step_vehicles(&mut sim);
    assert!(vehicle(&sim, id).gave_up);
    assert!(!sim.vehicles.vehicles.contains_key(&id));
    let (_, failed) = vehicle_events(&mut sim);
    assert_eq!(failed, vec![id], "vehicle_failed emitted with the vehicle");
}

/// Flow reacts to the network: cutting the road the commuters need mid-morning makes
/// their trips fail, and Flow drops.
#[test]
#[ignore = "needs Scenarios (M7.3): test_flow_drops_when_the_commute_road_is_cut"]
fn flow_drops_when_the_commute_road_is_cut() {}
