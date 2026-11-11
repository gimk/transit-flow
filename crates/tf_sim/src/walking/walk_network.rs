//! `WalkNetwork` (`sim/walking/walk_network.gd`): where pedestrians walk, derived from the
//! roads: a sidewalk along each side of every road, corners joining the sidewalks of
//! neighbouring roads around each node, and, at junctions of 3+ roads, a crosswalk
//! across the mouth of each road; plus mid-block crosswalks (at bus stops,
//! `Crosswalks::mid_block`). (A dead end is walked round; a bend or straight
//! continuation just joins its sidewalks.) Rebuilt lazily after any road change;
//! pedestrians cross roads only at crosswalks.
//!
//! Sides: +1 is the right of a road's start → end direction. Around a node, roads are
//! sorted by the angle of their direction away from the node; the sidewalk to the right
//! of that direction faces the next road, so corners join it to the next road's left one.
//!
//! The GDScript class holds the road network and listens to its `changed` signal; here
//! `PedestrianSystem` forwards it to `on_roads_changed` and the queries take the roads
//! and the crosswalks as parameters. The graph is Godot's `AStar2D` (the shared
//! `routing::astar::AStar`): point ids are the GDScript's (1, 2, ... in the order they are
//! added), so point `id` is index `id - 1`, and paths tie-break exactly like Godot's.

use glam::{IVec2, Vec2};
use indexmap::IndexMap;
use std::collections::HashMap;

use crate::geom::GdVec2;
use crate::geom::segment::closest_point_to_segment;
use crate::ids::{NodeId, SegmentId};
use crate::roads::lane_builder;
use crate::roads::{ChangeSet, RoadNetwork, RoadNode, RoadSegment};
use crate::routing::astar::AStar;
use crate::walking::crosswalks::{Crosswalks, NONE};

/// Sidewalks run this far (px) beyond the road's edge.
pub const SIDEWALK_OFFSET: f64 = 5.0;

/// A walking route (the GDScript dictionary `{ "points", "crossings", "length" }`).
#[derive(Clone, Debug, PartialEq)]
pub struct WalkPath {
    pub points: Vec<Vec2>,
    /// Per leg between consecutive points: the crosswalk it crosses, or `NONE`.
    pub crossings: Vec<IVec2>,
    pub length: f64,
}

/// The closest point on any sidewalk (the GDScript `{ "edge", "point" }`).
struct Nearest {
    edge: (i32, i32),
    point: Vec2,
}

#[derive(Debug)]
pub struct WalkNetwork {
    /// Sidewalk graph; point positions are sidewalk ends. Point `id` is index `id - 1`.
    graph: AStar,
    /// Edge (`IVec2(lower point id, higher point id)`) → the crosswalk it is
    /// (`Crosswalks` key).
    pub crosswalk_edges: IndexMap<IVec2, IVec2>,
    /// `IVec2(segment id, side)` → `IVec2(point at the start end, point at the end end)`.
    pub sidewalks: IndexMap<IVec2, IVec2>,
    dirty: bool,
    next_point_id: i32,
    /// Every stretch of sidewalk between two graph points (sidewalks are split where a
    /// mid-block crosswalk meets them).
    edges: Vec<(i32, i32)>,
    /// The `Crosswalks::mid_block_version` the graph was built with.
    built_version: Option<u64>,
}

impl Default for WalkNetwork {
    fn default() -> Self {
        Self::new()
    }
}

impl WalkNetwork {
    pub fn new() -> Self {
        Self {
            graph: AStar::new(),
            crosswalk_edges: IndexMap::new(),
            sidewalks: IndexMap::new(),
            dirty: true,
            next_point_id: 1,
            edges: Vec::new(),
            built_version: None,
        }
    }

    /// `_on_roads_changed`.
    pub fn on_roads_changed(&mut self, _change: &ChangeSet) {
        self.dirty = true;
    }

    /// The walking route between two places on the map (e.g. lot centers): from the
    /// nearest sidewalk to each, through the sidewalk graph. `None` (Godot: `{}`) if they
    /// aren't connected.
    pub fn find_path(
        &mut self,
        roads: &RoadNetwork,
        crosswalks: &Crosswalks,
        from: Vec2,
        to: Vec2,
    ) -> Option<WalkPath> {
        self.ensure_built(roads, crosswalks);
        let start = self.nearest_sidewalk(from)?;
        let goal = self.nearest_sidewalk(to)?;
        if start.edge == goal.edge {
            return Some(self.make_path(&[], start.point, goal.point));
        }
        let mut best: Vec<i32> = Vec::new();
        let mut best_length = f64::INFINITY;
        for a in [start.edge.0, start.edge.1] {
            for b in [goal.edge.0, goal.edge.1] {
                let ids = self.id_path(a, b);
                if ids.is_empty() {
                    continue;
                }
                let mut length = f64::from(start.point.gd_distance_to(self.position(a)))
                    + f64::from(goal.point.gd_distance_to(self.position(b)));
                for i in 1..ids.len() {
                    length += f64::from(
                        self.position(ids[i - 1])
                            .gd_distance_to(self.position(ids[i])),
                    );
                }
                if length < best_length {
                    best_length = length;
                    best = ids;
                }
            }
        }
        if best.is_empty() {
            return None;
        }
        Some(self.make_path(&best, start.point, goal.point))
    }

