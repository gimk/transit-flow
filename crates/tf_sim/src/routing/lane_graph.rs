//! `LaneGraph` (`sim/routing/lane_graph.gd`): an A* graph with one point per lane (road
//! lanes and connectors alike), placed at the lane's end, and one directed connection per
//! allowed lane → lane transition: on into the lanes a lane leads to, or sideways into
//! the lane next door (a lane change). The cost of a step is the travel time of the lane
//! being entered (a fixed penalty for a lane change), so A* minimises time rather than
//! distance. The graph for cars (rebuilt with `for_buses` false) never enters a bus-only
//! lane or turn: cars only change into a bus lane, or out of it, to reach the curb, or to
//! turn right out of it.
//!
//! The GDScript class extends Godot's `AStar2D`; the search is `routing::astar::AStar`, a
//! port of Godot 4.7's, so routes come out the same even where several are equally quick.
//! Costs come from the GDScript callbacks, in `f64`, converted to `f32` as Godot does.

use std::collections::HashMap;

use glam::Vec2;
use indexmap::IndexMap;

use crate::geom::GdVec2;
use crate::ids::{LaneId, NodeId, SegmentId};
use crate::roads::Lane;
use crate::routing::astar::AStar;

/// Extra cost per lane as a fraction of its free-flow time (Phase 2c congestion
/// feedback). Missing entries mean free flow. (`LaneGraph.congestion` in GDScript; the
/// router keeps one map shared by its graphs.)
pub type Congestion = IndexMap<LaneId, f64>;

#[derive(Clone, Debug, Default)]
pub struct LaneGraph {
    /// Point index → lane id, in the order points were added.
    ids: Vec<LaneId>,
    index_of: HashMap<LaneId, usize>,
    astar: AStar,
    max_speed: f64,
}

impl LaneGraph {
    /// Route cost of changing to the lane next door, in sim seconds: enough that drivers
    /// don't weave for nothing, small enough to pick the lane their turn needs.
    pub const LANE_CHANGE_COST: f64 = 1.0;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn rebuild(&mut self, lanes: &IndexMap<LaneId, Lane>, for_buses: bool) {
        *self = Self::default();
        self.max_speed = 1.0;
        for lane in lanes.values() {
            let curve = lane.curve();
            self.add_point(lane.id, curve.point_position(curve.point_count() - 1));
            self.max_speed = self.max_speed.max(lane.speed_limit);
        }
        // Road lanes side by side going the same way: a lane change, to the lane next door.
        let mut groups: IndexMap<(SegmentId, NodeId), Vec<&Lane>> = IndexMap::new();
        for lane in lanes.values() {
            if let Some(segment_id) = lane.segment_id {
                groups
                    .entry((segment_id, lane.from_node_id))
                    .or_default()
                    .push(lane);
            }
            for next_id in &lane.outgoing_lane_ids {
                if for_buses || !lanes[next_id].bus_only {
                    self.connect_points(lane.id, *next_id);
                }
            }
        }
        for group in groups.values() {
            for a in group {
                for b in group {
                    if (a.index_from_center - b.index_from_center).abs() == 1 {
                        self.connect_points(a.id, b.id);
                    }
                }
            }
        }
    }

    fn add_point(&mut self, id: LaneId, position: Vec2) {
        self.index_of.insert(id, self.ids.len());
        self.ids.push(id);
        self.astar.add_point(position);
    }

    /// `connect_points(from, to, false)`: a one-way connection.
    fn connect_points(&mut self, from: LaneId, to: LaneId) {
        self.astar
            .connect_one_way(self.index_of[&from], self.index_of[&to]);
    }

    pub fn has_point(&self, id: LaneId) -> bool {
        self.index_of.contains_key(&id)
    }

    /// `are_points_connected(from, to, false)`.
    pub fn are_points_connected(&self, from: LaneId, to: LaneId) -> bool {
        match (self.index_of.get(&from), self.index_of.get(&to)) {
            (Some(&a), Some(&b)) => self.astar.are_connected(a, b),
            _ => false,
        }
    }

    /// Expected time to drive the whole lane, congestion included.
    pub fn lane_cost(
        lanes: &IndexMap<LaneId, Lane>,
        congestion: &Congestion,
        lane_id: LaneId,
    ) -> f64 {
        lanes[&lane_id].travel_time() * (1.0 + congestion.get(&lane_id).copied().unwrap_or(0.0))
    }

    /// Cost of moving from one lane onto the next one of a path: a lane change costs a
    /// fixed penalty (the vehicle drives on alongside, not the whole new lane again);
    /// anything else costs the whole lane entered.
    pub fn step_cost(
        lanes: &IndexMap<LaneId, Lane>,
        congestion: &Congestion,
        from_id: LaneId,
        to_id: LaneId,
    ) -> f64 {
        if lanes[&from_id].is_beside(&lanes[&to_id]) {
            return Self::LANE_CHANGE_COST;
        }
        Self::lane_cost(lanes, congestion, to_id)
    }

    /// `get_id_path(from, to)`: the lanes of the cheapest path, both ends included;
    /// empty if there is none. The estimate (`_estimate_cost`) is the straight-line
    /// distance at the fastest speed anywhere: it never overestimates, so A* still finds
    /// the quickest route.
    pub fn id_path(
        &mut self,
        lanes: &IndexMap<LaneId, Lane>,
        congestion: &Congestion,
        from: LaneId,
        to: LaneId,
    ) -> Vec<LaneId> {
        let (Some(&begin), Some(&end)) = (self.index_of.get(&from), self.index_of.get(&to)) else {
            return Vec::new();
        };
        let ids = &self.ids;
        let max_speed = self.max_speed;
        let path = self.astar.id_path(
            begin,
            end,
            |_, a, b| Self::step_cost(lanes, congestion, ids[a], ids[b]) as f32,
            |from, end| (f64::from(from.gd_distance_to(end)) / max_speed) as f32,
        );
        path.into_iter().map(|p| self.ids[p]).collect()
    }
}
