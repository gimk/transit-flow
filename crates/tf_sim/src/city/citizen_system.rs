//! `CitizenSystem` (`sim/city/citizen_system.gd`): a stub until work package M7.1 ports it. It only has the methods
//! `Simulation` already calls, with empty bodies.

use glam::Vec2;

use crate::events::Outbox;
use crate::ids::{CitizenId, PedestrianId, VehicleId};

/// Stub: see the module docs.
#[derive(Debug, Default)]
pub struct CitizenSystem {}

impl CitizenSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// `LotSystem.changed` → `_sync_with_lots`.
    pub fn sync_with_lots(&mut self, _out: &mut Outbox) {}

    pub fn on_vehicle_arrived(&mut self, _vehicle: VehicleId, _out: &mut Outbox) {}

    pub fn on_vehicle_failed(&mut self, _vehicle: VehicleId, _out: &mut Outbox) {}

    pub fn on_pedestrian_arrived(&mut self, _pedestrian: PedestrianId, _out: &mut Outbox) {}

    pub fn on_rider_arrived(&mut self, _citizen: CitizenId, _door: Vec2, _out: &mut Outbox) {}

    pub fn on_rider_stranded(&mut self, _citizen: CitizenId, _out: &mut Outbox) {}

    pub fn step(&mut self, _time: f64, _out: &mut Outbox) {}
}
