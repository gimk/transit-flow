//! `VehicleSystem` (`sim/traffic/vehicle_system.gd`): a stub until work package M4.2 ports it. It only has the methods
//! `Simulation` already calls, with empty bodies.

use crate::events::Outbox;
use crate::roads::ChangeSet;

/// Stub: see the module docs.
#[derive(Debug, Default)]
pub struct VehicleSystem {}

impl VehicleSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_roads_changed(&mut self, _change: &ChangeSet, _out: &mut Outbox) {}

    /// `LotSystem.changed` → `_request_reroute`.
    pub fn request_reroute(&mut self, _out: &mut Outbox) {}

    /// The start of `step(dt)`: advance the clock and re-route everyone if the roads or
    /// lots changed. Split from `step_rest` because re-routing emits `vehicle_failed`,
    /// whose listeners run before the rest of the step in Godot.
    pub fn step_begin(&mut self, _dt: f64, _out: &mut Outbox) {}

    /// The rest of `step(dt)`: driveways, lane changes, movement, arrivals, give-ups.
    pub fn step_rest(&mut self, _dt: f64, _out: &mut Outbox) {}

    /// Drops the vehicles that left the road once their events have been delivered.
    pub fn clear_finished(&mut self) {}
}
