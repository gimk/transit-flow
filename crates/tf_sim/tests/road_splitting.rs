//! Port of `tests/unit/test_road_splitting.gd`: roads that cross, touch or overlap
//! existing roads are split into segments joined by junction nodes.

mod common;

use common::roads::{G, assert_graph_valid, changes, local, node_at};
use glam::Vec2;
use tf_sim::events::Outbox;
use tf_sim::ids::SegmentId;
use tf_sim::rng::SimRng;
use tf_sim::roads::RoadNetwork;

fn add(roads: &mut RoadNetwork, from: Vec2, to: Vec2) -> Vec<SegmentId> {
    roads.add_road_pieces(from * G, to * G, local(), 0, &mut Outbox::new())
}

fn add_on(roads: &mut RoadNetwork, from: Vec2, to: Vec2, layer: i32) -> Vec<SegmentId> {
    roads.add_road_pieces(from * G, to * G, local(), layer, &mut Outbox::new())
}

fn degree(roads: &RoadNetwork, at: Vec2) -> usize {
    roads
        .find_node_at(at * G, 0)
        .map_or(0, |id| roads.nodes[&id].degree())
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

#[test]
fn crossing_creates_a_four_way_junction() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 2.0), v(4.0, 2.0));
    let pieces = add(&mut roads, v(2.0, 0.0), v(2.0, 4.0));
    assert_eq!(pieces.len(), 2);
    assert_eq!(roads.segments.len(), 4);
    assert_eq!(degree(&roads, v(2.0, 2.0)), 4);
    assert_eq!(node_at(&roads, v(2.0, 2.0)).connector_lane_ids.len(), 12);
    assert_graph_valid(&roads);
}

#[test]
fn ending_on_a_road_creates_a_t_junction() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 2.0), v(4.0, 2.0));
    assert_eq!(add(&mut roads, v(2.0, 0.0), v(2.0, 2.0)).len(), 1);
    assert_eq!(roads.segments.len(), 3);
    assert_eq!(degree(&roads, v(2.0, 2.0)), 3);
    assert_graph_valid(&roads);
}

#[test]
fn starting_on_a_road_creates_a_t_junction() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 2.0), v(4.0, 2.0));
    add(&mut roads, v(2.0, 2.0), v(2.0, 5.0));
    assert_eq!(degree(&roads, v(2.0, 2.0)), 3);
    assert_graph_valid(&roads);
}

#[test]
fn passing_through_an_existing_node_splits_the_new_road() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(2.0, 0.0), v(2.0, 2.0));
    let pieces = add(&mut roads, v(0.0, 2.0), v(4.0, 2.0));
    assert_eq!(pieces.len(), 2);
    assert_eq!(degree(&roads, v(2.0, 2.0)), 3);
    assert_graph_valid(&roads);
}

#[test]
fn crossing_several_roads_at_once() {
    let mut roads = RoadNetwork::new();
    for y in [1.0, 3.0, 5.0] {
        add(&mut roads, v(0.0, y), v(4.0, y));
    }
    let pieces = add(&mut roads, v(2.0, 0.0), v(2.0, 6.0));
    assert_eq!(pieces.len(), 4);
    assert_eq!(roads.segments.len(), 4 + 6);
    for y in [1.0, 3.0, 5.0] {
        assert_eq!(degree(&roads, v(2.0, y)), 4);
    }
    assert_graph_valid(&roads);
}

#[test]
fn new_pieces_are_returned_in_order_from_start_to_end() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 2.0), v(4.0, 2.0));
    let pieces = add(&mut roads, v(2.0, 4.0), v(2.0, 0.0));
    let first = &roads.segments[&pieces[0]];
    let second = &roads.segments[&pieces[1]];
    assert_eq!(roads.nodes[&first.start_node_id].position, v(2.0, 4.0) * G);
    assert_eq!(roads.nodes[&second.end_node_id].position, v(2.0, 0.0) * G);
}

#[test]
fn extending_along_an_existing_road_only_builds_the_new_part() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 2.0), v(4.0, 2.0));
    let pieces = add(&mut roads, v(2.0, 2.0), v(6.0, 2.0));
    assert_eq!(pieces.len(), 1, "only 4..6 is new");
    assert_eq!(roads.segments.len(), 3, "0..2, 2..4, 4..6");
    assert_eq!(degree(&roads, v(2.0, 2.0)), 2);
    assert_eq!(degree(&roads, v(4.0, 2.0)), 2);
    assert_graph_valid(&roads);
}

#[test]
fn road_entirely_on_an_existing_road_is_rejected() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 2.0), v(6.0, 2.0));
    assert_eq!(
        roads.validate_road(v(1.0, 2.0) * G, v(4.0, 2.0) * G, 0),
        "Road already exists"
    );
    assert_eq!(add(&mut roads, v(1.0, 2.0), v(4.0, 2.0)).len(), 0);
    assert_eq!(
        roads.segments.len(),
        1,
        "nothing was split by the rejected road"
    );
}

