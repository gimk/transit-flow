//! Helpers for tests that use citizens.

use glam::IVec2;
use tf_sim::Simulation;
use tf_sim::city::LotType;
use tf_sim::city::citizen::{Citizen, Mode};
use tf_sim::city::citizen_system::Plan;
use tf_sim::events::SimEvent;
use tf_sim::ids::{CitizenId, LotId};

/// `sim.lots.add_lot(cell, type)` (the lot, or `None` where it doesn't fit).
pub fn add_lot(sim: &mut Simulation, cell: IVec2, lot_type: LotType) -> Option<LotId> {
    let mut id = None;
    sim.call(|sim, out| id = sim.lots.add_lot(&sim.roads, cell, lot_type, out));
    id
}

/// `sim.citizens.citizens.values()[index]`.
pub fn nth(sim: &Simulation, index: usize) -> CitizenId {
    *sim.citizens
        .citizens
        .get_index(index)
        .expect("test setup: citizens moved in")
        .0
}

pub fn citizen(sim: &mut Simulation, id: CitizenId) -> &mut Citizen {
    &mut sim.citizens.citizens[&id]
}

/// `sim.citizens.plan_trip(citizen, from, to, going_home)`.
pub fn plan_trip(
    sim: &mut Simulation,
    citizen: CitizenId,
    from: LotId,
    to: LotId,
    going_home: bool,
) -> Option<Plan> {
    let mut plan = None;
    sim.call(|sim, out| {
        let (citizens, mut ctx) = sim.citizens_ctx();
        plan = citizens.plan_trip(&mut ctx, citizen, from, to, going_home, out);
    });
    plan
}

/// The modes of a plan's legs.
pub fn modes(plan: &Plan) -> Vec<Mode> {
    plan.legs.iter().map(|leg| leg.mode()).collect()
}

/// The `trip_ended` signals since the last call: (citizen, completed).
pub fn trips_ended(sim: &mut Simulation) -> Vec<(CitizenId, bool)> {
    sim.take_view_events()
        .into_iter()
        .filter_map(|event| match event {
            SimEvent::TripEnded { citizen, completed } => Some((citizen, completed)),
            _ => None,
        })
        .collect()
}
