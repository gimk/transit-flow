//! `ZoneSystem` (`sim/city/zone_system.gd`): zoned land. The player paints cells
//! Residential, Commercial or Industrial; a zoned cell with road access develops into a
//! building (a lot of the same type) after a short, random delay, and loses it again when
//! unzoned or rezoned. Cells without road access, or where the roads leave no room for a
//! building (`LotSystem::compute_plot`), wait and are re-checked. Lots placed directly
//! (scenarios, tests) are left alone.
//!
//! The roads and lots are passed into the calls that need them.

use glam::IVec2;
use indexmap::IndexMap;
use serde_json::{Value, json};

use super::lot::LotType;
use super::lot_system::{self, LotSystem};
use crate::events::{Outbox, SimEvent};
use crate::grid;
use crate::ids::LotId;
use crate::rng::SimRng;
use crate::roads::{ChangeSet, RoadNetwork};

/// Sim seconds.
pub const DEVELOP_MIN_DELAY: f64 = 2.0;
pub const DEVELOP_MAX_DELAY: f64 = 10.0;
/// How often waiting cells without road access look again.
pub const ACCESS_RETRY: f64 = 3.0;
/// Limits buildings appearing per tick, so painting a big area doesn't stall a frame.
pub const MAX_DEVELOPMENTS_PER_TICK: usize = 8;

#[derive(Clone, Debug)]
pub struct ZoneSystem {
    /// Cell → zone type.
    pub zones: IndexMap<IVec2, LotType>,
    rng: SimRng,
    time: f64,
    /// Cell → sim time at which it tries to develop.
    pending: IndexMap<IVec2, f64>,
    /// Cells that last tried and found no road nearby.
    without_access: IndexMap<IVec2, bool>,
    /// Cell → id of the lot zoning built there.
    zoned_lots: IndexMap<IVec2, LotId>,
}

impl Default for ZoneSystem {
    fn default() -> Self {
        Self {
            zones: IndexMap::new(),
            rng: SimRng::new(7),
            time: 0.0,
            pending: IndexMap::new(),
            without_access: IndexMap::new(),
            zoned_lots: IndexMap::new(),
        }
    }
}

