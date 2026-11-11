//! Road types load with exactly the values of `data/road_types/*.tres` (and the parts
//! of `tests/unit/test_road_types.gd` that are about the data).

use tf_sim::roads::RoadType;

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
