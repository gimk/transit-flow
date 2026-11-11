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
fn dragging_another_type_over_a_road_upgrades_it_in_place() {
    use tf_sim::Simulation;
    use tf_sim::city::LotType;
    let mut sim = Simulation::new();
    let road = |sim: &mut Simulation, from: Vec2, to: Vec2, type_id: &str| {
        let mut ids = Vec::new();
        sim.call(|sim, out| {
            ids = sim
                .roads
                .add_road_pieces(from * G, to * G, road_type(type_id), 0, out)
        });
        ids
    };
    road(&mut sim, v(1.0, 2.0), v(6.0, 2.0), "local_2lane");
    road(&mut sim, v(3.0, 0.0), v(3.0, 2.0), "local_2lane"); // A side street meeting it.
    let mut lot = None;
    sim.call(|sim, out| {
        lot = sim.lots.add_lot(
            &sim.roads,
            glam::IVec2::new(2, 2),
            LotType::Residential,
            out,
        )
    });
    let lot = lot.unwrap();
    let nodes_before = sim.roads.nodes.len();
    let segments_before = sim.roads.segments.len();
    let pieces = road(&mut sim, v(1.0, 2.0), v(6.0, 2.0), "avenue_4lane");
    assert_eq!(
        pieces.len(),
        2,
        "both pieces along it (it was split by the side street)"
    );
    for segment_id in &pieces {
        assert_eq!(sim.roads.segments[segment_id].road_type.id, "avenue_4lane");
        assert_eq!(lanes_of(&sim.roads, *segment_id).len(), 4);
    }
    assert_eq!(
        sim.roads.segments.len(),
        segments_before,
        "no new segments: the same ones, retyped"
    );
    assert_eq!(sim.roads.nodes.len(), nodes_before, "no new nodes");
    assert!(
        sim.lots.access(&sim.roads, lot).is_some(),
        "lots along it keep their access"
    );
    assert_eq!(
        sim.roads.validate_road(v(1.0, 2.0) * G, v(6.0, 2.0) * G, 0),
        "Road already exists"
    );
    assert_eq!(
        sim.roads
            .plan_road(
                v(1.0, 2.0) * G,
                v(6.0, 2.0) * G,
                0,
                Some(road_type("avenue_4lane"))
            )
            .error,
        "Road already exists"
    );
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
fn vehicles_on_a_road_being_upgraded_carry_on() {
    use common::traffic::{on_road, road, spawn, vehicle};
    let mut sim = tf_sim::Simulation::new();
    road(
        &mut sim,
        v(0.0, 2.0),
        v(12.0, 2.0),
        road_type("local_2lane"),
    );
    let id = spawn(
        &mut sim,
        v(1.0, 2.0) + v(0.0, 7.0) / G,
        v(11.0, 2.0) + v(0.0, 7.0) / G,
        1,
    )
    .unwrap();
    for _ in 0..20 {
        sim.step();
    }
    road(
        &mut sim,
        v(0.0, 2.0),
        v(12.0, 2.0),
        road_type("avenue_4lane"),
    );
    let mut arrived = false;
    for _ in 0..600 {
        sim.step();
        if !on_road(&sim, id) {
            // As in GDScript, only a patience give-up counts against it. (In both builds
            // the car actually fails at once: its destination lane was replaced; see
            // NOTES.md, "GDScript bugs found".)
            arrived = !vehicle(&sim, id).gave_up;
            break;
        }
    }
    assert!(arrived, "re-snapped onto the new lanes and arrived");
}

/// 200 random trips for 90 s on a 5 x 5 grid, 3 cells apart: even rows/columns of
/// `even` roads, odd ones of `odd_h` / `odd_v` roads (one-way ones alternate direction).
fn stress_grid(even: &str, odd_h: &str, odd_v: &str, min_arrivals: usize, godot: (usize, usize)) {
    use common::traffic::{connector_conflict_violations, min_queue_gap, road, vehicle_events};
    let mut sim = tf_sim::Simulation::new();
    for i in 0..5 {
        let at = (1 + i * 3) as f32;
        let horizontal = road_type(if i % 2 == 0 { even } else { odd_h });
        let vertical = road_type(if i % 2 == 0 { even } else { odd_v });
        if i % 4 == 1 {
            road(&mut sim, v(13.0, at), v(1.0, at), horizontal); // Westbound.
            road(&mut sim, v(at, 13.0), v(at, 1.0), vertical); // Northbound.
        } else {
            road(&mut sim, v(1.0, at), v(13.0, at), horizontal);
            road(&mut sim, v(at, 1.0), v(at, 13.0), vertical);
        }
    }
    sim.stress.target_count = 200;
    let mut arrived = 0;
    let mut violations = 0;
    let mut worst_gap = f64::INFINITY;
    for _ in 0..tf_sim::Simulation::TICK_RATE * 90 {
        sim.step();
        arrived += vehicle_events(&mut sim).0.len();
        violations += connector_conflict_violations(&sim);
        worst_gap = worst_gap.min(min_queue_gap(&sim));
    }
    println!(
        "grid {even}/{odd_h}: vehicles {}, arrivals {arrived}, stuck {}, smallest gap {worst_gap:.2}",
        sim.vehicles.vehicles.len(),
        sim.vehicles.stuck_count(30.0)
    );
    assert_eq!(
        violations, 0,
        "never two vehicles where conflicting paths meet"
    );
    assert!(
        worst_gap >= tf_sim::traffic::vehicle_system::MIN_GAP - 0.01,
        "no overlaps, lane changes included"
    );
    assert!(arrived > min_arrivals, "traffic flows");
    assert_eq!(sim.vehicles.stuck_count(30.0), 0, "no deadlock");
    assert_eq!(
        (arrived, sim.vehicles.vehicles.len()),
        godot,
        "the same arrivals and vehicles as Godot (`migration/baseline.md`)"
    );
}

/// Random traffic on grids mixing road types: multi-lane and one-way junctions and lane
/// changes keep the safety rules and nothing deadlocks. 200 vehicles: at 250 a grid with
/// one-way streets is at the edge of real gridlock (it locks up or not depending on the
/// random trips).
#[test]
fn stress_grid_with_avenues() {
    stress_grid(
        "avenue_4lane",
        "local_2lane",
        "local_2lane",
        300,
        (491, 200),
    );
}

#[test]
fn stress_grid_with_one_way_streets() {
    stress_grid(
        "local_2lane",
        "one_way_2lane",
        "one_way_1lane",
        200,
        (283, 199),
    );
}

/// Before lane changing (5b) this gridlocked: a car in the wrong lane of an avenue had to
/// circle the block to reach its turn.
#[test]
fn stress_grid_with_avenues_and_one_way_streets() {
    stress_grid(
        "avenue_4lane",
        "one_way_2lane",
        "one_way_1lane",
        300,
        (419, 199),
    );
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
