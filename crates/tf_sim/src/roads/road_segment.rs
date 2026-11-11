//! `RoadSegment` (`sim/roads/road_segment.gd`): a stretch of road between two nodes. Its
//! shape is a centerline `Curve2D`: a straight grid road is just a 2-point curve, so
//! curved roads need no new data.

use glam::Vec2;

use super::lane_builder;
use super::road_type::RoadType;
use crate::geom::{Curve2D, Rect2};
use crate::ids::{LaneId, NodeId, SegmentId};

#[derive(Clone, Debug, PartialEq)]
pub struct RoadSegment {
    pub id: SegmentId,
    pub start_node_id: NodeId,
    pub end_node_id: NodeId,
    pub road_type: &'static RoadType,
    pub elevation_layer: i32,
    /// Replaced, never edited in place (`set_centerline`, which refreshes the cached
    /// polyline and bounds).
    centerline: Curve2D,
    /// Lanes derived from the centerline; rebuilt whenever the segment changes.
    pub lane_ids: Vec<LaneId>,
    /// The centerline as a polyline, and its bounding box, worked out once.
    polyline: Vec<Vec2>,
    bounds: Rect2,
}

impl RoadSegment {
    pub fn new(
        id: SegmentId,
        start_node_id: NodeId,
        end_node_id: NodeId,
        road_type: &'static RoadType,
        elevation_layer: i32,
        centerline: Curve2D,
    ) -> Self {
        let mut segment = Self {
            id,
            start_node_id,
            end_node_id,
            road_type,
            elevation_layer,
            centerline: Curve2D::from_points(&[]),
            lane_ids: Vec::new(),
            polyline: Vec::new(),
            bounds: Rect2::default(),
        };
        segment.set_centerline(centerline);
        segment
    }

    pub fn centerline(&self) -> &Curve2D {
        &self.centerline
    }

    /// The GDScript `centerline` setter. Godot works the polyline out lazily; here it's
    /// done at once (the result is the same).
    pub fn set_centerline(&mut self, centerline: Curve2D) {
        self.centerline = centerline;
        self.polyline = lane_builder::get_polyline(&self.centerline);
        let mut bounds = Rect2::new(self.polyline[0], Vec2::ZERO);
        for &point in &self.polyline {
            bounds = bounds.expand(point);
        }
        self.bounds = bounds;
    }

    pub fn other_node_id(&self, node_id: NodeId) -> NodeId {
        if node_id == self.start_node_id {
            self.end_node_id
        } else {
            self.start_node_id
        }
    }

    pub fn length(&self) -> f64 {
        f64::from(self.centerline.baked_length())
    }

    /// The centerline as a polyline (`lane_builder::get_polyline`), cached: views and
    /// plots ask for it often.
    pub fn polyline(&self) -> &[Vec2] {
        &self.polyline
    }

    /// The box round the centerline (no width).
    pub fn bounds(&self) -> Rect2 {
        self.bounds
    }
}