    /// Walking distance between two places, or infinity if they aren't connected.
    pub fn distance(
        &mut self,
        roads: &RoadNetwork,
        crosswalks: &Crosswalks,
        from: Vec2,
        to: Vec2,
    ) -> f64 {
        self.find_path(roads, crosswalks, from, to)
            .map_or(f64::INFINITY, |path| path.length)
    }

    /// Builds the graph now if the roads (or the mid-block crosswalks) changed since.
    pub fn ensure_built(&mut self, roads: &RoadNetwork, crosswalks: &Crosswalks) {
        if self.dirty || self.built_version != Some(crosswalks.mid_block_version) {
            self.dirty = false;
            self.built_version = Some(crosswalks.mid_block_version);
            self.build(roads, crosswalks);
        }
    }

    /// Every graph point id, in the order they were added (`graph.get_point_ids()`).
    pub fn point_ids(&self) -> std::ops::RangeInclusive<i32> {
        1..=self.graph.point_count() as i32
    }

    /// `graph.get_point_position(id)`.
    pub fn position(&self, id: i32) -> Vec2 {
        self.graph.position(Self::index(id))
    }

    /// `graph.get_point_connections(id)`: neighbouring point ids in connection order.
    pub fn connections(&self, id: i32) -> Vec<i32> {
        self.graph
            .connections(Self::index(id))
            .iter()
            .map(|&i| i as i32 + 1)
            .collect()
    }

    fn index(id: i32) -> usize {
        (id - 1) as usize
    }

    /// `graph.get_id_path(a, b)`: Godot's default cost and estimate are both the
    /// straight-line distance (`f32`).
    fn id_path(&mut self, a: i32, b: i32) -> Vec<i32> {
        self.graph
            .id_path(
                Self::index(a),
                Self::index(b),
                |positions, from, to| positions[from].gd_distance_to(positions[to]),
                |from, to| from.gd_distance_to(to),
            )
            .into_iter()
            .map(|i| i as i32 + 1)
            .collect()
    }

    fn make_path(&self, ids: &[i32], from: Vec2, to: Vec2) -> WalkPath {
        let mut points = vec![from];
        let mut crossings = vec![NONE];
        for (i, &id) in ids.iter().enumerate() {
            points.push(self.position(id));
            let mut crosses = NONE;
            if i + 1 < ids.len() {
                let key = IVec2::new(id.min(ids[i + 1]), id.max(ids[i + 1]));
                crosses = self.crosswalk_edges.get(&key).copied().unwrap_or(NONE);
            }
            crossings.push(crosses);
        }
        points.push(to);
        crossings.truncate(points.len() - 1);
        let mut length = 0.0;
        for i in 1..points.len() {
            length += f64::from(points[i - 1].gd_distance_to(points[i]));
        }
        WalkPath {
            points,
            crossings,
            length,
        }
    }

    /// The closest point on any sidewalk.
    fn nearest_sidewalk(&self, position: Vec2) -> Option<Nearest> {
        let mut best = None;
        let mut best_distance = f64::INFINITY;
        for &(x, y) in &self.edges {
            let a = self.position(x);
            let b = self.position(y);
            let point = closest_point_to_segment(position, a, b);
            let distance = f64::from(point.gd_distance_to(position));
            if distance < best_distance {
                best_distance = distance;
                best = Some(Nearest {
                    edge: (x, y),
                    point,
                });
            }
        }
        best
    }

