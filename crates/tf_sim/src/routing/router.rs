//! `Router` (`sim/routing/router.gd`): finds quickest lane-level routes between two
//! positions on road lanes. The lane graph is rebuilt lazily on the first query after a
//! network change, and lane paths are cached per (from lane, to lane): most trips share
//! a few such pairs. Buses route on a graph of their own once there are bus lanes (`bus`
//! arguments): cars may not drive along them (see `LaneGraph`).
//!
//! Unlike the GDScript class, the router doesn't hold the road network: every query
//! takes it (`roads`).

use std::collections::HashMap;

use super::lane_graph::{Congestion, LaneGraph};
use super::route::Route;
use crate::events::Outbox;
use crate::ids::LaneId;
use crate::roads::{ChangeSet, RoadNetwork};

/// A position on a road lane a trip may start or end at: `{ "lane_id", "distance" }` in
/// GDScript, with an optional "penalty" (expected delay, like turning across the road).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LanePoint {
    pub lane_id: LaneId,
    pub distance: f64,
    pub penalty: f64,
}

impl LanePoint {
    pub fn new(lane_id: LaneId, distance: f64) -> Self {
        Self {
            lane_id,
            distance,
            penalty: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Router {
    /// The graph for everyone but buses.
    pub graph: LaneGraph,
    /// `graph.congestion` in GDScript: shared by both graphs (the bus graph copies the
    /// main one's there). `CongestionMonitor` replaces it every 5 s.
    pub congestion: Congestion,
    graph_dirty: bool,
    /// The graph for buses, used when there are bus lanes (otherwise `graph` serves).
    bus_graph: LaneGraph,
    has_bus_lanes: bool,
    /// (from lane, to lane, loop back, for buses) → lane path.
    path_cache: HashMap<(LaneId, LaneId, bool, bool), Vec<LaneId>>,
}

impl Default for Router {
    fn default() -> Self {
        Self {
            graph: LaneGraph::new(),
            congestion: Congestion::new(),
            graph_dirty: true,
            bus_graph: LaneGraph::new(),
            has_bus_lanes: false,
            path_cache: HashMap::new(),
        }
    }
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    /// Quickest route from `from_distance` along lane `from_lane_id` to `to_distance`
    /// along lane `to_lane_id`, or `None` if the destination can't be reached. A route
    /// may change lanes along a road (consecutive lanes beside each other). `bus`: for a
    /// bus, which may use bus lanes.
    pub fn find_route(
        &mut self,
        roads: &RoadNetwork,
        from_lane_id: LaneId,
        from_distance: f64,
        to_lane_id: LaneId,
        to_distance: f64,
        bus: bool,
    ) -> Option<Route> {
        let (Some(from_lane), Some(to_lane)) =
            (roads.lanes.get(&from_lane_id), roads.lanes.get(&to_lane_id))
        else {
            return None;
        };
        self.ensure_graph(roads);
        let same_road = from_lane_id == to_lane_id || from_lane.is_beside(to_lane);
        let path = if from_lane_id == to_lane_id && to_distance >= from_distance {
            vec![from_lane_id]
        } else {
            // Behind us on the same road (this lane or one beside it): go round the block.
            self.lane_path(
                roads,
                from_lane_id,
                to_lane_id,
                same_road && to_distance < from_distance,
                bus,
            )
        };
        if path.is_empty() {
            return None;
        }
        let travel_time = self.travel_time(roads, &path, from_distance, to_distance);
        Some(Route::new(path, from_distance, to_distance, travel_time))
    }

    /// Quickest route from any of `starts` to any of `ends` (e.g. both lanes in front of
    /// a building), counting each point's penalty, or `None` if none connects.
    pub fn find_best_route(
        &mut self,
        roads: &RoadNetwork,
        starts: &[LanePoint],
        ends: &[LanePoint],
        bus: bool,
    ) -> Option<Route> {
        let mut best = None;
        let mut best_cost = f64::INFINITY;
        for start in starts {
            for end in ends {
                let Some(route) = self.find_route(
                    roads,
                    start.lane_id,
                    start.distance,
                    end.lane_id,
                    end.distance,
                    bus,
                ) else {
                    continue;
                };
                let cost = route.travel_time + start.penalty + end.penalty;
                if cost < best_cost {
                    best = Some(route);
                    best_cost = cost;
                }
            }
        }
        best
    }

    /// Quickest route to any of `ends` that leaves `from_lane_id` through its end, i.e.
    /// without changing lanes first (for a driver who missed the lane they needed). The
    /// route starts on the lane after it; `None` if there's none.
    pub fn find_route_onward(
        &mut self,
        roads: &RoadNetwork,
        from_lane_id: LaneId,
        ends: &[LanePoint],
        bus: bool,
    ) -> Option<Route> {
        let lane = roads.lanes.get(&from_lane_id)?;
        let starts: Vec<LanePoint> = lane
            .outgoing_lane_ids
            .iter()
            .map(|&id| LanePoint::new(id, 0.0))
            .collect();
        self.find_best_route(roads, &starts, ends, bus)
    }

    /// Travel time of a route on empty roads (`Route::free_flow_time`).
    pub fn free_flow_time(&self, roads: &RoadNetwork, route: &Route) -> f64 {
        route.free_flow_time(roads)
    }

    /// Drops cached paths, e.g. after congestion costs change.
    pub fn invalidate_cache(&mut self) {
        self.path_cache.clear();
    }

    pub fn on_roads_changed(&mut self, _change: &ChangeSet, _out: &mut Outbox) {
        self.graph_dirty = true;
        self.path_cache.clear();
    }

    fn ensure_graph(&mut self, roads: &RoadNetwork) {
        if self.graph_dirty {
            self.has_bus_lanes = roads.lanes.values().any(|lane| lane.bus_only);
            self.graph.rebuild(&roads.lanes, !self.has_bus_lanes);
            if self.has_bus_lanes {
                self.bus_graph.rebuild(&roads.lanes, true);
            }
            self.graph_dirty = false;
        }
    }

    fn graph_for(&mut self, bus: bool) -> &mut LaneGraph {
        if bus && self.has_bus_lanes {
            &mut self.bus_graph
        } else {
            &mut self.graph
        }
    }

    fn lane_path(
        &mut self,
        roads: &RoadNetwork,
        from_lane_id: LaneId,
        to_lane_id: LaneId,
        loop_back: bool,
        bus: bool,
    ) -> Vec<LaneId> {
        let for_buses = bus && self.has_bus_lanes;
        let key = (from_lane_id, to_lane_id, loop_back, for_buses);
        if let Some(path) = self.path_cache.get(&key) {
            return path.clone();
        }
        let congestion = std::mem::take(&mut self.congestion);
        let mut path = Vec::new();
        if loop_back {
            // The destination is behind us: leave the road and loop back around.
            let mut best_cost = f64::INFINITY;
            for &next_id in &roads.lanes[&from_lane_id].outgoing_lane_ids {
                let graph = self.graph_for(bus);
                if !graph.are_points_connected(from_lane_id, next_id) {
                    continue; // Not this way (a car on a bus lane).
                }
                let loop_path = graph.id_path(&roads.lanes, &congestion, next_id, to_lane_id);
                if loop_path.is_empty() {
                    continue;
                }
                let cost = Self::path_cost(roads, &congestion, &loop_path);
                if cost < best_cost {
                    best_cost = cost;
                    path = std::iter::once(from_lane_id).chain(loop_path).collect();
                }
            }
        } else {
            path = self
                .graph_for(bus)
                .id_path(&roads.lanes, &congestion, from_lane_id, to_lane_id);
        }
        self.congestion = congestion;
        self.path_cache.insert(key, path.clone());
        path
    }

    fn path_cost(roads: &RoadNetwork, congestion: &Congestion, path: &[LaneId]) -> f64 {
        let mut cost = LaneGraph::lane_cost(&roads.lanes, congestion, path[0]);
        for pair in path.windows(2) {
            cost += LaneGraph::step_cost(&roads.lanes, congestion, pair[0], pair[1]);
        }
        cost
    }

    /// Per run of lanes driven side by side (`Route::runs`): the part of its last lane
    /// actually driven (from the start on the first run, to the destination on the last),
    /// plus a penalty per lane change.
    fn travel_time(
        &self,
        roads: &RoadNetwork,
        path: &[LaneId],
        from_distance: f64,
        to_distance: f64,
    ) -> f64 {
        let runs = Route::runs(path, roads);
        let mut time = 0.0;
        for (r, &(first, last)) in runs.iter().enumerate() {
            let lane = &roads.lanes[&path[last]];
            let from_fraction = if r == 0 {
                (from_distance / lane.length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let to_fraction = if r == runs.len() - 1 {
                (to_distance / lane.length).clamp(0.0, 1.0)
            } else {
                1.0
            };
            time += LaneGraph::lane_cost(&roads.lanes, &self.congestion, lane.id)
                * (to_fraction - from_fraction).max(0.0);
            time += LaneGraph::LANE_CHANGE_COST * (last - first) as f64;
        }
        time
    }
}
