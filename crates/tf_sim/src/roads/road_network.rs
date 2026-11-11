//! `RoadNetwork` (`sim/roads/road_network.gd`): the authoritative road graph: nodes,
//! segments, and the lanes derived from them. Every edit goes through a command method
//! (`add_road`, `remove_segment` …), and every command emits `RoadsChanged` with the
//! node and segment ids it touched, so views and routing can update incrementally.
//!
//! Lanes: each segment owns road lanes (stable ids for the segment's lifetime), trimmed
//! back from junction nodes. Each node owns connector lanes linking its incoming lanes
//! to its outgoing lanes (rebuilt, with new ids, whenever the junction changes). Dead
//! ends get a U-turn connector so no lane is a trap.
//!
//! Building a road that crosses, touches or passes through existing roads on the same
//! elevation layer splits everything at those points and creates junction nodes there.
//!
//! Removed lanes. In Godot, vehicles hold `Lane` objects and keep driving a lane the
//! network has dropped (a replaced connector is finished, then the vehicle goes on from
//! the lane it fed). Here vehicles hold ids, so removed lanes move to `retired_lanes`
//! instead of disappearing; `lane_any` finds them there, and `VehicleSystem` drops the
//! ones no vehicle is on (`prune_retired_lanes`). Their `outgoing_lane_ids` are kept, as
//! in Godot.

use glam::Vec2;
use indexmap::IndexMap;
use serde_json::{Value, json};

use super::lane::{Lane, Turn};
use super::lane_builder;
use super::road_node::{JunctionControl, RoadNode};
use super::road_segment::RoadSegment;
use super::road_type::RoadType;
use crate::events::{Outbox, SimEvent};
use crate::geom::segment::{closest_point_to_segment, segment_intersects_segment};
use crate::geom::{Curve2D, CurvePoint, GdVec2};
use crate::ids::{LaneId, NodeId, SegmentId};
use crate::traffic::vehicle;

/// What `RoadNetwork.changed(node_ids, segment_ids)` carries: the nodes and segments an
/// edit touched. Ids may refer to things that were just removed, and may repeat (as in
/// Godot).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
    pub node_ids: Vec<NodeId>,
    pub segment_ids: Vec<SegmentId>,
}

/// How a road along a path would be built (`plan_path`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadPlan {
    /// "" or a player-facing reason it can't be built.
    pub error: String,
    /// Where the road will have nodes, in order (its ends plus every point where it
    /// meets an existing road or node on the same layer).
    pub points: Vec<Vec2>,
    /// How far along the path each of those points is.
    pub distances: Vec<f64>,
    /// Per consecutive pair of points: true if that piece runs along an existing road
    /// and won't be built again.
    pub overlaps: Vec<bool>,
    /// Per piece: true if it runs along an existing road that will be converted to the
    /// road type (another type, or a one-way road to reverse); only when one is given.
    pub upgrades: Vec<bool>,
    pub path: Vec<Vec2>,
    /// Empty; filled in by `add_path_pieces`.
    pub node_ids: Vec<NodeId>,
}

/// A position on a road lane (`find_lane_position`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LanePosition {
    pub lane_id: LaneId,
    pub distance: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoadNetwork {
    pub nodes: IndexMap<NodeId, RoadNode>,
    pub segments: IndexMap<SegmentId, RoadSegment>,
    pub lanes: IndexMap<LaneId, Lane>,
    /// Lanes removed from the network that vehicles may still be on (see the module doc).
    pub retired_lanes: IndexMap<LaneId, Lane>,
    next_node_id: i32,
    next_segment_id: i32,
    next_lane_id: i32,
}

impl Default for RoadNetwork {
    fn default() -> Self {
        Self {
            nodes: IndexMap::new(),
            segments: IndexMap::new(),
            lanes: IndexMap::new(),
            retired_lanes: IndexMap::new(),
            next_node_id: 1,
            next_segment_id: 1,
            next_lane_id: 1,
        }
    }
}

impl RoadNetwork {
    /// Positions closer than this are treated as the same point.
    pub const EPSILON: f64 = 0.5;
    /// No segment may be shorter than this (it would leave no room between junctions).
    pub const MIN_SEGMENT_LENGTH: f64 = 16.0;
    /// Curved roads: sampled every CURVE_STEP px, and no tighter than MIN_CURVE_RADIUS
    /// (a quarter turn within one grid square is too tight; within two, fine).
    pub const CURVE_STEP: f64 = 8.0;
    pub const MIN_CURVE_RADIUS: f64 = 56.0;
    /// Sideways acceleration drivers accept in a curve (px/s²): a curve of radius 64 px
    /// is taken at ~72 px/s, one of 128 px at a local street's full 90 px/s.
    pub const CURVE_LATERAL_ACCELERATION: f64 = 80.0;
    /// Lanes between junctions must at least fit the longest vehicle (a truck) and a gap.
    pub const MIN_LANE_LENGTH: f64 = 21.0;
    pub const TOO_CLOSE_ERROR: &'static str =
        "Junctions too close together for these roads (wider roads need more room)";
    /// Connectors whose paths come closer than this (center to center) conflict: two
    /// vehicles (up to `TRUCK_WIDTH` wide) on them at once could touch.
    pub const CONFLICT_DISTANCE: f64 = 8.0;
    /// A connector counts as going straight if it turns by less than this (30°).
    pub const STRAIGHT_MAX_ANGLE: f64 = std::f64::consts::PI / 6.0;

