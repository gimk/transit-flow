//! Port of `tests/unit/test_road_network.gd`.

mod common;

use common::roads::{add, changes, local};
use glam::Vec2;
use tf_sim::events::Outbox;
use tf_sim::ids::SegmentId;
use tf_sim::roads::RoadNetwork;

fn add_on(roads: &mut RoadNetwork, from: Vec2, to: Vec2, layer: i32) -> Option<SegmentId> {
    roads.add_road(from, to, local(), layer, &mut Outbox::new())
}

#[test]
fn road_type_resource_loads() {
    assert_eq!(local().id, "local_2lane");
}

#[test]
fn add_road_creates_two_nodes_one_segment_two_road_lanes() {
    let mut roads = RoadNetwork::new();
    let id = add(&mut roads, Vec2::ZERO, Vec2::new(128.0, 0.0), local()).unwrap();
    assert_eq!(roads.nodes.len(), 2);
    assert_eq!(roads.segments.len(), 1);
    assert_eq!(roads.segments[&id].lane_ids.len(), 2);
    assert_eq!(
        roads.lanes.len(),
        4,
        "2 road lanes + a U-turn connector at each dead end"
    );
}

#[test]
fn lanes_connect_the_segment_nodes_in_both_directions() {
    let mut roads = RoadNetwork::new();
    let id = add(&mut roads, Vec2::ZERO, Vec2::new(128.0, 0.0), local()).unwrap();
    let segment = &roads.segments[&id];
    let mut directions = Vec::new();
    for lane_id in &segment.lane_ids {
        let lane = &roads.lanes[lane_id];
        directions.push((lane.from_node_id, lane.to_node_id));
        assert_eq!(lane.speed_limit, local().speed_limit);
    }
    assert!(directions.contains(&(segment.start_node_id, segment.end_node_id)));
    assert!(directions.contains(&(segment.end_node_id, segment.start_node_id)));
}

#[test]
fn roads_sharing_an_endpoint_share_a_node() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, Vec2::ZERO, Vec2::new(128.0, 0.0), local());
    add(
        &mut roads,
        Vec2::new(128.0, 0.0),
        Vec2::new(128.0, 128.0),
        local(),
    );
    assert_eq!(roads.nodes.len(), 3);
    let corner = roads.find_node_at(Vec2::new(128.0, 0.0), 0).unwrap();
    assert_eq!(roads.nodes[&corner].degree(), 2);
}

#[test]
fn rejects_too_short_road() {
    let mut roads = RoadNetwork::new();
    assert_eq!(
        add(&mut roads, Vec2::splat(64.0), Vec2::splat(64.0), local()),
        None
    );
    assert_eq!(roads.segments.len(), 0);
}

#[test]
fn rejects_duplicate_road_in_either_direction() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, Vec2::ZERO, Vec2::new(128.0, 0.0), local());
    assert_eq!(
        add(&mut roads, Vec2::ZERO, Vec2::new(128.0, 0.0), local()),
        None
    );
    assert_eq!(
        add(&mut roads, Vec2::new(128.0, 0.0), Vec2::ZERO, local()),
        None
    );
}

#[test]
fn allows_crossing_on_different_layers() {
    let mut roads = RoadNetwork::new();
    add_on(&mut roads, Vec2::new(0.0, 64.0), Vec2::new(128.0, 64.0), 0);
    assert!(add_on(&mut roads, Vec2::new(64.0, 0.0), Vec2::new(64.0, 128.0), 1).is_some());
    assert_eq!(roads.nodes.len(), 4);
}

#[test]
fn allows_extending_a_road_in_a_straight_line() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, Vec2::ZERO, Vec2::new(128.0, 0.0), local());
    assert!(
        add(
            &mut roads,
            Vec2::new(128.0, 0.0),
            Vec2::new(256.0, 0.0),
            local()
        )
        .is_some()
    );
}

#[test]
fn remove_segment_removes_lanes_and_orphan_nodes_only() {
    let mut roads = RoadNetwork::new();
    let first = add(&mut roads, Vec2::ZERO, Vec2::new(128.0, 0.0), local()).unwrap();
    add(
        &mut roads,
        Vec2::new(128.0, 0.0),
        Vec2::new(128.0, 128.0),
        local(),
    );
    assert!(roads.remove_segment(first, &mut Outbox::new()));
    assert_eq!(roads.segments.len(), 1);
    assert_eq!(roads.lanes.len(), 4, "2 road lanes + 2 dead-end U-turns");
    assert_eq!(roads.nodes.len(), 2, "the shared corner node is kept");
    assert_eq!(
        roads.find_node_at(Vec2::ZERO, 0),
        None,
        "the dead-end node is removed"
    );
}

#[test]
fn remove_unknown_segment_returns_false() {
    let mut roads = RoadNetwork::new();
    assert!(!roads.remove_segment(SegmentId(42), &mut Outbox::new()));
}

#[test]
fn changed_signal_reports_touched_ids() {
    let mut roads = RoadNetwork::new();
    let mut out = Outbox::new();
    let id = roads
        .add_road(Vec2::ZERO, Vec2::new(128.0, 0.0), local(), 0, &mut out)
        .unwrap();
    let emitted = changes(&out);
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].segment_ids, vec![id]);
    assert_eq!(emitted[0].node_ids.len(), 2);
    roads.remove_segment(id, &mut out);
    assert_eq!(changes(&out).len(), 2);
}

#[test]
fn find_segment_at() {
    let mut roads = RoadNetwork::new();
    let id = add(&mut roads, Vec2::ZERO, Vec2::new(128.0, 0.0), local());
    assert_eq!(roads.find_segment_at(Vec2::new(60.0, 10.0), 18.0, 0), id);
    assert_eq!(roads.find_segment_at(Vec2::new(60.0, 40.0), 18.0, 0), None);
}

#[test]
fn save_round_trip_through_json() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, Vec2::ZERO, Vec2::new(128.0, 0.0), local());
    add(
        &mut roads,
        Vec2::new(128.0, 0.0),
        Vec2::new(128.0, 192.0),
        local(),
    );
    let removed = add(
        &mut roads,
        Vec2::new(128.0, 192.0),
        Vec2::new(320.0, 192.0),
        local(),
    )
    .unwrap();
    roads.remove_segment(removed, &mut Outbox::new());

    let saved = roads.to_dict();
    let parsed: serde_json::Value = serde_json::from_str(&saved.to_string()).unwrap();
    let mut loaded = RoadNetwork::from_dict(&parsed);

    assert_eq!(loaded.to_dict(), saved);
    assert_eq!(
        loaded.lanes.len(),
        8,
        "4 road lanes + 2 corner connectors + 2 dead-end U-turns"
    );
    let corner = loaded.find_node_at(Vec2::new(128.0, 0.0), 0).unwrap();
    assert_eq!(loaded.nodes[&corner].degree(), 2);
    // Ids keep counting from where the saved network left off.
    let next_id = add(&mut loaded, Vec2::ZERO, Vec2::new(0.0, 128.0), local()).unwrap();
    assert_eq!(next_id.0, removed.0 + 1);
}
