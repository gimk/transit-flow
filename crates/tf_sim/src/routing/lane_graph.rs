//! `LaneGraph` (`sim/routing/lane_graph.gd`): an A* graph with one point per lane (road
//! lanes and connectors alike), placed at the lane's end, and one directed connection per
//! allowed lane → lane transition: on into the lanes a lane leads to, or sideways into
//! the lane next door (a lane change). The cost of a step is the travel time of the lane
//! being entered (a fixed penalty for a lane change), so A* minimises time rather than
//! distance. The graph for cars (rebuilt with `for_buses` false) never enters a bus-only
//! lane or turn: cars only change into a bus lane, or out of it, to reach the curb, or to
//! turn right out of it.
//!
//! The GDScript class extends Godot's `AStar2D`; this is a port of the parts of Godot
//! 4.7's A* it uses (`AStar2D::_solve`), so routes come out the same even where several
//! are equally quick: neighbours are tried in the order they were connected, the open
//! list is Godot's binary heap (`SortArray::push_heap` / `pop_heap`), points are ordered
//! by `f` then by the larger `g`, and scores are `real_t` (`f32`). Costs come from the
//! GDScript callbacks, in `f64`, converted to `f32` as Godot does.

use std::collections::HashMap;

use glam::Vec2;
use indexmap::IndexMap;

use crate::geom::GdVec2;
use crate::ids::{LaneId, NodeId, SegmentId};
use crate::roads::Lane;

/// Extra cost per lane as a fraction of its free-flow time (Phase 2c congestion
/// feedback). Missing entries mean free flow. (`LaneGraph.congestion` in GDScript; the
/// router keeps one map shared by its graphs.)
pub type Congestion = IndexMap<LaneId, f64>;

#[derive(Clone, Debug, Default)]
pub struct LaneGraph {
    /// Point index → lane id, in the order points were added.
    ids: Vec<LaneId>,
    index_of: HashMap<LaneId, usize>,
    positions: Vec<Vec2>,
    /// Outgoing connections per point, in the order they were connected.
    neighbors: Vec<Vec<usize>>,
    max_speed: f64,
    // A* state, reused between searches (Godot's per-point pass markers).
    pass: u64,
    open_pass: Vec<u64>,
    closed_pass: Vec<u64>,
    g_score: Vec<f32>,
    f_score: Vec<f32>,
    prev: Vec<usize>,
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
        self.positions.push(position);
        self.neighbors.push(Vec::new());
        self.open_pass.push(0);
        self.closed_pass.push(0);
        self.g_score.push(0.0);
        self.f_score.push(0.0);
        self.prev.push(usize::MAX);
    }

    /// `connect_points(from, to, false)`: a one-way connection.
    fn connect_points(&mut self, from: LaneId, to: LaneId) {
        let (a, b) = (self.index_of[&from], self.index_of[&to]);
        if !self.neighbors[a].contains(&b) {
            self.neighbors[a].push(b);
        }
    }

    pub fn has_point(&self, id: LaneId) -> bool {
        self.index_of.contains_key(&id)
    }

    /// `are_points_connected(from, to, false)`.
    pub fn are_points_connected(&self, from: LaneId, to: LaneId) -> bool {
        match (self.index_of.get(&from), self.index_of.get(&to)) {
            (Some(&a), Some(&b)) => self.neighbors[a].contains(&b),
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

    /// `_estimate_cost`: straight-line distance at the fastest speed anywhere: never
    /// overestimates, so A* still finds the quickest route.
    fn estimate_cost(&self, from: usize, end: usize) -> f32 {
        (f64::from(self.positions[from].gd_distance_to(self.positions[end])) / self.max_speed)
            as f32
    }

    /// `get_id_path(from, to)`: the lanes of the cheapest path, both ends included;
    /// empty if there is none.
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
        if begin == end {
            return vec![from];
        }
        if !self.solve(lanes, congestion, begin, end) {
            return Vec::new();
        }
        let mut path = vec![self.ids[end]];
        let mut point = end;
        while point != begin {
            point = self.prev[point];
            path.push(self.ids[point]);
        }
        path.reverse();
        path
    }

    /// `SortPoints`: true when point `a` is worse than point `b` (higher f; on equal f,
    /// lower g: points further from the start go first).
    fn worse(&self, a: usize, b: usize) -> bool {
        let (fa, fb) = (self.f_score[a], self.f_score[b]);
        if fa > fb {
            true
        } else if fa < fb {
            false
        } else {
            self.g_score[a] < self.g_score[b]
        }
    }

    /// `SortArray::push_heap`.
    fn push_heap(&self, heap: &mut [usize], mut hole: usize, top: usize, value: usize) {
        while hole > top {
            let parent = (hole - 1) / 2;
            if !self.worse(heap[parent], value) {
                break;
            }
            heap[hole] = heap[parent];
            hole = parent;
        }
        heap[hole] = value;
    }

    /// `SortArray::adjust_heap`.
    fn adjust_heap(&self, heap: &mut [usize], mut hole: usize, len: usize, value: usize) {
        let top = hole;
        let mut second_child = 2 * hole + 2;
        while second_child < len {
            if self.worse(heap[second_child], heap[second_child - 1]) {
                second_child -= 1;
            }
            heap[hole] = heap[second_child];
            hole = second_child;
            second_child = 2 * (second_child + 1);
        }
        if second_child == len {
            heap[hole] = heap[second_child - 1];
            hole = second_child - 1;
        }
        self.push_heap(heap, hole, top, value);
    }

    /// `SortArray::pop_heap(0, len)`: moves the best point to the end.
    fn pop_heap(&self, heap: &mut [usize]) {
        let last = heap.len() - 1;
        let value = heap[last];
        heap[last] = heap[0];
        self.adjust_heap(heap, 0, last, value);
    }

    /// `AStar2D::_solve`.
    fn solve(
        &mut self,
        lanes: &IndexMap<LaneId, Lane>,
        congestion: &Congestion,
        begin: usize,
        end: usize,
    ) -> bool {
        self.pass += 1;
        let pass = self.pass;
        let mut open: Vec<usize> = Vec::new();
        self.g_score[begin] = 0.0;
        self.f_score[begin] = self.estimate_cost(begin, end);
        open.push(begin);
        while let Some(&point) = open.first() {
            if point == end {
                return true;
            }
            self.pop_heap(&mut open);
            open.pop();
            self.closed_pass[point] = pass;
            for k in 0..self.neighbors[point].len() {
                let neighbor = self.neighbors[point][k];
                if self.closed_pass[neighbor] == pass {
                    continue;
                }
                let cost =
                    Self::step_cost(lanes, congestion, self.ids[point], self.ids[neighbor]) as f32;
                let tentative = self.g_score[point] + cost * 1.0;
                let new_point = self.open_pass[neighbor] != pass;
                if new_point {
                    self.open_pass[neighbor] = pass;
                    open.push(neighbor);
                } else if tentative >= self.g_score[neighbor] {
                    continue;
                }
                self.prev[neighbor] = point;
                self.g_score[neighbor] = tentative;
                self.f_score[neighbor] = tentative + self.estimate_cost(neighbor, end);
                let hole = if new_point {
                    open.len() - 1
                } else {
                    open.iter()
                        .position(|&p| p == neighbor)
                        .expect("in the open list")
                };
                self.push_heap(&mut open, hole, 0, neighbor);
            }
        }
        false
    }
}
