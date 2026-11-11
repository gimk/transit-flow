//! Port of `tests/unit/test_road_types.gd` (Phase 5a: avenues, one-way roads, upgrading
//! roads in place), plus a check that every road type has exactly the values of
//! `data/road_types/*.tres`. Tests that need routing, lots or traffic come with the
//! work packages that port those systems.

mod common;

use common::roads::{G, end_direction, lanes_of, pieces, road_type};
use glam::Vec2;
use tf_sim::roads::{RoadNetwork, RoadType, Turn};

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

#[test]
fn palette_order_starts_with_the_local_street() {
    let ids: Vec<&str> = RoadType::list_all().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "local_2lane",
            "avenue_4lane",
            "avenue_4lane_bus",
            "one_way_1lane",
            "one_way_2lane",
            "one_way_3lane"
        ]
    );
}

#[test]
fn every_field_matches_the_tres_files() {
    // (id, name, order, lanes, one way, priority, bus lanes, speed, surface, edge)
    let dark = [0.255, 0.294, 0.341, 1.0];
    let light = [0.29, 0.333, 0.384, 1.0];
    let edge = [0.169, 0.196, 0.231, 1.0];
    let expected = [
        (
            "local_2lane",
            "Local Street",
            0,
            1,
            false,
            0,
            false,
            90.0,
            light,
        ),
        ("avenue_4lane", "Avenue", 1, 2, false, 1, false, 100.0, dark),
        (
            "avenue_4lane_bus",
            "Avenue with Bus Lanes",
            2,
            2,
            false,
            1,
            true,
            100.0,
            dark,
        ),
        (
            "one_way_1lane",
            "One-way Street",
            3,
            1,
            true,
            0,
            false,
            90.0,
            light,
        ),
        (
            "one_way_2lane",
            "One-way, 2 lanes",
            4,
            2,
            true,
            1,
            false,
            90.0,
            light,
        ),
        (
            "one_way_3lane",
            "One-way, 3 lanes",
            5,
            3,
            true,
            1,
            false,
            100.0,
            dark,
        ),
    ];
    for (id, name, order, lanes, one_way, priority, bus, speed, surface) in expected {
        let t = RoadType::load_by_id(id).unwrap();
        assert_eq!(t.display_name, name);
        assert_eq!(t.palette_order, order);
        assert_eq!(t.lanes_per_direction, lanes);
        assert_eq!(t.one_way, one_way);
        assert_eq!(t.priority, priority);
        assert_eq!(t.bus_lanes, bus);
        assert_eq!(t.lane_width, 14.0);
        assert_eq!(t.speed_limit, speed);
        assert_eq!(t.surface_color, surface);
        assert_eq!(t.edge_color, edge);
    }
    assert!(RoadType::load_by_id("nope").is_none());
}

#[test]
fn widths_and_bus_lanes() {
    let avenue = RoadType::load_by_id("avenue_4lane_bus").unwrap();
    assert_eq!(avenue.lane_count(), 4);
    assert_eq!(avenue.total_width(), 56.0);
    assert!(avenue.is_bus_lane(1) && !avenue.is_bus_lane(0));
    assert_eq!(avenue.general_lane_count(), 1);
    let one_way = RoadType::load_by_id("one_way_3lane").unwrap();
    assert_eq!(one_way.lane_count(), 3);
    assert_eq!(one_way.general_lane_count(), 3);
}

#[test]
fn avenue_has_two_lanes_each_way() {
    let mut roads = RoadNetwork::new();
    let id = pieces(
        &mut roads,
        v(1.0, 2.0),
        v(6.0, 2.0),
        road_type("avenue_4lane"),
    )[0];
    let lanes = lanes_of(&roads, id);
    assert_eq!(lanes.len(), 4);
    assert_eq!(lanes.iter().filter(|l| end_direction(l).x > 0.0).count(), 2);
    assert_eq!(road_type("avenue_4lane").total_width(), 56.0);
}

