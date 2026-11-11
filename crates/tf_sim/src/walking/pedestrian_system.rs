//! `PedestrianSystem` (`sim/walking/pedestrian_system.gd`): a stub until work package M5.2 ports it. It only has the methods
//! `Simulation` already calls, with empty bodies.

use crate::events::Outbox;
use crate::roads::ChangeSet;

/// Stub: see the module docs.
#[derive(Debug, Default)]
pub struct PedestrianSystem {}

impl PedestrianSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// `RoadNetwork.changed` → its `WalkNetwork._on_roads_changed` (the walk network is
    /// created inside `PedestrianSystem._init`, so it listens in that place).
    pub fn on_roads_changed(&mut self, _change: &ChangeSet, _out: &mut Outbox) {}

    pub fn step(&mut self, _time: f64, _dt: f64, _out: &mut Outbox) {}

    /// Drops the pedestrians who arrived once their events have been delivered.
    pub fn clear_finished(&mut self) {}
}
