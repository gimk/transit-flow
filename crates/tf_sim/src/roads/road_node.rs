//! `RoadNode` (`sim/roads/road_node.gd`): a point where road segments meet: a dead end,
//! a corner or a junction. Owns the connector lanes that link incoming lanes to outgoing
//! lanes through it.

use glam::Vec2;

use crate::ids::{LaneId, NodeId, SegmentId};

/// How vehicles take turns at a junction. `TrafficLightProtectedLeft`: lights with a
/// phase of their own for left turns, so they don't have to find gaps in oncoming
/// traffic. The values are Godot's (saved by name, compared as ints in golden data).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum JunctionControl {
    #[default]
    None = 0,
    TrafficLight = 1,
    TrafficLightProtectedLeft = 2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoadNode {
    pub id: NodeId,
    pub position: Vec2,
    /// -1 tunnel/subway, 0 ground, 1+ bridges. Roads only connect on the same layer.
    pub elevation_layer: i32,
    pub segment_ids: Vec<SegmentId>,
    pub connector_lane_ids: Vec<LaneId>,
    /// How vehicles take turns at this junction (only junctions of 3+ roads can be
    /// controlled).
    pub control: JunctionControl,
    /// Traffic-light phases among the connectors (see `Lane::signal_phase`); 1 = no
    /// real alternation possible.
    pub signal_phase_count: i32,
}

impl RoadNode {
    pub fn new(id: NodeId, position: Vec2, elevation_layer: i32) -> Self {
        Self {
            id,
            position,
            elevation_layer,
            segment_ids: Vec::new(),
            connector_lane_ids: Vec::new(),
            control: JunctionControl::None,
            signal_phase_count: 1,
        }
    }

    pub fn degree(&self) -> usize {
        self.segment_ids.len()
    }

    pub fn has_lights(&self) -> bool {
        self.control != JunctionControl::None
    }
}
