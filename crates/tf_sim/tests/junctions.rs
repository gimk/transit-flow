//! Port of `tests/unit/test_junctions.gd`: lane trimming at junctions and the connector
//! lanes that link roads through them.

mod common;

use common::roads::{G, changes, connectors, custom, lane_ids, local};
use glam::Vec2;
use tf_sim::events::Outbox;
use tf_sim::ids::SegmentId;
use tf_sim::roads::{Lane, RoadNetwork, RoadType, Turn};

fn add(roads: &mut RoadNetwork, from: Vec2, to: Vec2) -> SegmentId {
    add_typed(roads, from, to, local())
}

fn add_typed(
    roads: &mut RoadNetwork,
    from: Vec2,
    to: Vec2,
    road_type: &'static RoadType,
) -> SegmentId {
    roads
        .add_road(from * G, to * G, road_type, 0, &mut Outbox::new())
        .unwrap()
}

fn turns(roads: &RoadNetwork, at: Vec2) -> Vec<Turn> {
    connectors(roads, at).iter().map(|lane| lane.turn).collect()
}

fn road_lane(roads: &RoadNetwork, segment_id: SegmentId, from_at: Vec2) -> &Lane {
    let from_node = roads.find_node_at(from_at * G, 0).unwrap();
    roads.segments[&segment_id]
        .lane_ids
        .iter()
        .map(|id| &roads.lanes[id])
        .find(|lane| lane.from_node_id == from_node)
        .unwrap()
}

fn start(lane: &Lane) -> Vec2 {
    lane.curve().point_position(0)
}

fn end(lane: &Lane) -> Vec2 {
    lane.curve().point_position(lane.curve().point_count() - 1)
}

/// GUT's `assert_almost_eq(a, b, Vector2.ONE * 0.01)`: each coordinate within 0.01.
fn near(a: Vec2, b: Vec2) -> bool {
    (a - b).abs().max_element() <= 0.01
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

#[test]
fn dead_end_gets_a_u_turn_and_no_trim() {
    let mut roads = RoadNetwork::new();
    let id = add(&mut roads, v(0.0, 0.0), v(2.0, 0.0));
    assert_eq!(turns(&roads, v(2.0, 0.0)), vec![Turn::UTurn]);
    let eastbound = road_lane(&roads, id, v(0.0, 0.0));
    assert_eq!(end(eastbound), v(2.0 * G, 7.0));
}

#[test]
fn u_turn_links_the_two_directions() {
    let mut roads = RoadNetwork::new();
    let id = add(&mut roads, v(0.0, 0.0), v(2.0, 0.0));
    let eastbound = road_lane(&roads, id, v(0.0, 0.0));
    let westbound = road_lane(&roads, id, v(2.0, 0.0));
    let u_turn = connectors(&roads, v(2.0, 0.0))[0];
    assert_eq!(eastbound.outgoing_lane_ids, vec![u_turn.id]);
    assert_eq!(u_turn.outgoing_lane_ids, vec![westbound.id]);
    assert!(near(start(u_turn), end(eastbound)));
    assert!(near(end(u_turn), start(westbound)));
}

#[test]
fn corner_trims_lanes_by_half_the_road_width() {
    let mut roads = RoadNetwork::new();
    let east = add(&mut roads, v(0.0, 0.0), v(2.0, 0.0));
    add(&mut roads, v(2.0, 0.0), v(2.0, 2.0));
    let half_width = (local().total_width() * 0.5) as f32;
    let eastbound = road_lane(&roads, east, v(0.0, 0.0));
    let westbound = road_lane(&roads, east, v(2.0, 0.0));
    assert!(near(end(eastbound), v(2.0 * G - half_width, 7.0)));
    assert!(near(start(westbound), v(2.0 * G - half_width, -7.0)));
    // The far end is still a dead end and stays untrimmed.
    assert!(near(start(eastbound), v(0.0, 7.0)));
}

#[test]
fn corner_has_one_turn_each_way_and_no_u_turns() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 0.0), v(2.0, 0.0));
    add(&mut roads, v(2.0, 0.0), v(2.0, 2.0));
    let turns = turns(&roads, v(2.0, 0.0));
    assert_eq!(turns.len(), 2);
    assert!(turns.contains(&Turn::Right)); // Eastbound turning south.
    assert!(turns.contains(&Turn::Left)); // Northbound turning west.
}

#[test]
fn connector_endpoints_meet_the_lanes_they_join() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 0.0), v(2.0, 0.0));
    add(&mut roads, v(2.0, 0.0), v(2.0, 2.0));
    for connector in connectors(&roads, v(2.0, 0.0)) {
        let target = &roads.lanes[&connector.outgoing_lane_ids[0]];
        assert!(near(end(connector), start(target)));
        let feeders: Vec<&Lane> = roads
            .lanes
            .values()
            .filter(|lane| lane.outgoing_lane_ids.contains(&connector.id))
            .collect();
        assert_eq!(feeders.len(), 1);
        assert!(near(start(connector), end(feeders[0])));
    }
}

