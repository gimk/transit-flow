//! `TrafficLights` (`sim/traffic/traffic_lights.gd`): a stub until work package M4.1 ports it. It only has the methods
//! `Simulation` already calls, with empty bodies.

use crate::events::Outbox;
use crate::roads::ChangeSet;

/// Stub: see the module docs.
#[derive(Debug, Default)]
pub struct TrafficLights {}

impl TrafficLights {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_roads_changed(&mut self, _change: &ChangeSet, _out: &mut Outbox) {}

    pub fn step(&mut self, _dt: f64, _out: &mut Outbox) {}
}