    fn build(&mut self, roads: &RoadNetwork, crosswalks: &Crosswalks) {
        self.graph.clear();
        self.crosswalk_edges.clear();
        self.sidewalks.clear();
        self.edges.clear();
        self.next_point_id = 1;
        // Segment id → its mid-block crosswalks, as (distance along the road, key).
        let mut mid_block: IndexMap<SegmentId, Vec<(f64, IVec2)>> = IndexMap::new();
        for (key, crossing) in &crosswalks.mid_block {
            mid_block
                .entry(crossing.segment_id)
                .or_default()
                .push((crossing.distance, *key));
        }
        // (segment id, side, node id) → the sidewalk end at that node.
        let mut ends: HashMap<(SegmentId, i32, NodeId), i32> = HashMap::new();
        for segment in roads.segments.values() {
            if segment.elevation_layer != 0 {
                continue;
            }
            let centerline = segment.polyline();
            let offset = segment.road_type.total_width() * 0.5 + SIDEWALK_OFFSET;
            let start_trim = roads.junction_radius(segment.start_node_id);
            let end_trim = roads.junction_radius(segment.end_node_id);
            let length = lane_builder::polyline_length(centerline);
            let mut crossings: Vec<(f64, IVec2)> = mid_block
                .get(&segment.id)
                .map(|list| {
                    list.iter()
                        .copied()
                        .filter(|c| c.0 > start_trim && c.0 < length - end_trim)
                        .collect()
                })
                .unwrap_or_default();
            // GDScript's sort_custom isn't stable; distances along one road don't tie.
            crossings.sort_by(|x, y| x.0.partial_cmp(&y.0).expect("finite distances"));
            // Per crossing: the point on each side where it meets the sidewalk.
            let mut crossing_points = vec![IVec2::ZERO; crossings.len()];
            for side in [-1, 1] {
                let full_line = lane_builder::offset_polyline(centerline, f64::from(side) * offset);
                let line = lane_builder::trim_polyline(&full_line, start_trim, end_trim);
                let a = self.add_point(line[0]);
                let b = self.add_point(*line.last().expect("a trimmed line has points"));
                let mut previous = a;
                for (i, crossing) in crossings.iter().enumerate() {
                    let point = self.add_point(
                        lane_builder::sub_polyline(&full_line, crossing.0, crossing.0)[0],
                    );
                    self.connect(previous, point);
                    previous = point;
                    if side == -1 {
                        crossing_points[i].x = point;
                    } else {
                        crossing_points[i].y = point;
                    }
                }
                self.connect(previous, b);
                self.sidewalks
                    .insert(IVec2::new(segment.id.0, side), IVec2::new(a, b));
                ends.insert((segment.id, side, segment.start_node_id), a);
                ends.insert((segment.id, side, segment.end_node_id), b);
            }
            for (i, crossing) in crossings.iter().enumerate() {
                let across = crossing_points[i];
                self.connect_points(across.x, across.y);
                self.crosswalk_edges.insert(
                    IVec2::new(across.x.min(across.y), across.x.max(across.y)),
                    crossing.1,
                );
            }
        }
        for node in roads.nodes.values() {
            if node.elevation_layer != 0 || node.segment_ids.is_empty() {
                continue;
            }
            let mut arms: Vec<(f32, SegmentId)> = node
                .segment_ids
                .iter()
                .map(|&id| (Self::away_from(node, &roads.segments[&id]).gd_angle(), id))
                .collect();
            arms.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("finite angles"));
            for i in 0..arms.len() {
                let here = arms[i].1;
                let next = arms[(i + 1) % arms.len()].1;
                let right = ends.get(&(here, Self::side(roads, node, here, 1), node.id));
                let left = ends.get(&(next, Self::side(roads, node, next, -1), node.id));
                if let (Some(&right), Some(&left)) = (right, left)
                    && right != left
                {
                    self.connect_points(right, left);
                }
            }
            if node.degree() >= 3 {
                for arm in &arms {
                    let segment_id = arm.1;
                    let a = ends.get(&(segment_id, 1, node.id));
                    let b = ends.get(&(segment_id, -1, node.id));
                    if let (Some(&a), Some(&b)) = (a, b) {
                        self.connect_points(a, b);
                        self.crosswalk_edges.insert(
                            IVec2::new(a.min(b), a.max(b)),
                            IVec2::new(node.id.0, segment_id.0),
                        );
                    }
                }
            }
        }
    }

    fn add_point(&mut self, position: Vec2) -> i32 {
        let id = self.next_point_id;
        self.next_point_id += 1;
        self.graph.add_point(position);
        id
    }

    /// `graph.connect_points(a, b)` (both ways).
    fn connect_points(&mut self, a: i32, b: i32) {
        self.graph.connect_points(Self::index(a), Self::index(b));
    }

    /// Direction of a road leaving `node`.
    fn away_from(node: &RoadNode, segment: &RoadSegment) -> Vec2 {
        let points = segment.polyline();
        if segment.start_node_id == node.id {
            (points[1] - points[0]).gd_normalized()
        } else {
            (points[points.len() - 2] - points[points.len() - 1]).gd_normalized()
        }
    }

    /// The sidewalk side (relative to the road's start → end) that is `away_side` of its
    /// direction away from `node`.
    fn side(roads: &RoadNetwork, node: &RoadNode, segment_id: SegmentId, away_side: i32) -> i32 {
        if roads.segments[&segment_id].start_node_id == node.id {
            away_side
        } else {
            -away_side
        }
    }

    /// Joins two points with a stretch of sidewalk.
    fn connect(&mut self, a: i32, b: i32) {
        self.connect_points(a, b);
        self.edges.push((a, b));
    }
}