#[test]
fn four_way_junction_has_straight_left_right_from_every_approach() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 2.0), v(2.0, 2.0));
    add(&mut roads, v(4.0, 2.0), v(2.0, 2.0));
    add(&mut roads, v(2.0, 0.0), v(2.0, 2.0));
    add(&mut roads, v(2.0, 4.0), v(2.0, 2.0));
    let turns = turns(&roads, v(2.0, 2.0));
    assert_eq!(turns.len(), 12);
    for turn in [Turn::Straight, Turn::Right, Turn::Left] {
        assert_eq!(turns.iter().filter(|&&t| t == turn).count(), 4);
    }
}

#[test]
fn straight_continuation_connector_is_straight() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 0.0), v(2.0, 0.0));
    add(&mut roads, v(2.0, 0.0), v(4.0, 0.0));
    let straight: Vec<&Lane> = connectors(&roads, v(2.0, 0.0))
        .into_iter()
        .filter(|lane| lane.turn == Turn::Straight)
        .collect();
    assert_eq!(straight.len(), 2);
    for connector in straight {
        assert!((connector.length - local().total_width()).abs() < 0.01);
    }
}

#[test]
fn turn_connectors_are_slower_than_straight() {
    let mut roads = RoadNetwork::new();
    add(&mut roads, v(0.0, 0.0), v(2.0, 0.0));
    add(&mut roads, v(2.0, 0.0), v(2.0, 2.0));
    for connector in connectors(&roads, v(2.0, 0.0)) {
        assert!(connector.speed_limit < local().speed_limit);
    }
}

#[test]
fn removing_a_road_turns_the_junction_back_into_a_dead_end() {
    let mut roads = RoadNetwork::new();
    let east = add(&mut roads, v(0.0, 0.0), v(2.0, 0.0));
    let south = add(&mut roads, v(2.0, 0.0), v(2.0, 2.0));
    roads.remove_segment(south, &mut Outbox::new());
    assert_eq!(turns(&roads, v(2.0, 0.0)), vec![Turn::UTurn]);
    assert_eq!(
        end(road_lane(&roads, east, v(0.0, 0.0))),
        v(2.0 * G, 7.0),
        "trim removed"
    );
    for lane in roads.lanes.values() {
        for next_id in &lane.outgoing_lane_ids {
            assert!(
                roads.lanes.contains_key(next_id),
                "no dangling lane references"
            );
        }
    }
}

#[test]
fn road_lane_ids_survive_junction_changes() {
    let mut roads = RoadNetwork::new();
    let east = add(&mut roads, v(0.0, 0.0), v(2.0, 0.0));
    let before = lane_ids(&roads.segments[&east].lane_ids);
    add(&mut roads, v(2.0, 0.0), v(2.0, 2.0));
    assert_eq!(lane_ids(&roads.segments[&east].lane_ids), before);
}

#[test]
fn changed_signal_lists_segments_whose_lanes_were_retrimmed() {
    let mut roads = RoadNetwork::new();
    let east = add(&mut roads, v(0.0, 0.0), v(2.0, 0.0));
    let mut out = Outbox::new();
    let south = roads
        .add_road(v(2.0, 0.0) * G, v(2.0, 2.0) * G, local(), 0, &mut out)
        .unwrap();
    let change = changes(&out)[0];
    assert!(change.segment_ids.contains(&east));
    assert!(change.segment_ids.contains(&south));
}

#[test]
fn multi_lane_roads_turn_from_the_matching_lanes() {
    let avenue = custom(RoadType {
        lanes_per_direction: 2,
        lane_width: 14.0,
        ..RoadType::default()
    });
    let mut roads = RoadNetwork::new();
    add_typed(&mut roads, v(0.0, 0.0), v(3.0, 0.0), avenue);
    add_typed(&mut roads, v(3.0, 0.0), v(3.0, 3.0), avenue);
    for connector in connectors(&roads, v(3.0, 0.0)) {
        let source = roads
            .lanes
            .values()
            .find(|lane| lane.outgoing_lane_ids.contains(&connector.id))
            .unwrap();
        let target = &roads.lanes[&connector.outgoing_lane_ids[0]];
        let pair = [source.index_from_center, target.index_from_center];
        if connector.turn == Turn::Right {
            assert_eq!(pair, [1, 1], "right turns use curb lanes");
        } else {
            assert_eq!(pair, [0, 0], "left turns use center lanes");
        }
    }
    assert_eq!(connectors(&roads, v(3.0, 0.0)).len(), 2);
}