#[test]
fn rejects_pieces_shorter_than_the_minimum() {
    let mut roads = RoadNetwork::new();
    roads.add_road(
        v(100.0, 0.0),
        v(100.0, 200.0),
        local(),
        0,
        &mut Outbox::new(),
    );
    assert_eq!(
        roads.validate_road(v(90.0, 100.0), v(300.0, 100.0), 0),
        "Too close to an existing junction"
    );
    assert_eq!(
        roads.add_road(
            v(90.0, 100.0),
            v(300.0, 100.0),
            local(),
            0,
            &mut Outbox::new()
        ),
        None
    );
    assert_eq!(roads.segments.len(), 1);
}

#[test]
fn different_layers_do_not_split() {
    let mut roads = RoadNetwork::new();
    add_on(&mut roads, v(0.0, 2.0), v(4.0, 2.0), 0);
    add_on(&mut roads, v(2.0, 0.0), v(2.0, 4.0), 1);
    assert_eq!(roads.segments.len(), 2);
    assert_eq!(roads.find_node_at(v(2.0, 2.0) * G, 0), None);
    assert_eq!(roads.find_node_at(v(2.0, 2.0) * G, 1), None);
}

#[test]
fn planning_changes_nothing() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 2.0), v(4.0, 2.0));
    let before = roads.to_dict();
    // plan_road takes `&self` and no outbox: it can neither change nor emit anything.
    let plan = roads.plan_road(v(2.0, 0.0) * G, v(2.0, 4.0) * G, 0, None);
    assert_eq!(plan.error, "");
    assert_eq!(
        plan.points,
        vec![v(2.0, 0.0) * G, v(2.0, 2.0) * G, v(2.0, 4.0) * G]
    );
    assert_eq!(plan.overlaps, vec![false, false]);
    assert_eq!(roads.to_dict(), before);
}

#[test]
fn changed_signal_reports_split_and_new_segments() {
    let mut roads = RoadNetwork::new();
    let original = add(&mut roads, v(0.0, 2.0), v(4.0, 2.0))[0];
    let mut out = Outbox::new();
    let pieces = roads.add_road_pieces(v(2.0, 0.0) * G, v(2.0, 4.0) * G, local(), 0, &mut out);
    let change = changes(&out)[0];
    assert!(
        change.segment_ids.contains(&original),
        "the removed segment, so views drop it"
    );
    for id in roads.segments.keys() {
        assert!(
            change.segment_ids.contains(id),
            "every segment now at the junction"
        );
    }
    assert!(!roads.segments.contains_key(&original));
    assert_eq!(pieces.len(), 2);
}

#[test]
fn split_keeps_direction_and_type() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 2.0), v(4.0, 2.0));
    add(&mut roads, v(2.0, 0.0), v(2.0, 4.0));
    let west_half = roads
        .segment_between(
            roads.find_node_at(v(0.0, 2.0) * G, 0).unwrap(),
            roads.find_node_at(v(2.0, 2.0) * G, 0).unwrap(),
        )
        .unwrap();
    let segment = &roads.segments[&west_half];
    assert_eq!(
        roads.nodes[&segment.start_node_id].position,
        v(0.0, 2.0) * G
    );
    assert!(std::ptr::eq(segment.road_type, local()));
}

#[test]
#[ignore = "needs the Router (M2.1)"]
fn routing_goes_straight_through_a_created_crossing() {
    unimplemented!("port with M2.1");
}

#[test]
fn random_grid_roads_keep_the_graph_valid() {
    // Godot's RandomNumberGenerator, seed 12345: the same 60 roads as the GDScript test.
    let mut rng = SimRng::new(12345);
    let mut roads = RoadNetwork::new();
    for _ in 0..60 {
        let a = v(rng.randi_range(0, 8) as f32, rng.randi_range(0, 8) as f32);
        let mut b = a;
        if rng.randi() % 2 == 0 {
            b.x = rng.randi_range(0, 8) as f32;
        } else {
            b.y = rng.randi_range(0, 8) as f32;
        }
        add(&mut roads, a, b);
        if rng.randi() % 5 == 0 && !roads.segments.is_empty() {
            let index = (rng.randi() % roads.segments.len() as i64) as usize;
            let id = *roads.segments.get_index(index).unwrap().0;
            roads.remove_segment(id, &mut Outbox::new());
        }
    }
    assert!(
        roads.segments.len() > 10,
        "the fuzz actually built something"
    );
    assert_graph_valid(&roads);
    let reloaded =
        RoadNetwork::from_dict(&serde_json::from_str(&roads.to_dict().to_string()).unwrap());
    assert_eq!(reloaded.segments.len(), roads.segments.len());
    assert_eq!(reloaded.lanes.len(), roads.lanes.len());
}
