//! `CongestionMonitor` (`sim/traffic/congestion_monitor.gd`): a stub until work package M4.6 ports it. It only has the methods
//! `Simulation` already calls, with empty bodies.

use crate::events::Outbox;

/// Stub: see the module docs.
#[derive(Debug, Default)]
pub struct CongestionMonitor {}

impl CongestionMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, _time: f64, _out: &mut Outbox) {}
}
