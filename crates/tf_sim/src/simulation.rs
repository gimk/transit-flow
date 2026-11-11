//! `Simulation` (`sim/simulation.gd`): the root of the simulation state. Pure data and
//! logic, so it can be stepped headless from tests.
//!
//! It owns every system and plays the part of Godot's signal connections: systems push
//! events into an `Outbox`, and `deliver` hands each one to its listeners in the order
//! `Simulation._init` connected them, depth first (a listener's own events are
//! delivered before the next listener runs), which is the order Godot runs them in.

use crate::city::citizen_system::CitizenCtx;
use crate::city::{CitizenSystem, ExternalConnections, FlowMetric, LotSystem, ZoneSystem};
use crate::clock::SimClock;
use crate::events::{Outbox, SimEvent};
use crate::roads::RoadNetwork;
use crate::routing::Router;
use crate::traffic::vehicle_system::TrafficCtx;
use crate::traffic::{
    CongestionMonitor, FreightSystem, StressTraffic, TrafficLights, VehicleSystem,
};
use crate::transit::{TransitCtx, TransitSystem};
use crate::walking::{Crosswalks, PedestrianSystem, WalkCtx};

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

    /// `Simulation.new(roads)`: a simulation on an existing road network (loaded saves,
    /// the benchmark). The systems that index the roads see them from the start.
    pub fn with_roads(roads: RoadNetwork) -> Self {
        let lots = LotSystem::new(&roads);
        let signals = TrafficLights::new(&roads);
        Self {
            roads,
            lots,
            signals,
            ..Self::default()
        }
    }

    /// Simulated seconds since the start of the game.
    pub fn sim_time(&self) -> f64 {
        self.tick_count as f64 * Self::TICK_DT
    }

    pub fn step(&mut self) {
        // Vehicles and pedestrians that finished last tick stay readable until now (Godot
        // keeps their objects for whoever holds them; tests and views read them).
        self.vehicles.clear_finished();
        self.pedestrians.clear_finished();
        self.tick_count += 1;
        let time = self.sim_time();
        let dt = Self::TICK_DT;
        self.call(|sim, out| sim.clock.advance(dt, out));
        self.call(|sim, out| sim.zones.step(time, &sim.roads, &mut sim.lots, out));
        self.call(|sim, out| {
            let (citizens, mut ctx) = sim.citizens_ctx();
            citizens.step(time, &mut ctx, out);
        });
        self.call(|sim, out| {
            let ctx = &mut TrafficCtx {
                roads: &sim.roads,
                router: &mut sim.router,
                lots: &mut sim.lots,
                signals: &sim.signals,
                crosswalks: &sim.crosswalks,
            };
            sim.freight
                .step(dt, &sim.clock, &sim.externals, &mut sim.vehicles, ctx, out);
        });
        self.call(|sim, _out| {
            sim.stress
                .step(&sim.roads, &mut sim.router, &mut sim.vehicles)
        });
        self.call(|sim, _out| {
            sim.signals
                .step(dt, &sim.roads, &sim.vehicles, &sim.crosswalks)
        });
        self.call(|sim, out| {
            let (pedestrians, mut ctx) = sim.walking();
            pedestrians.step(time, dt, &mut ctx, out);
        });
        self.call(|sim, out| {
            let (transit, mut ctx) = sim.transit_ctx();
            transit.step(time, &mut ctx, out);
        });
        self.call(|sim, out| {
            let (vehicles, mut ctx) = sim.traffic();
            vehicles.step_begin(dt, &mut ctx, out);
        });
        self.call(|sim, out| {
            let (vehicles, mut ctx) = sim.traffic();
            vehicles.step_rest(dt, &mut ctx, out);
        });
        self.call(|sim, out| {
            let mut ctx = TrafficCtx {
                roads: &sim.roads,
                router: &mut sim.router,
                lots: &mut sim.lots,
                signals: &sim.signals,
                crosswalks: &sim.crosswalks,
            };
            sim.congestion.step(time, &mut sim.vehicles, &mut ctx, out);
        });
        self.vehicles.prune_retired_lanes(&mut self.roads);
    }

    /// The vehicle system and what it works with, borrowed apart (`VehicleSystem` takes a
    /// `TrafficCtx` where the GDScript class holds references to the other systems).
    pub fn traffic(&mut self) -> (&mut VehicleSystem, TrafficCtx<'_>) {
        let ctx = TrafficCtx {
            roads: &self.roads,
            router: &mut self.router,
            lots: &mut self.lots,
            signals: &self.signals,
            crosswalks: &self.crosswalks,
        };
        (&mut self.vehicles, ctx)
    }

    /// The pedestrian system and what it works with, borrowed apart (`PedestrianSystem`
    /// takes a `WalkCtx` where the GDScript class holds references to the other systems).
    pub fn walking(&mut self) -> (&mut PedestrianSystem, WalkCtx<'_>) {
        let ctx = WalkCtx {
            roads: &self.roads,
            lots: &self.lots,
            signals: &self.signals,
            vehicles: &self.vehicles,
            crosswalks: &mut self.crosswalks,
        };
        (&mut self.pedestrians, ctx)
    }

    /// The transit system and what it works with, borrowed apart (`TransitSystem` takes a
    /// `TransitCtx` where the GDScript class holds references to the other systems).
    pub fn transit_ctx(&mut self) -> (&mut TransitSystem, TransitCtx<'_>) {
        let ctx = TransitCtx {
            roads: &self.roads,
            router: &mut self.router,
            vehicles: &mut self.vehicles,
            pedestrians: &mut self.pedestrians,
            crosswalks: &mut self.crosswalks,
        };
        (&mut self.transit, ctx)
    }

    /// The citizen system and what it works with, borrowed apart (`CitizenSystem` takes
    /// a `CitizenCtx` where the GDScript class holds references to the other systems).
    pub fn citizens_ctx(&mut self) -> (&mut CitizenSystem, CitizenCtx<'_>) {
        let ctx = CitizenCtx {
            roads: &self.roads,
            router: &mut self.router,
            lots: &mut self.lots,
            signals: &self.signals,
            vehicles: &mut self.vehicles,
            pedestrians: &mut self.pedestrians,
            transit: &mut self.transit,
            crosswalks: &mut self.crosswalks,
            clock: &self.clock,
        };
        (&mut self.citizens, ctx)
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
                self.call(|sim, out| sim.lots.on_roads_changed(&sim.roads, change, out));
                self.call(|sim, out| sim.zones.on_roads_changed(&sim.lots, change, out));
                self.call(|sim, out| sim.signals.on_roads_changed(&sim.roads, change, out));
                self.call(|sim, out| sim.vehicles.on_roads_changed(change, out));
                self.call(|sim, out| sim.pedestrians.on_roads_changed(change, out));
                self.call(|sim, out| sim.transit.on_roads_changed(change, out));
                self.call(|sim, out| sim.stress.on_roads_changed(change, out));
            }
            SimEvent::LotsChanged => {
                self.call(|sim, out| sim.vehicles.request_reroute(out));
                self.call(|sim, out| {
                    let (citizens, mut ctx) = sim.citizens_ctx();
                    citizens.sync_with_lots(&mut ctx, out);
                });
                self.call(|sim, out| sim.freight.on_lots_changed(&sim.lots, out));
            }
            &SimEvent::VehicleArrived(id) => {
                self.call(|sim, out| {
                    let (transit, mut ctx) = sim.transit_ctx();
                    transit.on_vehicle_arrived(id, &mut ctx, out);
                });
                self.call(|sim, out| {
                    let (citizens, mut ctx) = sim.citizens_ctx();
                    citizens.on_vehicle_arrived(id, &mut ctx, out);
                });
                self.call(|sim, out| {
                    let owner = sim.vehicles.vehicle_any(id).map(|v| v.owner_id);
                    sim.stress.on_vehicle_gone(owner, out);
                });
                self.call(|sim, _out| {
                    if let Some(vehicle) = sim.vehicles.vehicle_any(id) {
                        sim.flow.on_vehicle_arrived(vehicle);
                    }
                });
            }
            &SimEvent::VehicleFailed(id) => {
                self.call(|sim, out| {
                    let (transit, mut ctx) = sim.transit_ctx();
                    transit.on_vehicle_failed(id, &mut ctx, out);
                });
                self.call(|sim, out| {
                    let (citizens, mut ctx) = sim.citizens_ctx();
                    citizens.on_vehicle_failed(id, &mut ctx, out);
                });
                self.call(|sim, out| {
                    let owner = sim.vehicles.vehicle_any(id).map(|v| v.owner_id);
                    sim.stress.on_vehicle_gone(owner, out);
                });
                self.call(|sim, _out| {
                    if let Some(vehicle) = sim.vehicles.vehicle_any(id) {
                        sim.flow.on_vehicle_failed(vehicle);
                    }
                });
            }
            &SimEvent::PedestrianArrived(id) => {
                self.call(|sim, out| {
                    let (citizens, mut ctx) = sim.citizens_ctx();
                    citizens.on_pedestrian_arrived(id, &mut ctx, out);
                });
            }
            &SimEvent::RiderArrived { citizen, door } => {
                self.call(|sim, out| {
                    let (citizens, mut ctx) = sim.citizens_ctx();
                    citizens.on_rider_arrived(citizen, door, &mut ctx, out);
                });
            }
            &SimEvent::RiderStranded(citizen) => {
                self.call(|sim, out| {
                    let (citizens, mut ctx) = sim.citizens_ctx();
                    citizens.on_rider_stranded(citizen, &mut ctx, out);
                });
            }
            &SimEvent::TripEnded { citizen, completed } => {
                self.call(|sim, _out| {
                    if let Some(citizen) = sim.citizens.citizens.get(&citizen) {
                        sim.flow.on_trip_ended(citizen, completed);
                    }
                });
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
