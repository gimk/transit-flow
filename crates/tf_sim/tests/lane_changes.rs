//! Port of `tests/unit/test_lane_changes.gd` (Phase 5b: lane choice and lane changing on
//! multi-lane roads).

mod common;

use common::roads::{G, road_type};
use common::traffic::{min_queue_gap, on_road, road, step_vehicles, vehicle};
use glam::Vec2;
use tf_sim::Simulation;
use tf_sim::events::Outbox;
use tf_sim::ids::{LaneId, VehicleId};
use tf_sim::roads::{Lane, Turn, lane_builder};
use tf_sim::routing::{LaneGraph, Route};
use tf_sim::traffic::vehicle::{Kind, MAX_LENGTH};
use tf_sim::traffic::vehicle_system::{LANE_CHANGE_MAX_YAW, MIN_GAP};

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// The eastbound lane of the avenue along y = 4 with the given index (0 = next to the
/// center).
fn eastbound(sim: &Simulation, index: i32, x: f32) -> Lane {
    sim.roads
        .lanes
        .values()
        .find(|lane| {
            if lane.is_connector()
                || lane.index_from_center != index
                || lane_builder::end_direction(lane.curve()).x <= 0.9
            {
                return false;
            }
            let points = lane_builder::get_polyline(lane.curve());
            points[0].x <= x * G
                && points[points.len() - 1].x >= x * G
                && (points[0].y - 4.0 * G).abs() < 30.0
        })
        .unwrap()
        .clone()
}

/// A car in the inner (left) lane of an avenue that must turn right at the next junction.
fn car_needing_to_change(sim: &mut Simulation, from_x: f32) -> VehicleId {
    let lane = eastbound(sim, 0, from_x);
    let end = sim
        .roads
        .find_lane_position((v(8.0, 8.0) + v(-7.0, 0.0) / G) * G, 4.0, 0)
        .unwrap();
    let start = f64::from(from_x * G - lane_builder::get_polyline(lane.curve())[0].x);
    let route = sim
        .router
        .find_route(&sim.roads, lane.id, start, end.lane_id, end.distance, false)
        .expect("test setup: route exists");
    sim.vehicles
        .spawn(&sim.roads, route, None, 1, Kind::Car)
        .unwrap()
}

fn build_avenue_with_side_street(sim: &mut Simulation) {
    road(sim, v(0.0, 4.0), v(12.0, 4.0), road_type("avenue_4lane"));
    road(sim, v(8.0, 4.0), v(8.0, 9.0), road_type("local_2lane"));
}

fn lane(sim: &Simulation, id: LaneId) -> &Lane {
    sim.roads.lane_any(id).unwrap()
}

#[test]
fn route_changes_to_the_lane_its_turn_needs() {
    let mut sim = Simulation::new();
    build_avenue_with_side_street(&mut sim);
    let id = car_needing_to_change(&mut sim, 2.0);
    let car = vehicle(&sim, id);
    let lanes = &car.next_lanes;
    assert!(
        lane(&sim, lanes[0]).is_beside(lane(&sim, car.lane.unwrap())),
        "first, over to the curb lane"
    );
    assert_eq!(lane(&sim, lanes[0]).index_from_center, 1);
    assert_eq!(
        lane(&sim, lanes[1]).turn,
        Turn::Right,
        "then the right turn"
    );
}

#[test]
fn a_lane_change_costs_a_little_not_a_whole_lane() {
    let mut sim = Simulation::new();
    build_avenue_with_side_street(&mut sim);
    let inner = eastbound(&sim, 0, 2.0);
    let curb = eastbound(&sim, 1, 2.0);
    let roads = &sim.roads;
    let straight = sim
        .router
        .find_route(roads, curb.id, 10.0, curb.id, curb.length - 10.0, false)
        .unwrap();
    let changing = sim
        .router
        .find_route(roads, inner.id, 10.0, curb.id, curb.length - 10.0, false)
        .unwrap();
    assert_eq!(changing.lane_ids.len(), 2);
    assert!(
        (changing.travel_time - straight.travel_time - LaneGraph::LANE_CHANGE_COST).abs() <= 0.01
    );
    let free_flow_gap = Route::new(changing.lane_ids.clone(), 10.0, curb.length - 10.0, 0.0)
        .free_flow_time(roads)
        - Route::new(straight.lane_ids.clone(), 10.0, curb.length - 10.0, 0.0)
            .free_flow_time(roads);
    assert!(
        free_flow_gap.abs() <= 0.01,
        "the distance driven is the same"
    );
}

