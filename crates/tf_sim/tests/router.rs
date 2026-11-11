//! Port of `tests/unit/test_router.gd`.

mod common;

use std::collections::HashMap;

use common::roads::{G, RoutedRoads, custom, local};
use glam::Vec2;
use tf_sim::ids::{LaneId, SegmentId};
use tf_sim::roads::{Lane, RoadType, Turn};
use tf_sim::routing::Route;

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// The lane of `segment_id` that drives away from the node at `from_at`.
fn lane(net: &RoutedRoads, segment_id: SegmentId, from_at: Vec2) -> &Lane {
    let from_node = net.roads.find_node_at(from_at * G, 0).unwrap();
    net.roads.segments[&segment_id]
        .lane_ids
        .iter()
        .map(|id| &net.roads.lanes[id])
        .find(|lane| lane.from_node_id == from_node)
        .unwrap()
}

fn lane_id(net: &RoutedRoads, segment_id: SegmentId, from_at: Vec2) -> LaneId {
    lane(net, segment_id, from_at).id
}

fn assert_connected(net: &RoutedRoads, route: &Route) {
    for pair in route.lane_ids.windows(2) {
        assert!(
            net.roads.lanes[&pair[0]]
                .outgoing_lane_ids
                .contains(&pair[1]),
            "each lane must lead into the next"
        );
    }
}

/// The road segments a route drives on, in order, ignoring connectors.
fn segments_of(net: &RoutedRoads, route: &Route) -> Vec<SegmentId> {
    let mut result: Vec<SegmentId> = Vec::new();
    for id in &route.lane_ids {
        if let Some(segment_id) = net.roads.lanes[id].segment_id
            && result.last() != Some(&segment_id)
        {
            result.push(segment_id);
        }
    }
    result
}

/// Approach road into (0,0), then either a direct 4-cell road to (4,0) or a 10-cell
/// detour south via y=3, then an exit road. Returns the key segment ids.
fn build_detour_map(
    net: &mut RoutedRoads,
    direct: &'static RoadType,
    detour: &'static RoadType,
) -> HashMap<&'static str, SegmentId> {
    let mut ids = HashMap::new();
    ids.insert(
        "start",
        net.add(v(-2.0, 0.0), v(0.0, 0.0), local()).unwrap(),
    );
    ids.insert("direct", net.add(v(0.0, 0.0), v(4.0, 0.0), direct).unwrap());
    net.add(v(0.0, 0.0), v(0.0, 3.0), detour);
    net.add(v(0.0, 3.0), v(4.0, 3.0), detour);
    net.add(v(4.0, 3.0), v(4.0, 0.0), detour);
    ids.insert("dest", net.add(v(4.0, 0.0), v(6.0, 0.0), local()).unwrap());
    ids
}

#[test]
fn same_lane_ahead_is_a_single_lane_route() {
    let mut net = RoutedRoads::new();
    let id = net.add(v(0.0, 0.0), v(4.0, 0.0), local()).unwrap();
    let lane = lane(&net, id, v(0.0, 0.0)).clone();
    let route = net.route(lane.id, 10.0, lane.id, 100.0).unwrap();
    assert_eq!(route.lane_ids, vec![lane.id]);
    assert!((route.travel_time - 90.0 / lane.speed_limit).abs() <= 0.001);
}

#[test]
fn around_a_corner() {
    let mut net = RoutedRoads::new();
    let a = net.add(v(0.0, 0.0), v(3.0, 0.0), local()).unwrap();
    let b = net.add(v(3.0, 0.0), v(3.0, 3.0), local()).unwrap();
    let (from, to) = (lane_id(&net, a, v(0.0, 0.0)), lane_id(&net, b, v(3.0, 0.0)));
    let route = net.route(from, 0.0, to, 50.0).unwrap();
    assert_eq!(route.lane_ids.len(), 3, "lane, right-turn connector, lane");
    assert_eq!(net.roads.lanes[&route.lane_ids[1]].turn, Turn::Right);
    assert_connected(&net, &route);
}

#[test]
fn destination_behind_on_same_lane_loops_via_dead_end() {
    let mut net = RoutedRoads::new();
    let id = net.add(v(0.0, 0.0), v(4.0, 0.0), local()).unwrap();
    let eastbound = lane_id(&net, id, v(0.0, 0.0));
    let route = net.route(eastbound, 100.0, eastbound, 20.0).unwrap();
    assert_connected(&net, &route);
    assert_eq!(route.lane_ids[0], eastbound);
    assert_eq!(*route.lane_ids.last().unwrap(), eastbound);
    let u_turns = route
        .lane_ids
        .iter()
        .filter(|&&id| net.roads.lanes[&id].turn == Turn::UTurn)
        .count();
    assert_eq!(u_turns, 2, "turn around at each end");
}

#[test]
fn opposite_direction_uses_a_u_turn() {
    let mut net = RoutedRoads::new();
    let id = net.add(v(0.0, 0.0), v(4.0, 0.0), local()).unwrap();
    let (from, to) = (
        lane_id(&net, id, v(0.0, 0.0)),
        lane_id(&net, id, v(4.0, 0.0)),
    );
    let route = net.route(from, 10.0, to, 10.0).unwrap();
    assert_eq!(route.lane_ids.len(), 3);
    assert_eq!(net.roads.lanes[&route.lane_ids[1]].turn, Turn::UTurn);
}