#[test]
fn one_way_lanes_all_run_the_way_it_was_dragged() {
    let mut roads = RoadNetwork::new();
    let east = pieces(
        &mut roads,
        v(1.0, 2.0),
        v(6.0, 2.0),
        road_type("one_way_3lane"),
    )[0];
    let west = pieces(
        &mut roads,
        v(6.0, 5.0),
        v(1.0, 5.0),
        road_type("one_way_3lane"),
    )[0];
    for lane in lanes_of(&roads, east) {
        assert!(end_direction(lane).x > 0.9, "eastbound");
    }
    for lane in lanes_of(&roads, west) {
        assert!(end_direction(lane).x < -0.9, "westbound");
    }
    // Centered on the road, index 0 on the left of the direction of travel (north here).
    let ys: Vec<f32> = lanes_of(&roads, east)
        .iter()
        .map(|l| l.curve().point_position(0).y - 2.0 * G)
        .collect();
    assert_eq!(ys, vec![-14.0, 0.0, 14.0]);
}

#[test]
fn one_way_roads_are_never_driven_the_wrong_way() {
    // A one-way loop of three sides, closed by a two-way street.
    let mut net = common::roads::RoutedRoads::new();
    let one_way = road_type("one_way_1lane");
    net.pieces(v(1.0, 1.0), v(6.0, 1.0), one_way);
    net.pieces(v(6.0, 1.0), v(6.0, 5.0), one_way);
    net.pieces(v(6.0, 5.0), v(1.0, 5.0), one_way);
    net.pieces(v(1.0, 5.0), v(1.0, 1.0), road_type("local_2lane"));
    // From the middle of the top (eastbound) road back to its start: all the way around.
    let from = net
        .roads
        .find_lane_position(v(4.0, 1.0) * G, 10.0, 0)
        .unwrap();
    let to = net
        .roads
        .find_lane_position(v(2.0, 1.0) * G, 10.0, 0)
        .unwrap();
    let route = net
        .route(from.lane_id, from.distance, to.lane_id, to.distance)
        .unwrap();
    let turns = route
        .lane_ids
        .iter()
        .filter(|&&id| net.roads.lanes[&id].is_connector())
        .count();
    assert_eq!(turns, 4, "around the block, not backwards");
}

#[test]
#[ignore = "needs lots (M3.1)"]
fn dragging_another_type_over_a_road_upgrades_it_in_place() {
    unimplemented!("port with M3.1");
}

#[test]
fn upgrading_part_of_a_road_splits_it() {
    let mut roads = RoadNetwork::new();
    pieces(
        &mut roads,
        v(1.0, 2.0),
        v(9.0, 2.0),
        road_type("local_2lane"),
    );
    pieces(
        &mut roads,
        v(5.0, 2.0),
        v(9.0, 2.0),
        road_type("avenue_4lane"),
    );
    assert_eq!(roads.segments.len(), 2);
    let mut types: Vec<&str> = roads
        .segments
        .values()
        .map(|s| s.road_type.id.as_str())
        .collect();
    types.sort();
    assert_eq!(types, ["avenue_4lane", "local_2lane"]);
}

#[test]
fn dragging_a_one_way_road_backwards_reverses_it() {
    let mut roads = RoadNetwork::new();
    let one_way = road_type("one_way_2lane");
    let id = pieces(&mut roads, v(1.0, 2.0), v(6.0, 2.0), one_way)[0];
    assert_eq!(
        roads.validate_road(v(1.0, 2.0) * G, v(6.0, 2.0) * G, 0),
        "Road already exists"
    );
    assert_eq!(
        roads
            .plan_road(v(1.0, 2.0) * G, v(6.0, 2.0) * G, 0, Some(one_way))
            .error,
        "Road already exists"
    );
    pieces(&mut roads, v(6.0, 2.0), v(1.0, 2.0), one_way);
    for lane in lanes_of(&roads, id) {
        assert!(end_direction(lane).x < -0.9, "now westbound");
    }
    assert_eq!(
        roads.nodes[&roads.segments[&id].start_node_id].position,
        v(6.0, 2.0) * G
    );
}

#[test]
fn types_and_directions_survive_a_save() {
    let mut roads = RoadNetwork::new();
    pieces(
        &mut roads,
        v(1.0, 2.0),
        v(6.0, 2.0),
        road_type("avenue_4lane"),
    );
    pieces(
        &mut roads,
        v(6.0, 5.0),
        v(1.0, 5.0),
        road_type("one_way_2lane"),
    );
    let loaded =
        RoadNetwork::from_dict(&serde_json::from_str(&roads.to_dict().to_string()).unwrap());
    let one_way = loaded
        .segments
        .values()
        .find(|s| s.road_type.one_way)
        .unwrap();
    assert_eq!(
        loaded.nodes[&one_way.start_node_id].position,
        v(6.0, 5.0) * G
    );
    for lane_id in &one_way.lane_ids {
        assert!(end_direction(&loaded.lanes[lane_id]).x < -0.9);
    }
    assert_eq!(loaded.lanes.len(), roads.lanes.len());
}

