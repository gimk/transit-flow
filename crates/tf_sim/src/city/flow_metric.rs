//! `FlowMetric` (`sim/city/flow_metric.gd`): a stub until work package M7.2 ports it. It only has the methods
//! `Simulation` already calls, with empty bodies.

use crate::events::Outbox;
use crate::ids::{CitizenId, VehicleId};

/// Stub: see the module docs.
#[derive(Debug, Default)]
pub struct FlowMetric {}

impl FlowMetric {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_vehicle_arrived(&mut self, _vehicle: VehicleId, _out: &mut Outbox) {}

    pub fn on_vehicle_failed(&mut self, _vehicle: VehicleId, _out: &mut Outbox) {}

    pub fn on_trip_ended(&mut self, _citizen: CitizenId, _completed: bool, _out: &mut Outbox) {}
}
