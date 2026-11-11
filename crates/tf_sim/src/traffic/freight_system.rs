//! `FreightSystem` (`sim/traffic/freight_system.gd`): a stub until work package M4.7 ports it. It only has the methods
//! `Simulation` already calls, with empty bodies.

use crate::events::Outbox;

/// Stub: see the module docs.
#[derive(Debug, Default)]
pub struct FreightSystem {}

impl FreightSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_lots_changed(&mut self, _out: &mut Outbox) {}

    pub fn step(&mut self, _dt: f64, _out: &mut Outbox) {}
}