#[test]
fn turns_from_a_three_lane_one_way_use_the_matching_lanes() {
    let mut roads = RoadNetwork::new();
    pieces(
        &mut roads,
        v(1.0, 3.0),
        v(8.0, 3.0),
        road_type("one_way_3lane"),
    ); // Eastbound.
    pieces(
        &mut roads,
        v(5.0, 0.0),
        v(5.0, 6.0),
        road_type("local_2lane"),
    );
    let junction = &roads.nodes[&roads.find_node_at(v(5.0, 3.0) * G, 0).unwrap()];
    for lane_id in &junction.connector_lane_ids {
        let connector = &roads.lanes[lane_id];
        let feeder = &roads.lanes[&connector.feeder_lane_id.unwrap()];
        if !roads.segments[&feeder.segment_id.unwrap()]
            .road_type
            .one_way
        {
            continue;
        }
        match connector.turn {
            Turn::Left => assert_eq!(feeder.index_from_center, 0, "left turns from the left lane"),
            Turn::Right => assert_eq!(
                feeder.index_from_center, 2,
                "right turns from the curb lane"
            ),
            _ => {}
        }
    }
}

#[test]
#[ignore = "needs vehicles (M4.2)"]
fn vehicles_on_a_road_being_upgraded_carry_on() {
    unimplemented!("port with M4.2");
}

#[test]
#[ignore = "needs vehicles and lane changes (M4.4)"]
fn stress_grid_with_avenues() {
    unimplemented!("port with M4.4");
}

#[test]
#[ignore = "needs vehicles and lane changes (M4.4)"]
fn stress_grid_with_one_way_streets() {
    unimplemented!("port with M4.4");
}

#[test]
#[ignore = "needs vehicles and lane changes (M4.4)"]
fn stress_grid_with_avenues_and_one_way_streets() {
    unimplemented!("port with M4.4");
}

/// Junctions too close for the roads meeting there would leave lanes no vehicle fits
/// (an avenue cuts its lanes back 28 px from each junction): such roads are refused.
#[test]
fn junctions_must_leave_room_for_a_vehicle_between_them() {
    let mut roads = RoadNetwork::new();
    let one_way = road_type("one_way_2lane");
    pieces(
        &mut roads,
        v(0.0, 8.0),
        v(20.0, 8.0),
        road_type("avenue_4lane"),
    );
    pieces(&mut roads, v(8.0, 2.0), v(8.0, 14.0), one_way);
    let plan = roads.plan_road(v(9.0, 14.0) * G, v(9.0, 2.0) * G, 0, Some(one_way));
    assert_eq!(
        plan.error,
        RoadNetwork::TOO_CLOSE_ERROR,
        "one cell from the first crossing of the avenue"
    );
    assert_eq!(
        roads
            .plan_road(v(10.0, 14.0) * G, v(10.0, 2.0) * G, 0, Some(one_way))
            .error,
        "",
        "two cells is fine"
    );
    // Local streets only need one cell.
    let local = road_type("local_2lane");
    pieces(&mut roads, v(0.0, 20.0), v(10.0, 20.0), local);
    pieces(&mut roads, v(3.0, 17.0), v(3.0, 23.0), local);
    assert_eq!(
        roads
            .plan_road(v(4.0, 17.0) * G, v(4.0, 23.0) * G, 0, Some(local))
            .error,
        ""
    );
}

#[test]
fn upgrading_to_an_avenue_checks_the_room_too() {
    let mut roads = RoadNetwork::new();
    let local = road_type("local_2lane");
    pieces(&mut roads, v(0.0, 8.0), v(20.0, 8.0), local);
    pieces(&mut roads, v(8.0, 2.0), v(8.0, 14.0), local);
    pieces(&mut roads, v(9.0, 2.0), v(9.0, 14.0), local);
    let plan = roads.plan_road(
        v(0.0, 8.0) * G,
        v(20.0, 8.0) * G,
        0,
        Some(road_type("avenue_4lane")),
    );
    assert_eq!(
        plan.error,
        RoadNetwork::TOO_CLOSE_ERROR,
        "the street between the two crossings would shrink to nothing"
    );
    assert_eq!(
        roads
            .segments
            .values()
            .filter(|s| s.road_type.id == "avenue_4lane")
            .count(),
        0
    );
}
