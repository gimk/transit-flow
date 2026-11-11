//! `Simulation` (`sim/simulation.gd`): the root of the simulation state. Pure data and
//! logic, so it can be stepped headless from tests.
//!
//! It owns every system and plays the part of Godot's signal connections: systems push
//! events into an `Outbox`, and `deliver` hands each one to its listeners in the order
//! `Simulation._init` connected them, depth first (a listener's own events are
//! delivered before the next listener runs), which is the order Godot runs them in.

use crate::city::{CitizenSystem, ExternalConnections, FlowMetric, LotSystem, ZoneSystem};
use crate::clock::SimClock;
use crate::events::{Outbox, SimEvent};
use crate::roads::RoadNetwork;
use crate::routing::Router;
use crate::traffic::{
    CongestionMonitor, FreightSystem, StressTraffic, TrafficLights, VehicleSystem,
};
use crate::transit::TransitSystem;
use crate::walking::{Crosswalks, PedestrianSystem};

#[derive(Debug, Default)]
pub struct Simulation {
    pub clock: SimClock,
    pub roads: RoadNetwork,
    pub router: Router,
    pub lots: LotSystem,
    pub zones: ZoneSystem,
    pub externals: ExternalConnections,
    pub signals: TrafficLights,
    pub vehicles: VehicleSystem,
    pub crosswalks: Crosswalks,
    pub pedestrians: PedestrianSystem,
    pub transit: TransitSystem,
    pub citizens: CitizenSystem,
    pub freight: FreightSystem,
    pub stress: StressTraffic,
    pub congestion: CongestionMonitor,
    pub flow: FlowMetric,
    pub tick_count: u64,
    /// Every event delivered since the game last took them (`take_view_events`): the
    /// signals Godot relayed to the views through `Events`.
    view_events: Vec<SimEvent>,
}

impl Simulation {
    pub const TICK_RATE: u32 = 20;
    pub const TICK_DT: f64 = 1.0 / Self::TICK_RATE as f64;
    pub const SAVE_VERSION: i32 = 1;

    /// An empty map. Systems are created in dependency order (roads → router → lots →
    /// zones → externals → signals → vehicles → pedestrians → transit → citizens →
    /// freight → stress → congestion → flow), the order their listeners run in.
    pub fn new() -> Self {
        Self::default()
    }

    /// Simulated seconds since the start of the game.
    pub fn sim_time(&self) -> f64 {
        self.tick_count as f64 * Self::TICK_DT
    }

    pub fn step(&mut self) {
        self.tick_count += 1;
        let time = self.sim_time();
        let dt = Self::TICK_DT;
        self.call(|sim, out| sim.clock.advance(dt, out));
        self.call(|sim, out| sim.zones.step(time, out));
        self.call(|sim, out| sim.citizens.step(time, out));
        self.call(|sim, out| sim.freight.step(dt, out));
        self.call(|sim, out| sim.stress.step(out));
        self.call(|sim, out| sim.signals.step(dt, out));
        self.call(|sim, out| sim.pedestrians.step(time, dt, out));
        self.call(|sim, out| sim.transit.step(time, out));
        self.call(|sim, out| sim.vehicles.step_begin(dt, out));
        self.call(|sim, out| sim.vehicles.step_rest(dt, out));
        self.call(|sim, out| sim.congestion.step(time, out));
        self.vehicles.clear_finished();
        self.pedestrians.clear_finished();
    }

    /// The events delivered since the last call, oldest first, for the views.
    pub fn take_view_events(&mut self) -> Vec<SimEvent> {
        std::mem::take(&mut self.view_events)
    }

    /// Runs `f` with a fresh outbox, then delivers what it emitted. Every call into a
    /// system that may emit goes through here (or through a command method that does).
    pub fn call(&mut self, f: impl FnOnce(&mut Self, &mut Outbox)) {
        let mut out = Outbox::new();
        f(self, &mut out);
        self.deliver(out);
    }

    /// Delivers `events` in order. Each listener's own events are delivered before the
    /// next listener runs, as with Godot's synchronous signals. The views get every
    /// event last (Game connected after the systems).
    fn deliver(&mut self, events: Outbox) {
        for event in events {
            self.dispatch(&event);
            self.view_events.push(event);
        }
    }

    /// The listeners of each signal, in `Simulation._init`'s connection order.
    fn dispatch(&mut self, event: &SimEvent) {
        match event {
            SimEvent::RoadsChanged(change) => {
                self.call(|sim, out| sim.router.on_roads_changed(change, out));
                self.call(|sim, out| sim.lots.on_roads_changed(change, out));
                self.call(|sim, out| sim.zones.on_roads_changed(change, out));
                self.call(|sim, out| sim.signals.on_roads_changed(change, out));
                self.call(|sim, out| sim.vehicles.on_roads_changed(change, out));
                self.call(|sim, out| sim.pedestrians.on_roads_changed(change, out));
                self.call(|sim, out| sim.transit.on_roads_changed(change, out));
                self.call(|sim, out| sim.stress.on_roads_changed(change, out));
            }
            SimEvent::LotsChanged => {
                self.call(|sim, out| sim.vehicles.request_reroute(out));
                self.call(|sim, out| sim.citizens.sync_with_lots(out));
                self.call(|sim, out| sim.freight.on_lots_changed(out));
            }
            &SimEvent::VehicleArrived(id) => {
                self.call(|sim, out| sim.transit.on_vehicle_arrived(id, out));
                self.call(|sim, out| sim.citizens.on_vehicle_arrived(id, out));
                self.call(|sim, out| sim.stress.on_vehicle_arrived(id, out));
                self.call(|sim, out| sim.flow.on_vehicle_arrived(id, out));
            }
            &SimEvent::VehicleFailed(id) => {
                self.call(|sim, out| sim.transit.on_vehicle_failed(id, out));
                self.call(|sim, out| sim.citizens.on_vehicle_failed(id, out));
                self.call(|sim, out| sim.stress.on_vehicle_failed(id, out));
                self.call(|sim, out| sim.flow.on_vehicle_failed(id, out));
            }
            &SimEvent::PedestrianArrived(id) => {
                self.call(|sim, out| sim.citizens.on_pedestrian_arrived(id, out));
            }
            &SimEvent::RiderArrived { citizen, door } => {
                self.call(|sim, out| sim.citizens.on_rider_arrived(citizen, door, out));
            }
            &SimEvent::RiderStranded(citizen) => {
                self.call(|sim, out| sim.citizens.on_rider_stranded(citizen, out));
            }
            &SimEvent::TripEnded { citizen, completed } => {
                self.call(|sim, out| sim.flow.on_trip_ended(citizen, completed, out));
            }
            // Views only.
            SimEvent::ZonesChanged
            | SimEvent::ExternalsChanged
            | SimEvent::HourChanged(_)
            | SimEvent::DayStarted(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_advance_time_and_reach_the_views() {
        let mut sim = Simulation::new();
        // One game hour = 12 sim seconds = 240 ticks.
        for _ in 0..240 {
            sim.step();
        }
        assert!((sim.sim_time() - 12.0).abs() < 1e-9);
        assert_eq!(sim.clock.format_time(), "07:00");
        assert_eq!(sim.take_view_events(), vec![SimEvent::HourChanged(7)]);
        assert!(sim.take_view_events().is_empty());
    }
}