impl ZoneSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn paint(
        &mut self,
        lots: &mut LotSystem,
        cells: &[IVec2],
        zone_type: LotType,
        out: &mut Outbox,
    ) {
        let map = grid::map_rect();
        let mut touched = false;
        for &cell in cells {
            if self.zones.get(&cell) == Some(&zone_type) || !map.has_point(grid::cell_center(cell))
            {
                continue;
            }
            if lots.lot_at_cell(cell).is_some() && !self.zoned_lots.contains_key(&cell) {
                continue; // A hand-placed lot: not ours to replace.
            }
            self.clear_cell(lots, cell, out);
            self.zones.insert(cell, zone_type);
            let delay = self
                .rng
                .randf_range(DEVELOP_MIN_DELAY as f32, DEVELOP_MAX_DELAY as f32);
            self.pending.insert(cell, self.time + f64::from(delay));
            touched = true;
        }
        if touched {
            out.push(SimEvent::ZonesChanged);
        }
    }

    pub fn erase(&mut self, lots: &mut LotSystem, cells: &[IVec2], out: &mut Outbox) {
        let mut touched = false;
        for &cell in cells {
            if self.zones.contains_key(&cell) {
                self.clear_cell(lots, cell, out);
                self.zones.shift_remove(&cell);
                touched = true;
            }
        }
        if touched {
            out.push(SimEvent::ZonesChanged);
        }
    }

    pub fn is_developed(&self, cell: IVec2) -> bool {
        self.zoned_lots.contains_key(&cell)
    }

    pub fn is_waiting_for_road(&self, cell: IVec2) -> bool {
        self.without_access.contains_key(&cell)
    }

    pub fn step(&mut self, time: f64, roads: &RoadNetwork, lots: &mut LotSystem, out: &mut Outbox) {
        self.time = time;
        if self.pending.is_empty() {
            return;
        }
        let mut developed = 0;
        let cells: Vec<IVec2> = self.pending.keys().copied().collect();
        for cell in cells {
            if developed >= MAX_DEVELOPMENTS_PER_TICK {
                break;
            }
            if self.pending[&cell] > time {
                continue;
            }
            if roads
                .find_lane_position(grid::cell_center(cell), lot_system::ACCESS_DISTANCE, 0)
                .is_none()
                || !LotSystem::compute_plot(roads, cell).has_area()
            {
                // No road to reach it, or the roads leave no room for a building.
                self.without_access.insert(cell, true);
                self.pending.insert(cell, time + ACCESS_RETRY);
                continue;
            }
            self.without_access.shift_remove(&cell);
            self.pending.shift_remove(&cell);
            if let Some(lot_id) = lots.add_lot(roads, cell, self.zones[&cell], out) {
                self.zoned_lots.insert(cell, lot_id);
            }
            developed += 1;
        }
        if developed > 0 {
            out.push(SimEvent::ZonesChanged);
        }
    }

    /// Scenarios: every zoned cell with road access develops right away instead of after
    /// its delay.
    pub fn develop_all_now(&mut self, roads: &RoadNetwork, lots: &mut LotSystem, out: &mut Outbox) {
        for when in self.pending.values_mut() {
            *when = self.time;
        }
        let mut waiting = usize::MAX;
        // Each step develops a few cells; cells without access stay pending, so stop when
        // a step no longer shrinks the queue.
        while self.pending.len() != waiting {
            waiting = self.pending.len();
            self.step(self.time, roads, lots, out);
        }
    }

    pub fn to_dict(&self) -> Value {
        let zones: Vec<Value> = self
            .zones
            .iter()
            .map(|(cell, zone_type)| {
                let lot = self.zoned_lots.get(cell).map_or(-1, |id| id.0);
                json!([cell.x, cell.y, *zone_type as i32, lot])
            })
            .collect();
        json!({ "zones": zones })
    }

    /// Replaces all zones with saved ones. Lots must be restored first
    /// (`LotSystem::restore`); zoned cells whose building is missing start developing
    /// again.
    pub fn restore(&mut self, lots: &LotSystem, data: &Value, out: &mut Outbox) {
        let int = |value: &Value| value.as_f64().unwrap_or(0.0) as i32;
        self.zones.clear();
        self.pending.clear();
        self.without_access.clear();
        self.zoned_lots.clear();
        for entry in data["zones"].as_array().into_iter().flatten() {
            let cell = IVec2::new(int(&entry[0]), int(&entry[1]));
            self.zones.insert(cell, LotType::from_index(int(&entry[2])));
            let lot_id = LotId(int(&entry[3]));
            if lot_id.0 != -1 && lots.lots.contains_key(&lot_id) {
                self.zoned_lots.insert(cell, lot_id);
            } else {
                self.pending.insert(cell, self.time);
            }
        }
        out.push(SimEvent::ZonesChanged);
    }

    fn clear_cell(&mut self, lots: &mut LotSystem, cell: IVec2, out: &mut Outbox) {
        self.pending.shift_remove(&cell);
        self.without_access.shift_remove(&cell);
        if let Some(lot_id) = self.zoned_lots.shift_remove(&cell) {
            lots.remove_lot(lot_id, out);
        }
    }

    pub fn on_roads_changed(&mut self, lots: &LotSystem, _change: &ChangeSet, _out: &mut Outbox) {
        // A new road can give waiting cells access: let them look again right away.
        let cells: Vec<IVec2> = self.without_access.keys().copied().collect();
        for cell in cells {
            self.pending.insert(cell, self.time);
        }
        // A road built too close takes a building's land (LotSystem): the cell waits again.
        let cells: Vec<IVec2> = self.zoned_lots.keys().copied().collect();
        for cell in cells {
            if !lots.lots.contains_key(&self.zoned_lots[&cell]) {
                self.zoned_lots.shift_remove(&cell);
                self.without_access.insert(cell, true);
                self.pending.insert(cell, self.time + ACCESS_RETRY);
            }
        }
    }
}
