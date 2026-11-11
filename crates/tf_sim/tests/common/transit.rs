//! Helpers for tests that use bus stops and lines.

use glam::Vec2;
use tf_sim::Simulation;
use tf_sim::ids::{LineId, StopId};

pub fn add_stop(sim: &mut Simulation, point: Vec2, bay: bool) -> Option<StopId> {
    let (transit, mut ctx) = sim.transit_ctx();
    transit.add_stop(&mut ctx, point, bay)
}

pub fn add_line(sim: &mut Simulation, stops: &[StopId]) -> Option<LineId> {
    let (transit, mut ctx) = sim.transit_ctx();
    transit.add_line(&mut ctx, stops)
}
