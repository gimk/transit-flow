//! `Parking` (`sim/city/parking.gd`): where citizens' cars are parked. Homes have room
//! for their residents' cars (not tracked); shops and factories have a few spaces each
//! (`capacity`), shared by their workers and visitors. A driver heading for a full lot
//! parks at the nearest lot with a free space within walking reach and walks the rest
//! (`find_space`); if there is none, they park on the street by their destination, over
//! capacity. Spaces are taken when a trip is planned, so two drivers can't count on the
//! last one.
//!
//! The lots, and the walking distance between two lots (`PedestrianSystem`, ported in
//! M5.2), are passed into the calls that need them.

use indexmap::IndexMap;

use super::lot::LotType;
use super::lot_system::LotSystem;
use crate::ids::LotId;

#[derive(Clone, Debug, Default)]
pub struct Parking {
    /// Lot id → spaces taken (may exceed its capacity: street parking).
    used: IndexMap<LotId, i32>,
}

impl Parking {
    /// `CAPACITY`: spaces per lot type (homes aren't counted).
    pub fn capacity_of(lot_type: LotType) -> Option<i32> {
        match lot_type {
            LotType::Commercial => Some(3),
            LotType::Industrial => Some(4),
            LotType::Residential => None,
        }
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn capacity(&self, lots: &LotSystem, lot_id: LotId) -> i32 {
        lots.lots
            .get(&lot_id)
            .and_then(|lot| Self::capacity_of(lot.lot_type))
            .unwrap_or(0)
    }

    pub fn used(&self, lot_id: LotId) -> i32 {
        self.used.get(&lot_id).copied().unwrap_or(0)
    }

    pub fn has_space(&self, lots: &LotSystem, lot_id: LotId) -> bool {
        self.used(lot_id) < self.capacity(lots, lot_id)
    }

    /// Is the lot somewhere spaces are counted (not a home)?
    pub fn is_tracked(&self, lots: &LotSystem, lot_id: LotId) -> bool {
        lots.lots
            .get(&lot_id)
            .is_some_and(|lot| Self::capacity_of(lot.lot_type).is_some())
    }

    pub fn take(&mut self, lots: &LotSystem, lot_id: LotId) {
        if self.is_tracked(lots, lot_id) {
            *self.used.entry(lot_id).or_insert(0) += 1;
        }
    }

    pub fn release(&mut self, lot_id: LotId) {
        if let Some(used) = self.used.get_mut(&lot_id) {
            *used -= 1;
            if *used <= 0 {
                self.used.shift_remove(&lot_id);
            }
        }
    }

    /// Where to park for a visit to `lot_id`: the lot itself if it has a free space (or
    /// is a home), else the nearest lot with one within `max_walk` px on foot, else the
    /// lot itself (on the street). `walking_distance(from, to)` is
    /// `PedestrianSystem.get_walking_distance`.
    pub fn find_space(
        &self,
        lots: &LotSystem,
        lot_id: LotId,
        max_walk: f64,
        mut walking_distance: impl FnMut(LotId, LotId) -> f64,
    ) -> LotId {
        if !self.is_tracked(lots, lot_id) || self.has_space(lots, lot_id) {
            return lot_id;
        }
        let center = lots.lots[&lot_id].center();
        let mut candidates: Vec<_> = lots
            .lots
            .values()
            .filter(|lot| {
                lot.id != lot_id
                    && self.is_tracked(lots, lot.id)
                    && self.has_space(lots, lot.id)
                    && f64::from(lot.center().distance(center)) <= max_walk
            })
            .collect();
        // Godot's sort_custom isn't stable for long lists; ties in distance are rare here.
        candidates.sort_by(|a, b| {
            a.center()
                .distance_squared(center)
                .total_cmp(&b.center().distance_squared(center))
        });
        for lot in candidates {
            if walking_distance(lot.id, lot_id) <= max_walk {
                return lot.id;
            }
        }
        lot_id
    }

    /// Forgets lots that are gone.
    pub fn sync(&mut self, lots: &LotSystem) {
        self.used.retain(|lot_id, _| lots.lots.contains_key(lot_id));
    }
}
