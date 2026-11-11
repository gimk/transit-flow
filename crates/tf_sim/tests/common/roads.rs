//! Helpers for tests that build road networks (the GDScript tests' `_add`, `_type` …).

use glam::Vec2;
use tf_sim::events::{Outbox, SimEvent};
use tf_sim::ids::{LaneId, NodeId, SegmentId};
use tf_sim::roads::lane_builder;
use tf_sim::roads::{ChangeSet, Lane, RoadNetwork, RoadNode, RoadType};

/// One grid cell, in world px.
pub const G: f32 = 64.0;

pub fn road_type(id: &str) -> &'static RoadType {
    RoadType::load_by_id(id).unwrap()
}

pub fn local() -> &'static RoadType {
    road_type("local_2lane")
}

/// A road type that isn't in the data files (`RoadType.new()` in GDScript tests).
pub fn custom(road_type: RoadType) -> &'static RoadType {
    Box::leak(Box::new(road_type))
}

/// The `changed` emissions in an outbox.
pub fn changes(out: &Outbox) -> Vec<&ChangeSet> {
    out.iter()
        .filter_map(|event| match event {
            SimEvent::RoadsChanged(change) => Some(change),
            _ => None,
        })
        .collect()
}

pub fn add(
    roads: &mut RoadNetwork,
    from: Vec2,
    to: Vec2,
    road_type: &'static RoadType,
) -> Option<SegmentId> {
    roads.add_road(from, to, road_type, 0, &mut Outbox::new())
}

/// `add_road_pieces` in grid cells.
pub fn pieces(
    roads: &mut RoadNetwork,
    from: Vec2,
    to: Vec2,
    road_type: &'static RoadType,
) -> Vec<SegmentId> {
    roads.add_road_pieces(from * G, to * G, road_type, 0, &mut Outbox::new())
}

pub fn remove(roads: &mut RoadNetwork, id: SegmentId) -> bool {
    roads.remove_segment(id, &mut Outbox::new())
}

pub fn node_at(roads: &RoadNetwork, at: Vec2) -> &RoadNode {
    &roads.nodes[&roads.find_node_at(at * G, 0).expect("a node there")]
}

pub fn lanes_of(roads: &RoadNetwork, segment_id: SegmentId) -> Vec<&Lane> {
    roads.segments[&segment_id]
        .lane_ids
        .iter()
        .map(|id| &roads.lanes[id])
        .collect()
}

pub fn connectors(roads: &RoadNetwork, at: Vec2) -> Vec<&Lane> {
    node_at(roads, at)
        .connector_lane_ids
        .iter()
        .map(|id| &roads.lanes[id])
        .collect()
}

pub fn end_direction(lane: &Lane) -> Vec2 {
    lane_builder::end_direction(lane.curve())
}

/// Every invariant the rest of the game relies on (`test_road_splitting.gd`).
pub fn assert_graph_valid(roads: &RoadNetwork) {
    for segment in roads.segments.values() {
        let start = &roads.nodes[&segment.start_node_id];
        let end = &roads.nodes[&segment.end_node_id];
        assert!(start.segment_ids.contains(&segment.id));
        assert!(end.segment_ids.contains(&segment.id));
        let points = lane_builder::get_polyline(segment.centerline());
        assert!(points[0].distance(start.position) < 0.01);
        assert!(points[points.len() - 1].distance(end.position) < 0.01);
        assert!(segment.length() >= RoadNetwork::MIN_SEGMENT_LENGTH);
        for lane_id in &segment.lane_ids {
            assert!(roads.lanes.contains_key(lane_id));
        }
        // Segments on one layer only meet at their ends.
        for other in roads.segments.values() {
            if other.id != segment.id && other.elevation_layer == segment.elevation_layer {
                let mid = (points[0] + points[points.len() - 1]) * 0.5;
                assert!(
                    lane_builder::distance_to_polyline(
                        mid,
                        &lane_builder::get_polyline(other.centerline())
                    ) > RoadNetwork::EPSILON,
                    "segments {} and {} overlap",
                    segment.id,
                    other.id
                );
            }
        }
    }
    for node in roads.nodes.values() {
        assert!(node.degree() > 0, "no orphan nodes");
        for lane_id in &node.connector_lane_ids {
            assert!(roads.lanes.contains_key(lane_id));
        }
    }
    for lane in roads.lanes.values() {
        for next_id in &lane.outgoing_lane_ids {
            assert!(roads.lanes.contains_key(next_id), "no dangling lane links");
        }
    }
}

pub fn ids(list: &[SegmentId]) -> Vec<i32> {
    list.iter().map(|id| id.0).collect()
}

pub fn lane_ids(list: &[LaneId]) -> Vec<i32> {
    list.iter().map(|id| id.0).collect()
}

pub fn node_ids(list: &[NodeId]) -> Vec<i32> {
    list.iter().map(|id| id.0).collect()
}

/// A road network with a router listening to it, the way `Simulation` wires them
/// (`Router.new(roads)` in GDScript tests).
pub struct RoutedRoads {
    pub roads: RoadNetwork,
    pub router: tf_sim::routing::Router,
}

impl RoutedRoads {
    pub fn new() -> Self {
        Self {
            roads: RoadNetwork::new(),
            router: tf_sim::routing::Router::new(),
        }
    }

    /// Hands the router every `changed` the network emitted.
    pub fn deliver(&mut self, out: Outbox) {
        for change in changes(&out) {
            self.router.on_roads_changed(change, &mut Outbox::new());
        }
    }

    pub fn add(&mut self, from: Vec2, to: Vec2, road_type: &'static RoadType) -> Option<SegmentId> {
        let mut out = Outbox::new();
        let id = self
            .roads
            .add_road(from * G, to * G, road_type, 0, &mut out);
        self.deliver(out);
        id
    }

    pub fn pieces(&mut self, from: Vec2, to: Vec2, road_type: &'static RoadType) -> Vec<SegmentId> {
        let mut out = Outbox::new();
        let ids = self
            .roads
            .add_road_pieces(from * G, to * G, road_type, 0, &mut out);
        self.deliver(out);
        ids
    }

    pub fn remove(&mut self, id: SegmentId) -> bool {
        let mut out = Outbox::new();
        let removed = self.roads.remove_segment(id, &mut out);
        self.deliver(out);
        removed
    }

    pub fn route(
        &mut self,
        from: LaneId,
        from_distance: f64,
        to: LaneId,
        to_distance: f64,
    ) -> Option<tf_sim::routing::Route> {
        self.router
            .find_route(&self.roads, from, from_distance, to, to_distance, false)
    }
}
