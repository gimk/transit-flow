//! `Crosswalks` (`sim/walking/crosswalks.gd`): who is on, or waiting at, each crosswalk:
//! shared by pedestrians (who update it), cars (which give way to them) and traffic
//! lights (waiting pedestrians call the walk phase). A crosswalk at a junction is
//! identified by (junction node id, id of the road it crosses). Mid-block crosswalks (at
//! bus stops, laid out by TransitSystem) by (MID_BLOCK_BASE - stop id, id of the road),
//! and described in `mid_block`.

use glam::{IVec2, Vec2};
use indexmap::IndexMap;

use crate::ids::{LaneId, SegmentId};

pub const NONE: IVec2 = IVec2::new(-1, -1);
pub const MID_BLOCK_BASE: i32 = -1000;

/// A mid-block crosswalk (the GDScript dictionary `{ "segment_id", "distance", "center",
/// "across", "half_width", "lanes" }`).
#[derive(Clone, Debug, PartialEq)]
pub struct MidBlockCrossing {
    pub segment_id: SegmentId,
    /// Along the road from its start.
    pub distance: f64,
    pub center: Vec2,
    /// Unit vector across the road.
    pub across: Vec2,
    pub half_width: f64,
    /// Every lane it crosses, and where along it.
    pub lanes: Vec<(LaneId, f64)>,
}

#[derive(Clone, Debug, Default)]
pub struct Crosswalks {
    /// Crosswalk → number of pedestrians on it / waiting at its curb.
    pub walkers: IndexMap<IVec2, i32>,
    pub waiting: IndexMap<IVec2, i32>,
    pub mid_block: IndexMap<IVec2, MidBlockCrossing>,
    /// Goes up whenever `mid_block` changes (WalkNetwork rebuilds then).
    pub mid_block_version: u64,
}

impl Crosswalks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_mid_block(crosswalk: IVec2) -> bool {
        crosswalk.x <= MID_BLOCK_BASE
    }

    pub fn set_mid_block(&mut self, crossings: IndexMap<IVec2, MidBlockCrossing>) {
        self.mid_block = crossings;
        self.mid_block_version += 1;
    }

    pub fn is_empty(&self) -> bool {
        self.walkers.is_empty() && self.waiting.is_empty()
    }

    pub fn has_walkers(&self, crosswalk: IVec2) -> bool {
        self.walkers.contains_key(&crosswalk)
    }

    pub fn has_waiting(&self, crosswalk: IVec2) -> bool {
        self.waiting.contains_key(&crosswalk)
    }

    pub fn add_walker(&mut self, crosswalk: IVec2) {
        Self::change(&mut self.walkers, crosswalk, 1);
    }

    pub fn remove_walker(&mut self, crosswalk: IVec2) {
        Self::change(&mut self.walkers, crosswalk, -1);
    }

    pub fn add_waiting(&mut self, crosswalk: IVec2) {
        Self::change(&mut self.waiting, crosswalk, 1);
    }

    pub fn remove_waiting(&mut self, crosswalk: IVec2) {
        Self::change(&mut self.waiting, crosswalk, -1);
    }

    pub fn clear(&mut self) {
        self.walkers.clear();
        self.waiting.clear();
    }

    fn change(counts: &mut IndexMap<IVec2, i32>, crosswalk: IVec2, by: i32) {
        let count = counts.get(&crosswalk).copied().unwrap_or(0) + by;
        if count > 0 {
            counts.insert(crosswalk, count);
        } else {
            counts.shift_remove(&crosswalk);
        }
    }
}
