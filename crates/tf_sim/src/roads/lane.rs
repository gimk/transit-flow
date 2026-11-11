//! `Lane` (`sim/roads/lane.gd`): one directed path a vehicle can follow, from
//! `from_node_id` to `to_node_id`. Road lanes and the connector lanes that turn through
//! a junction are both lanes, so vehicles only ever need to know how to follow a curve.

use glam::Vec2;
use indexmap::IndexMap;

use super::lane_builder;
use crate::geom::{Curve2D, GdVec2};
use crate::ids::{LaneId, NodeId, SegmentId};

/// The movement a connector performs; `None` for road lanes. Godot's values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Turn {
    #[default]
    None = 0,
    Straight = 1,
    Right = 2,
    Left = 3,
    UTurn = 4,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub id: LaneId,
    /// Owning segment, or `None` for a connector inside a junction (Godot: -1).
    pub segment_id: Option<SegmentId>,
    /// For connectors both are the junction node.
    pub from_node_id: NodeId,
    pub to_node_id: NodeId,
    /// 0 = the lane next to the road's center line, increasing toward the curb.
    pub index_from_center: i32,
    curve: Curve2D,
    pub length: f64,
    pub speed_limit: f64,
    /// The movement a connector performs; `Turn::None` for road lanes.
    pub turn: Turn,
    /// Lanes a vehicle may continue onto from this lane's end. (Godot also keeps them
    /// as objects, `outgoing_lanes`, for speed; ids are enough here.)
    pub outgoing_lane_ids: Vec<LaneId>,
    /// Only buses may drive along it: a bus lane, or a turn into one or out of one
    /// (except right turns out of one, which cars take too).
    pub bus_only: bool,
    /// Connectors only: the road lane feeding this connector.
    pub feeder_lane_id: Option<LaneId>,
    /// Connectors only: other connectors at the same junction that cross or merge with
    /// this one, so must not be used at the same time.
    pub conflicting_lane_ids: Vec<LaneId>,
    /// Connectors only: for each conflicting connector id, the stretch of this
    /// connector (x = from, y = to, distances along it) that comes too close to that one.
    pub conflict_zones: IndexMap<LaneId, Vec2>,
    /// Traffic lights: on a road lane, the axis (0 or 1) its junction groups it in; on a
    /// connector, the light phase that lets vehicles onto it (see
    /// `RoadNetwork::assign_signal_phases`).
    pub signal_phase: i32,
    /// Straight lanes (two points, no handles) are sampled with plain math, which is
    /// much cheaper than curve sampling for the many vehicles on them.
    pub is_straight: bool,
    start: Vec2,
    direction: Vec2,
    angle: f32,
}

impl Lane {
    pub fn new(
        id: LaneId,
        segment_id: Option<SegmentId>,
        from_node_id: NodeId,
        to_node_id: NodeId,
        index_from_center: i32,
        curve: Curve2D,
        speed_limit: f64,
    ) -> Self {
        let mut lane = Self {
            id,
            segment_id,
            from_node_id,
            to_node_id,
            index_from_center,
            curve: Curve2D::from_points(&[]),
            length: 0.0,
            speed_limit,
            turn: Turn::None,
            outgoing_lane_ids: Vec::new(),
            bus_only: false,
            feeder_lane_id: None,
            conflicting_lane_ids: Vec::new(),
            conflict_zones: IndexMap::new(),
            signal_phase: 0,
            is_straight: false,
            start: Vec2::ZERO,
            direction: Vec2::ZERO,
            angle: 0.0,
        };
        lane.set_curve(curve);
        lane
    }

    pub fn curve(&self) -> &Curve2D {
        &self.curve
    }

    pub fn set_curve(&mut self, curve: Curve2D) {
        self.curve = curve;
        self.length = f64::from(self.curve.baked_length());
        self.is_straight = self.curve.point_count() == 2 && !lane_builder::has_handles(&self.curve);
        if self.is_straight {
            self.start = self.curve.point_position(0);
            let delta = self.curve.point_position(1) - self.start;
            let length = delta.length();
            self.length = f64::from(length);
            self.direction = if length > 0.0 {
                delta / length
            } else {
                Vec2::X
            };
            self.angle = self.direction.gd_angle();
        }
    }

    /// Position and heading (radians) at `distance` along the lane.
    pub fn sample_pose(&self, distance: f64) -> (Vec2, f32) {
        let distance = distance.clamp(0.0, self.length);
        if self.is_straight {
            return (self.start + self.direction * distance as f32, self.angle);
        }
        self.curve.sample_baked_with_rotation(distance as f32)
    }

    pub fn is_connector(&self) -> bool {
        self.segment_id.is_none()
    }

    /// True for another road lane of the same road going the same way (a lane a vehicle
    /// can change into, possibly several lanes over).
    pub fn is_beside(&self, other: &Lane) -> bool {
        other.id != self.id
            && self.segment_id.is_some()
            && other.segment_id == self.segment_id
            && other.from_node_id == self.from_node_id
    }

    /// Free-flow time to drive the whole lane, in sim seconds.
    pub fn travel_time(&self) -> f64 {
        self.length / self.speed_limit
    }
}