#[test]
fn car_changes_lanes_then_turns_right() {
    let mut sim = Simulation::new();
    build_avenue_with_side_street(&mut sim);
    let id = car_needing_to_change(&mut sim, 2.0);
    let start_lane = vehicle(&sim, id).lane;
    let start_distance = vehicle(&sim, id).distance;
    let mut changed_at = -1.0;
    let mut turned = false;
    for _ in 0..400 {
        step_vehicles(&mut sim);
        if !on_road(&sim, id) {
            break;
        }
        let car = vehicle(&sim, id);
        if changed_at < 0.0 && car.lane != start_lane {
            changed_at = car.distance;
            assert_eq!(
                lane(&sim, car.lane.unwrap()).index_from_center,
                1,
                "now in the curb lane"
            );
            assert_ne!(car.lateral_offset, 0.0, "drawn sliding across");
        }
        turned = turned || lane(&sim, car.lane.unwrap()).turn == Turn::Right;
    }
    assert!(changed_at >= 0.0, "changed lanes");
    assert!(
        changed_at - start_distance < 10.0,
        "right away, on an empty road"
    );
    assert!(turned);
    assert!(!on_road(&sim, id), "arrived");
}

#[test]
fn merging_car_finds_a_gap_in_a_stream() {
    let mut sim = Simulation::new();
    build_avenue_with_side_street(&mut sim);
    // A steady stream along the curb lane, a car starting late in the inner lane that
    // must get over to turn right.
    let curb = eastbound(&sim, 1, 1.0);
    let end = sim
        .roads
        .find_lane_position((v(11.5, 4.0) + v(0.0, 21.0) / G) * G, 4.0, 0)
        .unwrap();
    let mut merging_car = None;
    let mut worst_gap = f64::INFINITY;
    for i in 0..600 {
        if i % 20 == 0
            && sim
                .vehicles
                .can_spawn_at(&sim.roads, curb.id, 5.0, MAX_LENGTH)
            && let Some(route) =
                sim.router
                    .find_route(&sim.roads, curb.id, 5.0, end.lane_id, end.distance, false)
        {
            sim.vehicles.spawn(&sim.roads, route, None, 2, Kind::Car);
        }
        if i == 60 {
            merging_car = Some(car_needing_to_change(&mut sim, 6.0));
        }
        step_vehicles(&mut sim);
        worst_gap = worst_gap.min(min_queue_gap(&sim));
    }
    let id = merging_car.unwrap();
    assert!(!on_road(&sim, id), "got in, turned and arrived");
    assert!(!vehicle(&sim, id).gave_up);
    assert!(
        worst_gap >= MIN_GAP - 0.01,
        "never too close, merging included"
    );
}

#[test]
fn lane_changes_and_offsets_survive_rerouting() {
    let mut sim = Simulation::new();
    build_avenue_with_side_street(&mut sim);
    let id = car_needing_to_change(&mut sim, 2.0);
    sim.call(|sim, out| {
        let (vehicles, mut ctx) = sim.traffic();
        vehicles.reroute_delayed_vehicle(&mut ctx, id, out);
    });
    let car = vehicle(&sim, id);
    assert!(
        lane(&sim, car.next_lanes[0]).is_beside(lane(&sim, car.lane.unwrap())),
        "still plans the lane change"
    );
    for _ in 0..400 {
        step_vehicles(&mut sim);
    }
    assert!(!on_road(&sim, id));
    let _ = Outbox::new();
}

#[test]
fn a_lane_change_is_steered_not_strafed() {
    let mut sim = Simulation::new();
    build_avenue_with_side_street(&mut sim);
    let id = car_needing_to_change(&mut sim, 2.0);
    let start_lane = vehicle(&sim, id).lane;
    let mut steered = 0.0f64;
    let mut last_offset = f64::INFINITY;
    let mut last_distance = 0.0;
    for _ in 0..400 {
        step_vehicles(&mut sim);
        if !on_road(&sim, id) || vehicle(&sim, id).lane == start_lane {
            continue;
        }
        let car = vehicle(&sim, id);
        if car.lateral_offset == 0.0 {
            break;
        }
        // The nose points toward the new lane (offset > 0: right of it, so steering left).
        assert!(
            car.slide_yaw * car.lateral_offset <= 0.0,
            "steering into the lane"
        );
        steered = steered.max(car.slide_yaw.abs());
        if last_offset != f64::INFINITY {
            let across = last_offset.abs() - car.lateral_offset.abs();
            let along = car.distance - last_distance;
            assert!(
                across <= along * 0.35 + 0.01,
                "moves across no faster than a car can steer"
            );
        }
        last_offset = car.lateral_offset;
        last_distance = car.distance;
    }
    assert!(steered > 0.1, "turned its nose noticeably");
    assert!(steered < LANE_CHANGE_MAX_YAW + 0.001);
}