#[test]
fn unreachable_returns_null() {
    let mut net = RoutedRoads::new();
    let a = net.add(v(0.0, 0.0), v(2.0, 0.0), local()).unwrap();
    let b = net.add(v(5.0, 5.0), v(7.0, 5.0), local()).unwrap();
    let (from, to) = (lane_id(&net, a, v(0.0, 0.0)), lane_id(&net, b, v(5.0, 5.0)));
    assert!(net.route(from, 0.0, to, 10.0).is_none());
}

#[test]
fn unknown_lane_returns_null() {
    let mut net = RoutedRoads::new();
    assert!(net.route(LaneId(123), 0.0, LaneId(456), 0.0).is_none());
}

#[test]
fn prefers_shorter_route_when_speeds_are_equal() {
    let mut net = RoutedRoads::new();
    let map = build_detour_map(&mut net, local(), local());
    let (from, to) = (
        lane_id(&net, map["start"], v(-2.0, 0.0)),
        lane_id(&net, map["dest"], v(4.0, 0.0)),
    );
    let route = net.route(from, 0.0, to, 20.0).unwrap();
    assert_eq!(
        segments_of(&net, &route),
        vec![map["start"], map["direct"], map["dest"]]
    );
    assert_connected(&net, &route);
}

#[test]
fn prefers_longer_but_faster_route() {
    // Direct: 256 px at 20 = 12.8 s. Detour: 640 px at 200 = 3.2 s, plus turns.
    let slow = custom(RoadType {
        speed_limit: 20.0,
        ..RoadType::default()
    });
    let fast = custom(RoadType {
        speed_limit: 200.0,
        ..RoadType::default()
    });
    let mut net = RoutedRoads::new();
    let map = build_detour_map(&mut net, slow, fast);
    let (from, to) = (
        lane_id(&net, map["start"], v(-2.0, 0.0)),
        lane_id(&net, map["dest"], v(4.0, 0.0)),
    );
    let route = net.route(from, 0.0, to, 20.0).unwrap();
    assert_connected(&net, &route);
    assert!(!segments_of(&net, &route).contains(&map["direct"]));
    assert_eq!(segments_of(&net, &route).len(), 5);
}

#[test]
fn route_updates_after_network_change() {
    let mut net = RoutedRoads::new();
    let map = build_detour_map(&mut net, local(), local());
    let (from, to) = (
        lane_id(&net, map["start"], v(-2.0, 0.0)),
        lane_id(&net, map["dest"], v(4.0, 0.0)),
    );
    let first = net.route(from, 0.0, to, 20.0).unwrap();
    assert!(segments_of(&net, &first).contains(&map["direct"]));
    net.remove(map["direct"]);
    let route = net.route(from, 0.0, to, 20.0).expect("detour still exists");
    assert_eq!(segments_of(&net, &route).len(), 5);
    assert_connected(&net, &route);
}

#[test]
fn travel_time_accounts_for_partial_first_and_last_lanes() {
    let mut net = RoutedRoads::new();
    let a = net.add(v(0.0, 0.0), v(3.0, 0.0), local()).unwrap();
    let b = net.add(v(3.0, 0.0), v(3.0, 3.0), local()).unwrap();
    let first = lane(&net, a, v(0.0, 0.0)).clone();
    let last = lane(&net, b, v(3.0, 0.0)).clone();
    let route = net.route(first.id, 30.0, last.id, 40.0).unwrap();
    let expected = (first.length - 30.0) / first.speed_limit
        + net.roads.lanes[&route.lane_ids[1]].travel_time()
        + 40.0 / last.speed_limit;
    assert!((route.travel_time - expected).abs() <= 0.001);
}

#[test]
fn congestion_makes_a_lane_less_attractive() {
    let mut net = RoutedRoads::new();
    let map = build_detour_map(&mut net, local(), local());
    let (from, to) = (
        lane_id(&net, map["start"], v(-2.0, 0.0)),
        lane_id(&net, map["dest"], v(4.0, 0.0)),
    );
    net.route(from, 0.0, to, 20.0); // Builds the graph.
    let direct = lane_id(&net, map["direct"], v(0.0, 0.0));
    net.router.congestion.insert(direct, 50.0);
    net.router.invalidate_cache();
    let route = net.route(from, 0.0, to, 20.0).unwrap();
    assert!(!segments_of(&net, &route).contains(&map["direct"]));
}

#[test]
fn route_polyline_starts_and_ends_at_the_requested_points() {
    let mut net = RoutedRoads::new();
    let a = net.add(v(0.0, 0.0), v(3.0, 0.0), local()).unwrap();
    let b = net.add(v(3.0, 0.0), v(3.0, 3.0), local()).unwrap();
    let first = lane(&net, a, v(0.0, 0.0)).clone();
    let last = lane(&net, b, v(3.0, 0.0)).clone();
    let route = net.route(first.id, 30.0, last.id, 40.0).unwrap();
    let points = route.polyline(&net.roads);
    let near = |p: Vec2, q: Vec2| (p - q).abs().max_element() <= 0.5;
    assert!(near(points[0], first.curve().sample_baked(30.0)));
    assert!(near(
        *points.last().unwrap(),
        last.curve().sample_baked(40.0)
    ));
}
