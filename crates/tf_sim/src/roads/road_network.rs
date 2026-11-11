//! `RoadNetwork` (`sim/roads/road_network.gd`): a stub until work package M1.3 ports it. It only has the methods
//! `Simulation` already calls, with empty bodies.

use crate::ids::{NodeId, SegmentId};

/// What `RoadNetwork.changed(node_ids, segment_ids)` carries: the nodes and segments an
/// edit touched. Ids may refer to things that were just removed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
    pub node_ids: Vec<NodeId>,
    pub segment_ids: Vec<SegmentId>,
}

/// Stub: see the module docs.
#[derive(Debug, Default)]
pub struct RoadNetwork {}

impl RoadNetwork {
    pub fn new() -> Self {
        Self::default()
    }
}
