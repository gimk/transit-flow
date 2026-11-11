//! `Route` (`sim/routing/route.gd`): a path through the network: drive `lane_ids` in
//! order, entering the first lane at `start_distance` along it and stopping on the last
//! lane at `end_distance`. Consecutive lanes are either joined end to start (on through a
//! junction) or beside each other on the same road (a lane change, see `Lane::is_beside`):
//! a run of lanes side by side is driven once, moving across on the way.

use glam::Vec2;

use crate::ids::LaneId;
use crate::roads::{RoadNetwork, lane_builder};

#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub lane_ids: Vec<LaneId>,
    pub start_distance: f64,
    pub end_distance: f64,
    /// Free-flow (plus current congestion) travel time estimate, in sim seconds.
    pub travel_time: f64,
}

impl Route {
    pub fn new(
        lane_ids: Vec<LaneId>,
        start_distance: f64,
        end_distance: f64,
        travel_time: f64,
    ) -> Self {
        Self {
            lane_ids,
            start_distance,
            end_distance,
            travel_time,
        }
    }

    /// Groups a path into runs of lanes driven side by side: (first index, last index)
    /// each. A path without lane changes has one run per lane.
    pub fn runs(path: &[LaneId], roads: &RoadNetwork) -> Vec<(usize, usize)> {
        let mut runs = Vec::new();
        let mut start = 0;
        for i in 0..path.len() {
            let last = i == path.len() - 1;
            if last || !roads.lanes[&path[i]].is_beside(&roads.lanes[&path[i + 1]]) {
                runs.push((start, i));
                start = i + 1;
            }
        }
        runs
    }

    /// Travel time with no traffic at all (speed limits only), in sim seconds: the
    /// yardstick for patience and the Flow score.
    pub fn free_flow_time(&self, roads: &RoadNetwork) -> f64 {
        let runs = Self::runs(&self.lane_ids, roads);
        let mut time = 0.0;
        for (r, &(_, last)) in runs.iter().enumerate() {
            let lane = &roads.lanes[&self.lane_ids[last]];
            let from = if r == 0 { self.start_distance } else { 0.0 };
            let to = if r == runs.len() - 1 {
                self.end_distance
            } else {
                lane.length
            };
            time += (to - from).max(0.0) / lane.speed_limit;
        }
        time
    }

    /// The driven path as one polyline (for debug drawing and tests); a lane change is
    /// drawn as a step across at the start of the run.
    pub fn polyline(&self, roads: &RoadNetwork) -> Vec<Vec2> {
        let runs = Self::runs(&self.lane_ids, roads);
        let mut points = Vec::new();
        for (r, &(_, last)) in runs.iter().enumerate() {
            let lane = &roads.lanes[&self.lane_ids[last]];
            let lane_points = lane_builder::get_polyline(lane.curve());
            let from = if r == 0 { self.start_distance } else { 0.0 };
            let to = if r == runs.len() - 1 {
                self.end_distance
            } else {
                f64::INFINITY
            };
            points.extend(lane_builder::sub_polyline(&lane_points, from, to));
        }
        points
    }
}
