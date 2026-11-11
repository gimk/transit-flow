//! `ExternalConnections` (`sim/city/external_connections.gd`): fixed points on the map
//! edge where the city meets the outside world (highways for now). Trucks bring raw
//! materials and imports in, and take exports out, through them. A connection works while
//! a road ends exactly on its point.

use glam::Vec2;
use serde_json::{Value, json};

use crate::events::{Outbox, SimEvent};
use crate::roads::{Lane, RoadNetwork};
use crate::routing::LanePoint;
use crate::traffic::vehicle;

#[derive(Clone, Debug, Default)]
pub struct ExternalConnections {
    pub points: Vec<Vec2>,
}

impl ExternalConnections {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, point: Vec2, out: &mut Outbox) {
        if !self.points.contains(&point) {
            self.points.push(point);
            out.push(SimEvent::ExternalsChanged);
        }
    }

    /// Points that currently have a road ending on them.
    pub fn active_points(&self, roads: &RoadNetwork) -> Vec<Vec2> {
        self.points
            .iter()
            .copied()
            .filter(|&point| roads.find_node_at(point, 0).is_some())
            .collect()
    }

    /// Where vehicles arriving from outside appear: just inside the lane leaving the
    /// point; `None` if the connection has no road.
    pub fn inbound(&self, roads: &RoadNetwork, point: Vec2) -> Option<LanePoint> {
        let lane = Self::edge_lane(roads, point, true)?;
        Some(LanePoint::new(
            lane.id,
            vehicle::MAX_LENGTH.min(lane.length * 0.5),
        ))
    }

    /// Where vehicles leaving the city disappear: the end of the lane arriving at the
    /// point.
    pub fn outbound(&self, roads: &RoadNetwork, point: Vec2) -> Option<LanePoint> {
        let lane = Self::edge_lane(roads, point, false)?;
        Some(LanePoint::new(lane.id, (lane.length - 1.0).max(0.0)))
    }

    pub fn to_dict(&self) -> Value {
        let points: Vec<Value> = self.points.iter().map(|p| json!([p.x, p.y])).collect();
        json!({ "points": points })
    }

    pub fn restore(&mut self, data: &Value, out: &mut Outbox) {
        let number = |value: &Value| value.as_f64().unwrap_or(0.0) as f32;
        self.points = data["points"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|entry| Vec2::new(number(&entry[0]), number(&entry[1])))
            .collect();
        out.push(SimEvent::ExternalsChanged);
    }

    fn edge_lane(roads: &RoadNetwork, point: Vec2, leaving: bool) -> Option<&Lane> {
        let node_id = roads.find_node_at(point, 0)?;
        for segment_id in &roads.nodes[&node_id].segment_ids {
            for lane_id in &roads.segments[segment_id].lane_ids {
                let lane = &roads.lanes[lane_id];
                if (if leaving {
                    lane.from_node_id
                } else {
                    lane.to_node_id
                }) == node_id
                {
                    return Some(lane);
                }
            }
        }
        None
    }
}
