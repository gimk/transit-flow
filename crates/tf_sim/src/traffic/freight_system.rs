//! `FreightSystem` (`sim/traffic/freight_system.gd`): trucks. Through the day, factories
//! receive raw materials from outside the city, ship goods to shops and export the rest;
//! shops also import goods from outside. Trips are drawn at random per lot, at a rate that
//! follows the time of day: busiest in the midday logistics window (10:00-16:00), lighter
//! in the rush hours, a trickle at night.

use glam::Vec2;

use crate::city::{ExternalConnections, LotSystem, LotType};
use crate::clock::SimClock;
use crate::events::Outbox;
use crate::ids::LotId;
use crate::rng::SimRng;
use crate::routing::LanePoint;
use crate::traffic::vehicle::Kind;
use crate::traffic::vehicle_system::{TrafficCtx, VehicleSystem};

pub const OWNER_ID: i32 = -3;
/// Trucks per lot per game hour at the busiest time of day. (Lowered by a third with
/// driveways, Phase 6: before, trucks that found no room to appear were silently dropped,
/// so these gave about that much less traffic.)
pub const RAW_MATERIALS_PER_FACTORY: f64 = 0.2;
pub const GOODS_PER_FACTORY: f64 = 0.27;
pub const EXPORTS_PER_FACTORY: f64 = 0.1;
pub const IMPORTS_PER_SHOP: f64 = 0.1;

const SEED: u64 = 23;

#[derive(Clone, Debug)]
pub struct FreightSystem {
    rng: SimRng,
    factories: Vec<LotId>,
    shops: Vec<LotId>,
}

impl Default for FreightSystem {
    fn default() -> Self {
        Self {
            rng: SimRng::new(SEED),
            factories: Vec::new(),
            shops: Vec::new(),
        }
    }
}

impl FreightSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// Relative freight activity at a given hour of the day (1 = midday peak).
    pub fn activity(hour: f64) -> f64 {
        if (10.0..16.0).contains(&hour) {
            return 1.0;
        }
        if (6.0..20.0).contains(&hour) {
            return 0.35;
        }
        0.12
    }

    pub fn step(
        &mut self,
        dt: f64,
        clock: &SimClock,
        externals: &ExternalConnections,
        vehicles: &mut VehicleSystem,
        ctx: &mut TrafficCtx,
        _out: &mut Outbox,
    ) {
        if self.factories.is_empty() && self.shops.is_empty() {
            return;
        }
        let game_hours = dt * SimClock::GAME_MINUTES_PER_SIM_SECOND / 60.0;
        let level = Self::activity(clock.time_of_day_hours()) * game_hours;
        let gates = externals.active_points(ctx.roads);
        let roads = ctx.roads;
        for i in 0..self.factories.len() {
            let factory = self.factories[i];
            if ctx.lots.access_points(roads, factory).is_empty() {
                continue;
            }
            if !gates.is_empty() && self.chance(RAW_MATERIALS_PER_FACTORY * level) {
                let from = gate(externals.inbound(roads, self.pick(&gates)));
                send(vehicles, ctx, None, from, Some(factory), Vec::new());
            }
            if !self.shops.is_empty() && self.chance(GOODS_PER_FACTORY * level) {
                let shop = self.shops[(self.rng.randi() % self.shops.len() as i64) as usize];
                send(
                    vehicles,
                    ctx,
                    Some(factory),
                    Vec::new(),
                    Some(shop),
                    Vec::new(),
                );
            }
            if !gates.is_empty() && self.chance(EXPORTS_PER_FACTORY * level) {
                let to = gate(externals.outbound(roads, self.pick(&gates)));
                send(vehicles, ctx, Some(factory), Vec::new(), None, to);
            }
        }
        if gates.is_empty() {
            return;
        }
        for i in 0..self.shops.len() {
            let shop = self.shops[i];
            if self.chance(IMPORTS_PER_SHOP * level) {
                let from = gate(externals.inbound(roads, self.pick(&gates)));
                send(vehicles, ctx, None, from, Some(shop), Vec::new());
            }
        }
    }

    pub fn on_lots_changed(&mut self, lots: &LotSystem, _out: &mut Outbox) {
        self.factories = ids_of_type(lots, LotType::Industrial);
        self.shops = ids_of_type(lots, LotType::Commercial);
    }

    /// `_rng.randf() < probability` (GDScript compares as doubles).
    fn chance(&mut self, probability: f64) -> bool {
        f64::from(self.rng.randf()) < probability
    }

    fn pick(&mut self, points: &[Vec2]) -> Vec2 {
        points[(self.rng.randi() % points.len() as i64) as usize]
    }
}

fn ids_of_type(lots: &LotSystem, lot_type: LotType) -> Vec<LotId> {
    lots.lots_of_type(lot_type)
        .iter()
        .map(|lot| lot.id)
        .collect()
}

/// Sends a truck from a lot's driveway (or, `None`, the lane positions `from`, e.g. a
/// highway) to a lot (or, `None`, the quickest of `to`); quietly skipped if there's no way
/// there.
fn send(
    vehicles: &mut VehicleSystem,
    ctx: &mut TrafficCtx,
    origin: Option<LotId>,
    from: Vec<LanePoint>,
    destination: Option<LotId>,
    to: Vec<LanePoint>,
) {
    vehicles.depart(ctx, origin, from, destination, to, OWNER_ID, Kind::Truck);
}

/// A highway's lane position as a list of access points (empty if it has no road).
fn gate(position: Option<LanePoint>) -> Vec<LanePoint> {
    position.into_iter().collect()
}