    /// `TURN_SPEED_FACTORS`: connector speed limit relative to the slower of the two
    /// lanes it joins.
    pub fn turn_speed_factor(turn: Turn) -> f64 {
        match turn {
            Turn::Straight => 1.0,
            Turn::Right => 0.5,
            Turn::Left => 0.6,
            Turn::UTurn => 0.3,
            Turn::None => unreachable!("connectors always turn"),
        }
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// A lane by id, live or retired (vehicles may still be on a retired one).
    pub fn lane_any(&self, id: LaneId) -> Option<&Lane> {
        self.lanes.get(&id).or_else(|| self.retired_lanes.get(&id))
    }

    /// Drops the retired lanes `keep` says no one needs any more.
    pub fn prune_retired_lanes(&mut self, keep: impl Fn(LaneId) -> bool) {
        self.retired_lanes.retain(|&id, _| keep(id));
    }

    fn retire_lane(&mut self, id: LaneId) {
        if let Some(lane) = self.lanes.shift_remove(&id) {
            self.retired_lanes.insert(id, lane);
        }
    }

    // --- Commands -------------------------------------------------------------------

    /// Adds a straight road, splitting it and any existing road it meets into separate
    /// segments joined at junctions. Returns the first new segment id, or `None` if
    /// invalid.
    pub fn add_road(
        &mut self,
        from: Vec2,
        to: Vec2,
        road_type: &'static RoadType,
        elevation_layer: i32,
        out: &mut Outbox,
    ) -> Option<SegmentId> {
        self.add_road_pieces(from, to, road_type, elevation_layer, out)
            .first()
            .copied()
    }

    /// Like `add_road`, but returns the id of every segment built or rebuilt (in order
    /// from `from` to `to`). Pieces running along an existing road of another type are
    /// converted to `road_type` in place (an upgrade), and along a one-way road pointing
    /// the other way, reversed; pieces already of that type and direction are left alone.
    pub fn add_road_pieces(
        &mut self,
        from: Vec2,
        to: Vec2,
        road_type: &'static RoadType,
        elevation_layer: i32,
        out: &mut Outbox,
    ) -> Vec<SegmentId> {
        self.add_path_pieces(&[from, to], road_type, elevation_layer, out)
    }

    /// Adds a curved road from `from` to `to`, bending toward `control` (see
    /// `curve_path`). Returns the id of every segment built or rebuilt, as
    /// `add_road_pieces`.
    pub fn add_curved_road(
        &mut self,
        from: Vec2,
        control: Vec2,
        to: Vec2,
        road_type: &'static RoadType,
        elevation_layer: i32,
        out: &mut Outbox,
    ) -> Vec<SegmentId> {
        self.add_path_pieces(
            &Self::curve_path(from, control, to),
            road_type,
            elevation_layer,
            out,
        )
    }

    /// Builds a road along `path` (see `plan_path`), splitting it where it meets other
    /// roads.
    pub fn add_path_pieces(
        &mut self,
        path: &[Vec2],
        road_type: &'static RoadType,
        elevation_layer: i32,
        out: &mut Outbox,
    ) -> Vec<SegmentId> {
        let mut plan = self.plan_path(path, elevation_layer, Some(road_type));
        let mut new_ids = Vec::new();
        if !plan.error.is_empty() {
            return new_ids;
        }
        let mut touched_segments: Vec<SegmentId> = Vec::new();
        let mut junction_ids: Vec<NodeId> = Vec::new();
        for &point in &plan.points {
            let node_id = match self.find_node_at(point, elevation_layer) {
                Some(node_id) => node_id,
                None => match self.find_segment_with_interior_point(point, elevation_layer) {
                    Some(split_id) => {
                        let old = &self.segments[&split_id];
                        junction_ids.extend([old.start_node_id, old.end_node_id]);
                        touched_segments.push(split_id);
                        let node_id = self.split_segment(split_id, point);
                        touched_segments.extend(self.nodes[&node_id].segment_ids.iter().copied());
                        node_id
                    }
                    None => self.create_node(point, elevation_layer),
                },
            };
            junction_ids.push(node_id);
            plan.node_ids.push(node_id);
        }

        for i in 0..plan.overlaps.len() {
            let start_id = plan.node_ids[i];
            let end_id = plan.node_ids[i + 1];
            if plan.overlaps[i] {
                if plan.upgrades[i] {
                    let existing_id = self
                        .segment_between(start_id, end_id)
                        .expect("an overlapping piece runs along a segment");
                    let reverse =
                        road_type.one_way && self.segments[&existing_id].start_node_id != start_id;
                    self.retype(existing_id, road_type, reverse);
                    touched_segments.push(existing_id);
                    new_ids.push(existing_id);
                }
                continue;
            }
            let centerline = Self::piece_curve(
                path,
                plan.distances[i],
                plan.distances[i + 1],
                self.nodes[&start_id].position,
                self.nodes[&end_id].position,
            );
            let id = SegmentId(self.next_segment_id);
            self.insert_segment(id, start_id, end_id, road_type, elevation_layer, centerline);
            self.next_segment_id += 1;
            new_ids.push(id);
        }

        for segment_id in self.refresh_junctions(&junction_ids) {
            if !touched_segments.contains(&segment_id) {
                touched_segments.push(segment_id);
            }
        }
        out.push(SimEvent::RoadsChanged(ChangeSet {
            node_ids: junction_ids,
            segment_ids: touched_segments,
        }));
        new_ids
    }

    /// Changes a segment's road type in place (and, if `reverse`, swaps its direction,
    /// which matters for one-way roads). Its nodes, junctions and the lots along it stay;
    /// its lanes are rebuilt with new ids, so vehicles on it re-route.
    pub fn set_segment_type(
        &mut self,
        segment_id: SegmentId,
        road_type: &'static RoadType,
        reverse: bool,
        out: &mut Outbox,
    ) -> bool {
        let Some(segment) = self.segments.get(&segment_id) else {
            return false;
        };
        if std::ptr::eq(segment.road_type, road_type) && !reverse {
            return false;
        }
        self.retype(segment_id, road_type, reverse);
        let segment = &self.segments[&segment_id];
        let touched_nodes = vec![segment.start_node_id, segment.end_node_id];
        let mut touched_segments = self.refresh_junctions(&touched_nodes);
        if !touched_segments.contains(&segment_id) {
            touched_segments.push(segment_id);
        }
        out.push(SimEvent::RoadsChanged(ChangeSet {
            node_ids: touched_nodes,
            segment_ids: touched_segments,
        }));
        true
    }

    /// Sets how vehicles take turns at a junction. Ignored (returns false) for nodes
    /// where fewer than 3 roads meet.
    pub fn set_node_control(
        &mut self,
        node_id: NodeId,
        control: JunctionControl,
        out: &mut Outbox,
    ) -> bool {
        let Some(node) = self.nodes.get_mut(&node_id) else {
            return false;
        };
        if control != JunctionControl::None && node.degree() < 3 {
            return false;
        }
        node.control = control;
        let segment_ids = self.refresh_junctions(&[node_id]);
        out.push(SimEvent::RoadsChanged(ChangeSet {
            node_ids: vec![node_id],
            segment_ids,
        }));
        true
    }

    /// Removes a segment and its lanes. Nodes left without any segment are removed too.
    pub fn remove_segment(&mut self, segment_id: SegmentId, out: &mut Outbox) -> bool {
        let Some(segment) = self.segments.get(&segment_id) else {
            return false;
        };
        let touched_nodes = vec![segment.start_node_id, segment.end_node_id];
        self.detach_segment(segment_id);
        for &node_id in &touched_nodes {
            if self
                .nodes
                .get(&node_id)
                .is_some_and(|node| node.segment_ids.is_empty())
            {
                self.erase_connectors(node_id);
                self.nodes.shift_remove(&node_id);
            }
        }
        let mut touched_segments = self.refresh_junctions(&touched_nodes);
        touched_segments.push(segment_id);
        out.push(SimEvent::RoadsChanged(ChangeSet {
            node_ids: touched_nodes,
            segment_ids: touched_segments,
        }));
        true
    }

    // --- Queries --------------------------------------------------------------------

    /// "" if the road can be built, otherwise a player-facing reason.
    pub fn validate_road(&self, from: Vec2, to: Vec2, elevation_layer: i32) -> String {
        self.plan_road(from, to, elevation_layer, None).error
    }

    /// Works out, without changing anything, how a straight road from `from` to `to`
    /// would be built: see `plan_path`.
    pub fn plan_road(
        &self,
        from: Vec2,
        to: Vec2,
        elevation_layer: i32,
        road_type: Option<&'static RoadType>,
    ) -> RoadPlan {
        self.plan_path(&[from, to], elevation_layer, road_type)
    }

    /// Works out, without changing anything, how a road along `path` (a polyline: two
    /// points for a straight road, many for a curve, see `curve_path`) would be built.
    pub fn plan_path(
        &self,
        path: &[Vec2],
        elevation_layer: i32,
        road_type: Option<&'static RoadType>,
    ) -> RoadPlan {
        let mut plan = RoadPlan {
            path: path.to_vec(),
            ..RoadPlan::default()
        };
        let length = lane_builder::polyline_length(path);
        if length < Self::MIN_SEGMENT_LENGTH {
            plan.error = "Road is too short".into();
            return plan;
        }
        if path.len() > 2 && Self::min_radius(path) < Self::MIN_CURVE_RADIUS {
            plan.error = "Curve too tight (drivers need a wider turn)".into();
            return plan;
        }

        // (distance along the path, point)
        let mut found: Vec<(f64, Vec2)> = vec![(0.0, path[0]), (length, path[path.len() - 1])];
        for node in self.nodes.values() {
            if node.elevation_layer == elevation_layer
                && lane_builder::distance_to_polyline(node.position, path) <= Self::EPSILON
            {
                found.push((
                    lane_builder::distance_along_polyline(path, node.position),
                    node.position,
                ));
            }
        }
        for segment in self.segments.values() {
            if segment.elevation_layer != elevation_layer {
                continue;
            }
            let polyline = lane_builder::get_polyline(segment.centerline());
            let mut meets = false;
            for endpoint in [path[0], path[path.len() - 1]] {
                if Self::is_on_interior(endpoint, &polyline) {
                    meets = true;
                }
            }
            for i in 0..path.len() - 1 {
                for j in 0..polyline.len() - 1 {
                    if let Some(hit) = segment_intersects_segment(
                        path[i],
                        path[i + 1],
                        polyline[j],
                        polyline[j + 1],
                    ) {
                        found.push((lane_builder::distance_along_polyline(path, hit), hit));
                        if Self::is_on_interior(hit, &polyline) {
                            meets = true;
                        }
                    }
                }
            }
            if meets && Self::is_curved(segment) {
                plan.error = "Can't join a curved road in the middle yet".into();
                return plan;
            }
        }

        // Godot's sort_custom is an insertion sort (so stable) on lists this short.
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (distance, point) in found {
            if plan
                .points
                .last()
                .is_none_or(|last: &Vec2| f64::from(last.gd_distance_to(point)) > Self::EPSILON)
            {
                plan.points.push(point);
                plan.distances.push(distance);
            }
        }

        let mut builds_something = false;
        for i in 0..plan.points.len() - 1 {
            let from_distance = plan.distances[i];
            let to_distance = plan.distances[i + 1];
            if to_distance - from_distance < Self::MIN_SEGMENT_LENGTH {
                plan.error = "Too close to an existing junction".into();
                return plan;
            }
            // Pieces only meet other roads at their ends, so a piece whose middle lies on
            // a road runs along that road.
            let middle = Self::point_along(path, (from_distance + to_distance) * 0.5);
            let existing_id = self.find_segment_at(middle, Self::EPSILON, elevation_layer);
            let overlaps = existing_id.is_some();
            let direction = plan.points[i + 1] - plan.points[i];
            let upgrade = match (existing_id, road_type) {
                (Some(existing_id), Some(road_type)) => {
                    self.needs_retype(&self.segments[&existing_id], road_type, direction)
                }
                _ => false,
            };
            plan.overlaps.push(overlaps);
            plan.upgrades.push(upgrade);
            builds_something = builds_something || !overlaps || upgrade;
        }
        if !builds_something {
            plan.error = "Road already exists".into();
        } else if let Some(road_type) = road_type {
            plan.error = self.check_lane_room(&plan, road_type, elevation_layer);
        }
        plan
    }

    /// "" if, once the planned road is built, every lane it creates, converts or
    /// shortens still fits the longest vehicle between its junctions (lanes are cut back
    /// from each junction by half its widest road: two avenue junctions one cell apart
    /// would leave 8 px, which no vehicle fits, and traffic there would crawl through one
    /// at a time).
    fn check_lane_room(
        &self,
        plan: &RoadPlan,
        road_type: &RoadType,
        elevation_layer: i32,
    ) -> String {
        let points = &plan.points;
        let count = points.len();
        let half_width = road_type.total_width() * 0.5;
        // How far lanes will be cut back at each planned point (0 at a dead end).
        let mut trims: Vec<f64> = Vec::with_capacity(count);
        for (i, &point) in points.iter().enumerate() {
            let mut degree = 0;
            let mut widest = 0.0f64;
            if let Some(node_id) = self.find_node_at(point, elevation_layer) {
                let node = &self.nodes[&node_id];
                degree = node.degree();
                widest = if degree >= 2 {
                    self.junction_radius(node_id)
                } else {
                    self.segments[&node.segment_ids[0]].road_type.total_width() * 0.5
                };
            } else if let Some(split_id) =
                self.find_segment_with_interior_point(point, elevation_layer)
            {
                degree = 2;
                widest = self.segments[&split_id].road_type.total_width() * 0.5;
            }
            for piece in [i as i64 - 1, i as i64] {
                if piece < 0 || piece >= count as i64 - 1 {
                    continue;
                }
                let piece = piece as usize;
                if !plan.overlaps[piece] {
                    degree += 1;
                }
                if !plan.overlaps[piece] || plan.upgrades[piece] {
                    widest = widest.max(half_width);
                }
            }
            trims.push(if degree >= 2 { widest } else { 0.0 });
        }
        // The new road's own pieces (and those it converts).
        for i in 0..count - 1 {
            if plan.overlaps[i] && !plan.upgrades[i] {
                continue;
            }
            if plan.distances[i + 1] - plan.distances[i] - trims[i] - trims[i + 1]
                < Self::MIN_LANE_LENGTH
            {
                return Self::TOO_CLOSE_ERROR.into();
            }
        }
        // Existing roads it meets or splits: their stretches between (old and new)
        // junctions.
        for segment in self.segments.values() {
            if segment.elevation_layer != elevation_layer {
                continue;
            }
            let polyline = lane_builder::get_polyline(segment.centerline());
            let start = self.nodes[&segment.start_node_id].position;
            let end = self.nodes[&segment.end_node_id].position;
            // (distance along the segment, trim there)
            let mut stops: Vec<(f64, f64)> = vec![
                (0.0, self.junction_radius(segment.start_node_id)),
                (segment.length(), self.junction_radius(segment.end_node_id)),
            ];
            let mut touched = false;
            for i in 0..count {
                if f64::from(points[i].gd_distance_to(start)) <= Self::EPSILON {
                    stops[0].1 = trims[i];
                    touched = true;
                } else if f64::from(points[i].gd_distance_to(end)) <= Self::EPSILON {
                    stops[1].1 = trims[i];
                    touched = true;
                } else if Self::is_on_interior(points[i], &polyline) {
                    stops.push((
                        lane_builder::distance_along_polyline(&polyline, points[i]),
                        trims[i],
                    ));
                    touched = true;
                }
            }
            if !touched {
                continue;
            }
            stops.sort_by(|a, b| a.0.total_cmp(&b.0));
            for k in 0..stops.len() - 1 {
                if stops[k + 1].0 - stops[k].0 - stops[k].1 - stops[k + 1].1 < Self::MIN_LANE_LENGTH
                {
                    return Self::TOO_CLOSE_ERROR.into();
                }
            }
        }
        String::new()
    }

    /// The node at `position` on the given layer.
    pub fn find_node_at(&self, position: Vec2, elevation_layer: i32) -> Option<NodeId> {
        self.nodes
            .values()
            .find(|node| {
                node.elevation_layer == elevation_layer
                    && f64::from(node.position.gd_distance_to(position)) <= Self::EPSILON
            })
            .map(|node| node.id)
    }

    /// The segment whose centerline is closest to `position` within `max_distance` (the
    /// last one, if several are as close).
    pub fn find_segment_at(
        &self,
        position: Vec2,
        max_distance: f64,
        elevation_layer: i32,
    ) -> Option<SegmentId> {
        let mut best_id = None;
        let mut best_distance = max_distance;
        for segment in self.segments.values() {
            if segment.elevation_layer != elevation_layer {
                continue;
            }
            let distance = lane_builder::distance_to_polyline(
                position,
                &lane_builder::get_polyline(segment.centerline()),
            );
            if distance <= best_distance {
                best_id = Some(segment.id);
                best_distance = distance;
            }
        }
        best_id
    }

    /// Nearest road lane (not connector) to `position` within `max_distance`.
    pub fn find_lane_position(
        &self,
        position: Vec2,
        max_distance: f64,
        elevation_layer: i32,
    ) -> Option<LanePosition> {
        let mut best = None;
        let mut best_distance = max_distance;
        for segment in self.segments.values() {
            if segment.elevation_layer != elevation_layer {
                continue;
            }
            // Skip roads too far away to have a lane in reach (most of them).
            let reach = max_distance + segment.road_type.total_width() * 0.5;
            if !segment.bounds().grow(reach as f32).has_point(position) {
                continue;
            }
            for lane_id in &segment.lane_ids {
                let lane = &self.lanes[lane_id];
                let distance = f64::from(
                    lane.curve()
                        .closest_point(position)
                        .gd_distance_to(position),
                );
                if distance <= best_distance {
                    best_distance = distance;
                    best = Some(LanePosition {
                        lane_id: lane.id,
                        distance: f64::from(lane.curve().closest_offset(position)),
                    });
                }
            }
        }
        best
    }

    /// How far lanes are cut back from a node to leave room for its junction: half the
    /// width of the widest road meeting there. Dead ends are not trimmed.
    pub fn junction_radius(&self, node_id: NodeId) -> f64 {
        let node = &self.nodes[&node_id];
        if node.degree() < 2 {
            return 0.0;
        }
        node.segment_ids
            .iter()
            .map(|id| self.segments[id].road_type.total_width() * 0.5)
            .fold(0.0, f64::max)
    }

    pub fn segment_between(&self, node_a: NodeId, node_b: NodeId) -> Option<SegmentId> {
        let node = self.nodes.get(&node_a)?;
        node.segment_ids
            .iter()
            .copied()
            .find(|id| self.segments[id].other_node_id(node_a) == node_b)
    }

    pub fn straight_curve(from: Vec2, to: Vec2) -> Curve2D {
        Curve2D::from_points(&[from, to])
    }

    /// The centerline of the piece of `path` between two distances along it, ending
    /// exactly on its nodes: a straight curve (two points) where the path is straight
    /// there.
    fn piece_curve(
        path: &[Vec2],
        from_distance: f64,
        to_distance: f64,
        start: Vec2,
        end: Vec2,
    ) -> Curve2D {
        if path.len() == 2 {
            return Self::straight_curve(start, end);
        }
        let mut points = lane_builder::sub_polyline(path, from_distance, to_distance);
        points[0] = start;
        let last = points.len() - 1;
        points[last] = end;
        lane_builder::polyline_to_curve(&points)
    }

    /// A curved road's path: a quadratic Bézier from `from` to `to` pulled toward
    /// `control` (the road leaves `from` heading for `control` and arrives at `to`
    /// coming from it, so a control point on the corner of a grid square gives a smooth
    /// quarter turn), sampled into a polyline every CURVE_STEP px.
    pub fn curve_path(from: Vec2, control: Vec2, to: Vec2) -> Vec<Vec2> {
        let estimate =
            f64::from(from.gd_distance_to(control)) + f64::from(control.gd_distance_to(to));
        let steps = 2.max((estimate / Self::CURVE_STEP).ceil() as i64);
        (0..=steps)
            .map(|i| {
                let t = (i as f64 / steps as f64) as f32;
                from.gd_lerp(control, t).gd_lerp(control.gd_lerp(to, t), t)
            })
            .collect()
    }

    /// The tightest radius of a polyline's turns: the circle through each three
    /// consecutive points (infinite where it runs straight).
    pub fn min_radius(points: &[Vec2]) -> f64 {
        let mut tightest = f64::INFINITY;
        for i in 1..points.len().saturating_sub(1) {
            let (a, b, c) = (points[i - 1], points[i], points[i + 1]);
            let area = f64::from((b - a).gd_cross(c - a)).abs();
            if area < 0.0001 {
                continue;
            }
            tightest = tightest.min(
                f64::from(a.gd_distance_to(b))
                    * f64::from(b.gd_distance_to(c))
                    * f64::from(c.gd_distance_to(a))
                    / (2.0 * area),
            );
        }
        tightest
    }

    fn point_along(points: &[Vec2], distance: f64) -> Vec2 {
        lane_builder::sub_polyline(points, distance, distance)[0]
    }

    // --- Serialization --------------------------------------------------------------
    // Lanes are derived data: they are rebuilt on load, never saved.

    /// The same JSON as Godot's `to_dict`.
    pub fn to_dict(&self) -> Value {
        let nodes: Vec<Value> = self
            .nodes
            .values()
            .map(|node| {
                json!({
                    "id": node.id.0,
                    "position": [node.position.x, node.position.y],
                    "layer": node.elevation_layer,
                    "control": control_name(node.control),
                })
            })
            .collect();
        let segments: Vec<Value> = self
            .segments
            .values()
            .map(|segment| {
                let points: Vec<Value> = segment
                    .centerline()
                    .points()
                    .iter()
                    .map(|p| {
                        json!([
                            p.position.x,
                            p.position.y,
                            p.point_in.x,
                            p.point_in.y,
                            p.point_out.x,
                            p.point_out.y
                        ])
                    })
                    .collect();
                json!({
                    "id": segment.id.0,
                    "start": segment.start_node_id.0,
                    "end": segment.end_node_id.0,
                    "type": segment.road_type.id,
                    "layer": segment.elevation_layer,
                    "centerline": points,
                })
            })
            .collect();
        json!({
            "nodes": nodes,
            "segments": segments,
            "next_node_id": self.next_node_id,
            "next_segment_id": self.next_segment_id,
        })
    }

    /// Loads Godot's `to_dict` JSON. Segments of unknown road types are skipped (Godot
    /// logs a warning).
    pub fn from_dict(data: &Value) -> RoadNetwork {
        let number = |value: &Value| value.as_f64().unwrap_or(0.0);
        let int = |value: &Value| number(value) as i32;
        let mut network = RoadNetwork::new();
        for entry in data["nodes"].as_array().into_iter().flatten() {
            let id = NodeId(int(&entry["id"]));
            let position = Vec2::new(
                number(&entry["position"][0]) as f32,
                number(&entry["position"][1]) as f32,
            );
            let mut node = RoadNode::new(id, position, int(&entry["layer"]));
            node.control = control_from_save(entry.get("control").unwrap_or(&Value::from("none")));
            network.nodes.insert(id, node);
        }
        for entry in data["segments"].as_array().into_iter().flatten() {
            let Some(road_type) = entry["type"].as_str().and_then(RoadType::load_by_id) else {
                continue;
            };
            let points = entry["centerline"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| {
                    let f = |i: usize| number(&p[i]) as f32;
                    CurvePoint {
                        position: Vec2::new(f(0), f(1)),
                        point_in: Vec2::new(f(2), f(3)),
                        point_out: Vec2::new(f(4), f(5)),
                    }
                })
                .collect();
            network.insert_segment(
                SegmentId(int(&entry["id"])),
                NodeId(int(&entry["start"])),
                NodeId(int(&entry["end"])),
                road_type,
                int(&entry["layer"]),
                Curve2D::new(points),
            );
        }
        let node_ids: Vec<NodeId> = network.nodes.keys().copied().collect();
        network.refresh_junctions(&node_ids);
        network.next_node_id = data.get("next_node_id").map_or(1, int);
        network.next_segment_id = data.get("next_segment_id").map_or(1, int);
        network
    }

    // --- Internals ------------------------------------------------------------------

    fn create_node(&mut self, position: Vec2, elevation_layer: i32) -> NodeId {
        let id = NodeId(self.next_node_id);
        self.next_node_id += 1;
        self.nodes
            .insert(id, RoadNode::new(id, position, elevation_layer));
        id
    }

    /// Splits a segment at `point` (on its interior) into two segments joined by a new
    /// node, keeping the original direction. Returns the new node id. The caller must
    /// refresh the junctions at both original ends and at the new node.
    fn split_segment(&mut self, segment_id: SegmentId, point: Vec2) -> NodeId {
        let old = self.segments[&segment_id].clone();
        let polyline = lane_builder::get_polyline(old.centerline());
        let at = lane_builder::distance_along_polyline(&polyline, point);
        let mut first = lane_builder::sub_polyline(&polyline, 0.0, at);
        let mut second =
            lane_builder::sub_polyline(&polyline, at, lane_builder::polyline_length(&polyline));
        let last = first.len() - 1;
        first[last] = point;
        second[0] = point;
        self.detach_segment(segment_id);
        let node_id = self.create_node(point, old.elevation_layer);
        for (start, end, points) in [
            (old.start_node_id, node_id, first),
            (node_id, old.end_node_id, second),
        ] {
            let id = SegmentId(self.next_segment_id);
            self.insert_segment(
                id,
                start,
                end,
                old.road_type,
                old.elevation_layer,
                lane_builder::polyline_to_curve(&points),
            );
            self.next_segment_id += 1;
        }
        node_id
    }

    /// Removes a segment and its lanes from the graph, leaving its nodes in place.
    fn detach_segment(&mut self, segment_id: SegmentId) {
        let segment = self
            .segments
            .shift_remove(&segment_id)
            .expect("detaching an existing segment");
        for &lane_id in &segment.lane_ids {
            self.retire_lane(lane_id);
        }
        for node_id in [segment.start_node_id, segment.end_node_id] {
            let node = self.nodes.get_mut(&node_id).expect("segment nodes exist");
            if let Some(i) = node.segment_ids.iter().position(|&id| id == segment_id) {
                node.segment_ids.remove(i);
            }
        }
    }

    fn find_segment_with_interior_point(
        &self,
        point: Vec2,
        elevation_layer: i32,
    ) -> Option<SegmentId> {
        self.segments
            .values()
            .find(|segment| {
                segment.elevation_layer == elevation_layer
                    && Self::is_on_interior(
                        point,
                        &lane_builder::get_polyline(segment.centerline()),
                    )
            })
            .map(|segment| segment.id)
    }

    fn is_curved(segment: &RoadSegment) -> bool {
        lane_builder::has_handles(segment.centerline())
    }

    #[allow(dead_code)]
    fn distance_to_line(point: Vec2, from: Vec2, to: Vec2) -> f64 {
        f64::from(closest_point_to_segment(point, from, to).gd_distance_to(point))
    }

    fn insert_segment(
        &mut self,
        id: SegmentId,
        start_id: NodeId,
        end_id: NodeId,
        road_type: &'static RoadType,
        elevation_layer: i32,
        centerline: Curve2D,
    ) {
        let segment =
            RoadSegment::new(id, start_id, end_id, road_type, elevation_layer, centerline);
        self.segments.insert(id, segment);
        self.nodes[&start_id].segment_ids.push(id);
        self.nodes[&end_id].segment_ids.push(id);
        self.create_lanes(id);
    }

    /// Would building `road_type` along `segment`, dragged in `direction`, change it?
    fn needs_retype(
        &self,
        segment: &RoadSegment,
        road_type: &'static RoadType,
        direction: Vec2,
    ) -> bool {
        if !std::ptr::eq(segment.road_type, road_type) {
            return true;
        }
        let segment_direction =
            self.nodes[&segment.end_node_id].position - self.nodes[&segment.start_node_id].position;
        road_type.one_way && segment_direction.dot(direction) < 0.0
    }

    /// Replaces a segment's lanes for a new road type (and direction). The caller
    /// refreshes the junctions at both ends and emits `RoadsChanged`.
    fn retype(&mut self, segment_id: SegmentId, road_type: &'static RoadType, reverse: bool) {
        let lane_ids = std::mem::take(&mut self.segments[&segment_id].lane_ids);
        for lane_id in lane_ids {
            // Vehicles still on a removed lane re-snap or re-route from the retired lane.
            self.retire_lane(lane_id);
        }
        let segment = &mut self.segments[&segment_id];
        if reverse {
            std::mem::swap(&mut segment.start_node_id, &mut segment.end_node_id);
            let reversed = Self::reversed_curve(segment.centerline());
            segment.set_centerline(reversed);
        }
        segment.road_type = road_type;
        self.create_lanes(segment_id);
    }

    fn reversed_curve(curve: &Curve2D) -> Curve2D {
        Curve2D::new(
            curve
                .points()
                .iter()
                .rev()
                .map(|p| CurvePoint {
                    position: p.position,
                    point_in: p.point_out,
                    point_out: p.point_in,
                })
                .collect(),
        )
    }

    /// Re-derives everything that depends on the given nodes' sets of segments: the
    /// trimmed geometry of every lane touching them, and their connectors. Returns the
    /// ids of the segments whose lanes were updated. (Lanes only change at the trimmed
    /// end, so connectors at the far nodes stay valid.)
    fn refresh_junctions(&mut self, node_ids: &[NodeId]) -> Vec<SegmentId> {
        let node_ids = unique(node_ids);
        let mut segment_ids: Vec<SegmentId> = Vec::new();
        for node_id in &node_ids {
            let Some(node) = self.nodes.get(node_id) else {
                continue;
            };
            for &segment_id in &node.segment_ids {
                if !segment_ids.contains(&segment_id) {
                    segment_ids.push(segment_id);
                }
            }
        }
        for &segment_id in &segment_ids {
            self.update_lane_geometry(segment_id);
        }
        for &node_id in &node_ids {
            if self.nodes.contains_key(&node_id) {
                self.rebuild_connectors(node_id);
            }
        }
        segment_ids
    }

    /// Creates the segment's road lanes once; their ids stay stable while it exists.
    /// Geometry is untrimmed until `update_lane_geometry` runs.
    fn create_lanes(&mut self, segment_id: SegmentId) {
        let segment = &self.segments[&segment_id];
        let (start, end, road_type) = (
            segment.start_node_id,
            segment.end_node_id,
            segment.road_type,
        );
        let specs = lane_builder::build_lane_specs(segment.centerline(), road_type, 0.0, 0.0);
        for spec in specs {
            let id = LaneId(self.next_lane_id);
            let speed_limit = Self::lane_speed(road_type, &spec.curve);
            let mut lane = Lane::new(
                id,
                Some(segment_id),
                if spec.forward { start } else { end },
                if spec.forward { end } else { start },
                spec.index,
                spec.curve,
                road_type.speed_limit,
            );
            lane.bus_only = road_type.is_bus_lane(spec.index);
            lane.speed_limit = speed_limit;
            self.next_lane_id += 1;
            self.lanes.insert(id, lane);
            self.segments[&segment_id].lane_ids.push(id);
        }
    }

    /// `build_lane_specs` returns lanes in a fixed order, matching `segment.lane_ids`.
    fn update_lane_geometry(&mut self, segment_id: SegmentId) {
        let segment = &self.segments[&segment_id];
        let specs = lane_builder::build_lane_specs(
            segment.centerline(),
            segment.road_type,
            self.junction_radius(segment.start_node_id),
            self.junction_radius(segment.end_node_id),
        );
        let road_type = segment.road_type;
        let lane_ids = segment.lane_ids.clone();
        for (spec, lane_id) in specs.into_iter().zip(lane_ids) {
            let speed_limit = Self::lane_speed(road_type, &spec.curve);
            let lane = &mut self.lanes[&lane_id];
            lane.set_curve(spec.curve);
            lane.speed_limit = speed_limit;
        }
    }

    /// A lane's speed limit: its road's, or lower round a tight curve, where drivers
    /// keep their sideways acceleration under CURVE_LATERAL_ACCELERATION (v = sqrt(a r)).
    fn lane_speed(road_type: &RoadType, curve: &Curve2D) -> f64 {
        let radius = Self::min_radius(&lane_builder::get_polyline(curve));
        road_type
            .speed_limit
            .min((Self::CURVE_LATERAL_ACCELERATION * radius).sqrt())
    }

    fn erase_connectors(&mut self, node_id: NodeId) {
        let lane_ids = std::mem::take(&mut self.nodes[&node_id].connector_lane_ids);
        for lane_id in lane_ids {
            // Vehicles still on a removed connector only need its outgoing_lane_ids.
            self.retire_lane(lane_id);
        }
    }

    fn rebuild_connectors(&mut self, node_id: NodeId) {
        self.erase_connectors(node_id);
        let node = &mut self.nodes[&node_id];
        if node.degree() < 3 {
            node.control = JunctionControl::None;
        }
        let mut incoming: Vec<LaneId> = Vec::new();
        let mut outgoing: Vec<LaneId> = Vec::new();
        for segment_id in &self.nodes[&node_id].segment_ids {
            for lane_id in &self.segments[segment_id].lane_ids {
                if self.lanes[lane_id].to_node_id == node_id {
                    incoming.push(*lane_id);
                } else {
                    outgoing.push(*lane_id);
                }
            }
        }

        for &in_id in &incoming {
            self.lanes[&in_id].outgoing_lane_ids.clear();
            for &out_id in &outgoing {
                let turn =
                    self.classify_movement(node_id, &self.lanes[&in_id], &self.lanes[&out_id]);
                if turn == Turn::None {
                    continue;
                }
                let (in_lane, out_lane) = (&self.lanes[&in_id], &self.lanes[&out_id]);
                let id = LaneId(self.next_lane_id);
                let mut connector = Lane::new(
                    id,
                    None,
                    node_id,
                    node_id,
                    out_lane.index_from_center,
                    lane_builder::build_connector_curve(in_lane.curve(), out_lane.curve()),
                    in_lane.speed_limit.min(out_lane.speed_limit) * Self::turn_speed_factor(turn),
                );
                self.next_lane_id += 1;
                connector.turn = turn;
                connector.outgoing_lane_ids.push(out_id);
                connector.feeder_lane_id = Some(in_id);
                // Out of a bus lane, anyone may turn right (cars use it to turn right).
                connector.bus_only = out_lane.bus_only || (in_lane.bus_only && turn != Turn::Right);
                self.lanes[&in_id].outgoing_lane_ids.push(id);
                self.lanes.insert(id, connector);
                self.nodes[&node_id].connector_lane_ids.push(id);
            }
        }
        self.compute_conflicts(node_id);
        self.assign_signal_phases(node_id, &incoming);
    }

    /// Traffic lights alternate between the two road axes: lanes arriving east-west
    /// belong to axis 0, lanes arriving north-south to axis 1 (by their direction at the
    /// stop line). A connector's phase is its feeder lane's axis; with protected left
    /// turns, left turns and U-turns get a phase of their own after their axis's through
    /// phase: 0 axis-0 through, 1 axis-0 left, 2 axis-1 through, 3 axis-1 left. Phases
    /// nobody uses are simply never asked for (TrafficLights skips phases without demand).
    fn assign_signal_phases(&mut self, node_id: NodeId, incoming: &[LaneId]) {
        let mut axes: Vec<i32> = Vec::new();
        for lane_id in incoming {
            let lane = &mut self.lanes[lane_id];
            let direction = lane_builder::end_direction(lane.curve());
            lane.signal_phase = if direction.x.abs() >= direction.y.abs() {
                0
            } else {
                1
            };
            if !axes.contains(&lane.signal_phase) {
                axes.push(lane.signal_phase);
            }
        }
        if axes.len() == 1 {
            for lane_id in incoming {
                self.lanes[lane_id].signal_phase = 0;
            }
        }
        let node = &mut self.nodes[&node_id];
        let left_phases =
            node.control == JunctionControl::TrafficLightProtectedLeft && axes.len() > 1;
        node.signal_phase_count = if left_phases { 4 } else { axes.len() as i32 };
        let connector_ids = node.connector_lane_ids.clone();
        for lane_id in connector_ids {
            let feeder = self.lanes[&lane_id]
                .feeder_lane_id
                .expect("connectors have a feeder");
            let axis = self.lanes[&feeder].signal_phase;
            let connector = &mut self.lanes[&lane_id];
            connector.signal_phase = if left_phases {
                let turning_across = connector.turn == Turn::Left || connector.turn == Turn::UTurn;
                axis * 2 + i32::from(turning_across)
            } else {
                axis
            };
        }
    }

    /// Finds which of the node's connectors conflict and where: two connectors conflict
    /// where their paths come closer than CONFLICT_DISTANCE (crossing or merging). Each
    /// records the stretch of its own path that is shared (its conflict zone), so
    /// vehicles only have to keep out of each other's way there, not for the whole
    /// junction. Connectors leaving the same lane don't conflict: vehicles on them already
    /// follow each other.
    fn compute_conflicts(&mut self, node_id: NodeId) {
        let connectors = self.nodes[&node_id].connector_lane_ids.clone();
        let polylines: Vec<Vec<Vec2>> = connectors
            .iter()
            .map(|id| lane_builder::get_polyline(self.lanes[id].curve()))
            .collect();
        for i in 0..connectors.len() {
            for j in i + 1..connectors.len() {
                let (a, b) = (&self.lanes[&connectors[i]], &self.lanes[&connectors[j]]);
                if a.feeder_lane_id == b.feeder_lane_id {
                    continue;
                }
                let merge = a.outgoing_lane_ids[0] == b.outgoing_lane_ids[0];
                if !merge
                    && lane_builder::polyline_distance(&polylines[i], &polylines[j])
                        >= Self::CONFLICT_DISTANCE
                {
                    continue;
                }
                let mut zone_a = lane_builder::conflict_zone(
                    &polylines[i],
                    &polylines[j],
                    Self::CONFLICT_DISTANCE,
                    2.0,
                );
                let mut zone_b = lane_builder::conflict_zone(
                    &polylines[j],
                    &polylines[i],
                    Self::CONFLICT_DISTANCE,
                    2.0,
                );
                if merge {
                    // Merging into one lane: at the very least they meet where they end.
                    zone_a = Some(Self::with_end_zone(zone_a, a.length));
                    zone_b = Some(Self::with_end_zone(zone_b, b.length));
                }
                let (Some(zone_a), Some(zone_b)) = (zone_a, zone_b) else {
                    continue;
                };
                let (a_id, b_id) = (connectors[i], connectors[j]);
                let a = &mut self.lanes[&a_id];
                a.conflicting_lane_ids.push(b_id);
                a.conflict_zones.insert(b_id, zone_a);
                let b = &mut self.lanes[&b_id];
                b.conflicting_lane_ids.push(a_id);
                b.conflict_zones.insert(a_id, zone_b);
            }
        }
    }

    fn with_end_zone(zone: Option<Vec2>, length: f64) -> Vec2 {
        let end_zone = Vec2::new(
            (length - vehicle::MAX_LENGTH).max(0.0) as f32,
            length as f32,
        );
        match zone {
            None => end_zone,
            Some(zone) => Vec2::new(
                f64::from(zone.x).min(f64::from(end_zone.x)) as f32,
                length as f32,
            ),
        }
    }

    /// Which movement (if any) is allowed from `in_lane` into `out_lane` through `node`.
    /// Multi-lane roads: straight keeps its lane, right turns use the curb lanes, left
    /// turns the center lanes. U-turns only at dead ends. Bus lanes (`RoadType::bus_lanes`)
    /// are left out of the lanes everyone uses: the curb lane for cars is then the one
    /// next to the bus lane (buses may also turn right from it into a bus lane); a bus
    /// lane goes on straight into the next road's curb lane (its bus lane, if it has one)
    /// or turns right into it.
    fn classify_movement(&self, node_id: NodeId, in_lane: &Lane, out_lane: &Lane) -> Turn {
        let in_segment = in_lane.segment_id.expect("road lane");
        let out_segment = out_lane.segment_id.expect("road lane");
        let out_type = self.segments[&out_segment].road_type;
        let in_general = self.segments[&in_segment].road_type.general_lane_count();
        let out_count = out_type.lanes_per_direction;
        let out_general = out_type.general_lane_count();
        if in_segment == out_segment {
            let is_dead_end = self.nodes[&node_id].degree() == 1;
            if is_dead_end && in_lane.index_from_center == 0 && out_lane.index_from_center == 0 {
                return Turn::UTurn;
            }
            return Turn::None;
        }

        let d0 = lane_builder::end_direction(in_lane.curve());
        let d1 = lane_builder::start_direction(out_lane.curve());
        if f64::from(d0.gd_angle_to(d1).abs()) < Self::STRAIGHT_MAX_ANGLE {
            let target = if in_lane.bus_only {
                out_count - 1
            } else {
                in_lane.index_from_center.min(out_general - 1)
            };
            return if out_lane.index_from_center == target {
                Turn::Straight
            } else {
                Turn::None
            };
        }
        // y-down screen space: a positive cross product means turning clockwise, i.e. right.
        if d0.gd_cross(d1) > 0.0 {
            if in_lane.bus_only {
                return if out_lane.index_from_center == out_count - 1 {
                    Turn::Right
                } else {
                    Turn::None
                };
            }
            let from_curb = in_lane.index_from_center == in_general - 1;
            // Buses may also turn right straight into a bus lane (a bus-only turn: see
            // bus_only).
            let to_curb = out_lane.index_from_center == out_general - 1 || out_lane.bus_only;
            return if from_curb && to_curb {
                Turn::Right
            } else {
                Turn::None
            };
        }
        if in_lane.bus_only {
            return Turn::None;
        }
        let from_center = in_lane.index_from_center == 0;
        let to_center = out_lane.index_from_center == 0;
        if from_center && to_center {
            Turn::Left
        } else {
            Turn::None
        }
    }

    /// True if `point` lies on the polyline but not at either of its ends.
    fn is_on_interior(point: Vec2, polyline: &[Vec2]) -> bool {
        if f64::from(point.gd_distance_to(polyline[0])) <= Self::EPSILON
            || f64::from(point.gd_distance_to(polyline[polyline.len() - 1])) <= Self::EPSILON
        {
            return false;
        }
        lane_builder::distance_to_polyline(point, polyline) <= Self::EPSILON
    }
}

/// Junction control is saved by name, lower case.
fn control_name(control: JunctionControl) -> &'static str {
    match control {
        JunctionControl::None => "none",
        JunctionControl::TrafficLight => "traffic_light",
        JunctionControl::TrafficLightProtectedLeft => "traffic_light_protected_left",
    }
}

/// Junction control is saved by name. Older saves stored the enum number, back when it
/// also had stop signs (0 none, 1 stop sign, 2 lights): stop signs load as no control.
fn control_from_save(value: &Value) -> JunctionControl {
    match value {
        Value::String(name) => match name.to_uppercase().as_str() {
            "TRAFFIC_LIGHT" => JunctionControl::TrafficLight,
            "TRAFFIC_LIGHT_PROTECTED_LEFT" => JunctionControl::TrafficLightProtectedLeft,
            _ => JunctionControl::None,
        },
        _ => {
            if value.as_f64().map(|v| v as i64) == Some(2) {
                JunctionControl::TrafficLight
            } else {
                JunctionControl::None
            }
        }
    }
}

fn unique<T: PartialEq + Copy>(values: &[T]) -> Vec<T> {
    let mut result = Vec::new();
    for &value in values {
        if !result.contains(&value) {
            result.push(value);
        }
    }
    result
}
