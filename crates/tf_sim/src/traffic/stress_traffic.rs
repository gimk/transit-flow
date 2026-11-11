//! `StressTraffic` (`sim/traffic/stress_traffic.gd`): a stub until work package M4.6 ports it. It only has the methods
//! `Simulation` already calls, with empty bodies.

use crate::events::Outbox;
use crate::ids::VehicleId;
use crate::roads::ChangeSet;

/// Stub: see the module docs.
#[derive(Debug, Default)]
pub struct StressTraffic {}

impl StressTraffic {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_roads_changed(&mut self, _change: &ChangeSet, _out: &mut Outbox) {}

    pub fn on_vehicle_arrived(&mut self, _vehicle: VehicleId, _out: &mut Outbox) {}

    pub fn on_vehicle_failed(&mut self, _vehicle: VehicleId, _out: &mut Outbox) {}

    pub fn step(&mut self, _out: &mut Outbox) {}
}
