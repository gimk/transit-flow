//! `CitizenSystem` (`sim/city/citizen_system.gd`): the city's people. Each home houses
//! `RESIDENTS_PER_HOME` citizens; each looks for a job at the nearest workplace (shop or
//! factory) with a free one. Every citizen follows a personal daily schedule:
//! - morning rush: leave home between ~06:30 and 09:00 for work,
//! - evening rush: leave work between ~16:00 and 19:00, on some evenings via a shop
//!   (stay 30-60 game minutes), then go home.
//!
//! Trips (`plan_trip`). A trip is a list of legs: walked, driven or by bus. Most citizens
//! own a car (`CAR_OWNERSHIP`), parked where they last left it (`Citizen::car_lot_id`):
//! they can only drive from there, walking to it first if it's nearby. A driver parks at
//! their destination if it has a free space, else nearby and walks the rest (`Parking`).
//! Bus trips walk to a stop, ride (changing lines at shared stops) and walk on
//! (`TransitSystem::plan_ride`). Each trip goes the cheapest way by generalized cost: a
//! trip on foot costs its time; walking to and from the car or a stop counts
//! `WALK_WEIGHT` times; driving costs its expected time (current congestion included)
//! plus `CAR_OVERHEAD` for getting the car out and parking it, scaled by the citizen's
//! taste for driving; waiting for a bus counts double. Walks to or from the car or a
//! stop are at most `MAX_CHOSEN_WALK`. Someone heading home whose car isn't there drives
//! it home.
//!
//! Flow scores a trip against its legs' free-flow times, capped at the drive between the
//! two places on empty roads plus `CAR_ACCESS_TIME`: walking across town, or parking far
//! away, counts as slow even when nothing held it up.
//!
//! Trips that can't start (no road, no route) are retried shortly. A trip that fails on
//! the way (road gone, driver gave up) still ends at its destination if that still
//! exists, so the daily rhythm carries on; the Flow metric counts the failure.
//!
//! The GDScript class holds the other systems; here the calls that need them take a
//! `CitizenCtx` (`Simulation::citizens_ctx` splits the borrows). `trip_ended` is the
//! `SimEvent::TripEnded` the call's `Outbox` receives.

use std::collections::HashMap;

use glam::Vec2;
use indexmap::{IndexMap, IndexSet};

use super::citizen::{Citizen, Leg, Mode, State};
use super::lot::LotType;
use super::lot_system::LotSystem;
use super::parking::Parking;
use crate::clock::SimClock;
use crate::events::{Outbox, SimEvent};
use crate::geom::sort::sort_custom;
use crate::ids::{CitizenId, LotId, PedestrianId, VehicleId};
use crate::rng::SimRng;
use crate::roads::RoadNetwork;
use crate::routing::Router;
use crate::traffic::traffic_lights::TrafficLights;
use crate::traffic::vehicle::Kind;
use crate::traffic::vehicle_system::{START_STOP_ALLOWANCE, TrafficCtx, VehicleSystem};
use crate::transit::TransitSystem;
use crate::transit::transit_system::{RideLeg, TransitCtx};
use crate::walking::{Crosswalks, PedestrianSystem, WalkCtx};

pub const RESIDENTS_PER_HOME: i32 = 2;
/// Sim seconds.
pub const RETRY_DELAY: f64 = 2.0;
pub const SHOPPING_CHANCE: f64 = 0.35;
/// Shoppers pick one of this many shops nearest their work.
pub const SHOP_CHOICES: usize = 4;
pub const SHOP_MIN_MINUTES: f32 = 30.0;
pub const SHOP_MAX_MINUTES: f32 = 60.0;
/// Morning commutes only start before this hour; someone who missed it waits a day.
pub const MORNING_END_HOUR: f64 = 12.0;
/// Workers still at work after midnight (missed the evening) go home before this hour.
pub const LATE_NIGHT_END_HOUR: f64 = 5.0;
/// Job seekers matched to a workplace per tick (keeps big zoning sprees cheap).
pub const JOB_MATCHES_PER_TICK: usize = 10;
/// Mode choice (see the module docs). Costs are in sim seconds. Distances here are
/// compressed (a car crosses a cell in under a second), so the overhead of using a car
/// is large next to the drive itself: about the time it takes to walk two or three
/// cells. With it, the average citizen walks trips up to ~3 cells. Walks to and from the
/// car or a bus stop count `WALK_WEIGHT` times (a chore on the way); a trip on foot
/// counts as it is.
pub const WALK_WEIGHT: f64 = 1.5;
pub const CAR_OVERHEAD: f64 = 10.0;
/// Longest walk to or from the car or a bus stop.
pub const MAX_CHOSEN_WALK: f64 = PedestrianSystem::MAX_WALK_DISTANCE;
/// Spread of `Citizen::car_preference`.
pub const CAR_PREFERENCE_RANGE: (f32, f32) = (0.85, 1.2);
/// Share of citizens owning a car.
pub const CAR_OWNERSHIP: f64 = 0.8;
/// Time a car trip takes beyond the drive itself (to the car, parking), for Flow's
/// yardstick (sim seconds).
pub const CAR_ACCESS_TIME: f64 = 4.0;

const SEED: u64 = 11;

/// `JOBS`: jobs per workplace type.
pub fn jobs_at(lot_type: LotType) -> Option<i32> {
    match lot_type {
        LotType::Commercial => Some(2),
        LotType::Industrial => Some(3),
        LotType::Residential => None,
    }
}

/// What the citizens work with besides their own state (the GDScript class holds
/// references to these).
pub struct CitizenCtx<'a> {
    pub roads: &'a RoadNetwork,
    pub router: &'a mut Router,
    pub lots: &'a mut LotSystem,
    pub signals: &'a TrafficLights,
    pub vehicles: &'a mut VehicleSystem,
    pub pedestrians: &'a mut PedestrianSystem,
    pub transit: &'a mut TransitSystem,
    pub crosswalks: &'a mut Crosswalks,
    pub clock: &'a SimClock,
}

impl<'a> CitizenCtx<'a> {
    fn walking(&mut self) -> (&mut PedestrianSystem, WalkCtx<'_>) {
        let ctx = WalkCtx {
            roads: self.roads,
            lots: self.lots,
            signals: self.signals,
            vehicles: self.vehicles,
            crosswalks: self.crosswalks,
        };
        (self.pedestrians, ctx)
    }

    fn traffic(&mut self) -> (&mut VehicleSystem, TrafficCtx<'_>) {
        let ctx = TrafficCtx {
            roads: self.roads,
            router: self.router,
            lots: self.lots,
            signals: self.signals,
            crosswalks: self.crosswalks,
        };
        (self.vehicles, ctx)
    }

    fn transit(&mut self) -> (&mut TransitSystem, TransitCtx<'_>) {
        let ctx = TransitCtx {
            roads: self.roads,
            router: self.router,
            vehicles: self.vehicles,
            pedestrians: self.pedestrians,
            crosswalks: self.crosswalks,
        };
        (self.transit, ctx)
    }
}

/// What `plan_trip` returns: `{ "mode", "legs", "cost" }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub mode: Mode,
    pub legs: Vec<Leg>,
    /// Generalized cost.
    pub cost: f64,
}

/// A workplace or shop as `_sync_with_lots` saw it (lots never change type or place).
#[derive(Clone, Copy, Debug)]
struct Place {
    id: LotId,
    lot_type: LotType,
    center: Vec2,
}

#[derive(Clone, Debug)]
pub struct CitizenSystem {
    pub citizens: IndexMap<CitizenId, Citizen>,
    pub parking: Parking,
    rng: SimRng,
    next_id: i32,
    time: f64,
    /// Workplace lot id → jobs taken there.
    jobs_taken: IndexMap<LotId, i32>,
    /// Citizens without a job, waiting to be matched.
    job_seekers: IndexSet<CitizenId>,
    workplaces: Vec<Place>,
    shops: Vec<Place>,
}

impl Default for CitizenSystem {
    fn default() -> Self {
        Self {
            citizens: IndexMap::new(),
            parking: Parking::new(),
            rng: SimRng::new(SEED),
            next_id: 1,
            time: 0.0,
            jobs_taken: IndexMap::new(),
            job_seekers: IndexSet::new(),
            workplaces: Vec::new(),
            shops: Vec::new(),
        }
    }
}

impl CitizenSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&mut self, time: f64, ctx: &mut CitizenCtx, out: &mut Outbox) {
        self.time = time;
        self.match_job_seekers(ctx.lots);
        let hour = ctx.clock.time_of_day_hours();
        let day = ctx.clock.day();
        let ids: Vec<CitizenId> = self.citizens.keys().copied().collect();
        for id in ids {
            let citizen = &self.citizens[&id];
            if citizen.is_travelling() || time < citizen.retry_at {
                continue;
            }
            match citizen.state {
                State::AtHome => {
                    let Some(work) = citizen.work_lot_id else {
                        continue;
                    };
                    let morning = hour >= citizen.leave_home_hour && hour < MORNING_END_HOUR;
                    if citizen.leave_now || (morning && citizen.last_commute_day != day) {
                        let home = citizen.home_lot_id;
                        self.depart(ctx, id, home, work, State::ToWork, out);
                    }
                }
                State::AtWork => {
                    if citizen.leave_now
                        || hour >= citizen.leave_work_hour
                        || hour < LATE_NIGHT_END_HOUR
                    {
                        let work = citizen.work_lot_id.expect("someone at work has a job");
                        let home = citizen.home_lot_id;
                        if citizen.shopping_decided_day != day {
                            let going = f64::from(self.rng.randf()) < SHOPPING_CHANCE;
                            let citizen = &mut self.citizens[&id];
                            citizen.shopping_decided_day = day;
                            citizen.going_shopping = going;
                        }
                        let going_shopping = self.citizens[&id].going_shopping;
                        let shop = if going_shopping {
                            self.pick_shop(ctx.lots, work)
                        } else {
                            None
                        };
                        match shop {
                            Some(shop) if going_shopping => {
                                self.depart(ctx, id, work, shop, State::ToShop, out);
                            }
                            _ => self.depart(ctx, id, work, home, State::ToHome, out),
                        }
                    }
                }
                State::AtShop if time >= citizen.shop_until => {
                    let from = citizen.trip_lot_id.expect("someone at a shop went there");
                    let home = citizen.home_lot_id;
                    self.depart(ctx, id, from, home, State::ToHome, out);
                }
                _ => {}
            }
        }
    }

    /// `sim.citizens._rng.seed = seed`: tests reseed the citizens' draws to try other
    /// departure orders (before the citizens exist).
    pub fn set_seed(&mut self, seed: u64) {
        self.rng = SimRng::new(seed);
    }

    /// Debug: everyone at home leaves for work right away, ignoring the schedule (once
    /// they have a job), spread at random over the next `spread_seconds` sim seconds.
    pub fn depart_everyone_now(&mut self, spread_seconds: f64) {
        for citizen in self.citizens.values_mut() {
            if citizen.state == State::AtHome {
                citizen.leave_now = true;
                citizen.retry_at = self.time + f64::from(self.rng.randf()) * spread_seconds;
            }
        }
    }

    pub fn job_count(&self) -> i32 {
        self.workplaces
            .iter()
            .map(|place| jobs_at(place.lot_type).expect("workplaces have jobs"))
            .sum()
    }

    pub fn employed_count(&self) -> usize {
        self.citizens.len() - self.job_seekers.len()
    }

    /// The way `citizen` would go from one lot to another now, or `None` if there's none.
    /// (May strand riders of bus stops that lost their road, so it takes `out`.)
    pub fn plan_trip(
        &mut self,
        ctx: &mut CitizenCtx,
        citizen: CitizenId,
        from_lot: LotId,
        to_lot: LotId,
        going_home: bool,
        out: &mut Outbox,
    ) -> Option<Plan> {
        let from = ctx.lots.lots[&from_lot].center();
        let to = ctx.lots.lots[&to_lot].center();
        let mut walk = None;
        let walk_length = {
            let (pedestrians, walk_ctx) = ctx.walking();
            pedestrians
                .network
                .distance(walk_ctx.roads, walk_ctx.crosswalks, from, to)
        };
        if walk_length < f64::INFINITY {
            walk = Some(Plan {
                mode: Mode::Walk,
                legs: vec![Leg::Walk { from, to }],
                cost: walk_time(walk_length),
            });
        }
        let car = self.plan_drive(ctx, citizen, from_lot, to_lot);
        // The car goes back home with them.
        if going_home && car.is_some() {
            let c = &self.citizens[&citizen];
            if c.car_lot_id != c.home_lot_id {
                return car;
            }
        }
        let bus = {
            let (transit, mut tctx) = ctx.transit();
            transit
                .plan_ride(&mut tctx, from, to, MAX_CHOSEN_WALK, WALK_WEIGHT, out)
                .map(|plan| Plan {
                    mode: Mode::Bus,
                    legs: plan.legs.iter().map(ride_leg).collect(),
                    cost: plan.cost,
                })
        };
        let mut best: Option<Plan> = None;
        for option in [car, bus, walk].into_iter().flatten() {
            if best.as_ref().is_none_or(|b| option.cost < b.cost) {
                best = Some(option);
            }
        }
        best
    }

    fn depart(
        &mut self,
        ctx: &mut CitizenCtx,
        id: CitizenId,
        from_lot: LotId,
        to_lot: LotId,
        travelling: State,
        out: &mut Outbox,
    ) {
        if from_lot == to_lot {
            // Already there.
            self.set_off(ctx, id, to_lot, travelling);
            self.arrive(id);
            return;
        }
        let Some(plan) =
            self.plan_trip(ctx, id, from_lot, to_lot, travelling == State::ToHome, out)
        else {
            // No way there (yet): try again shortly.
            self.citizens[&id].retry_at = self.time + RETRY_DELAY;
            return;
        };
        let reference = self.drive_reference(ctx, from_lot, to_lot);
        let citizen = self.citizens.get_mut(&id).expect("a citizen");
        citizen.legs = plan.legs;
        citizen.trip_mode = plan.mode;
        citizen.trip_start_time = self.time;
        citizen.trip_ideal_time = 0.0;
        citizen.trip_reference_time = reference;
        for leg in &citizen.legs {
            if let Leg::Car { to, .. } = leg {
                // The space at the other end is theirs from now on (the car moves when
                // driven).
                self.parking.take(ctx.lots, *to);
            }
        }
        self.set_off(ctx, id, to_lot, travelling);
        self.next_leg(ctx, id, out);
    }

    /// A drive from wherever the car is (walking there first) to a parking space at or
    /// near `to_lot` (walking on from it), as a plan (see `plan_trip`), or `None` if the
    /// citizen has no car within reach or it can't get there.
    pub fn plan_drive(
        &mut self,
        ctx: &mut CitizenCtx,
        id: CitizenId,
        from_lot: LotId,
        to_lot: LotId,
    ) -> Option<Plan> {
        let citizen = &self.citizens[&id];
        if !citizen.has_car || !ctx.lots.lots.contains_key(&citizen.car_lot_id) {
            return None;
        }
        let car_at = citizen.car_lot_id;
        let home = citizen.home_lot_id;
        let preference = citizen.car_preference;
        let park_at = if to_lot == home {
            to_lot
        } else {
            let (pedestrians, walk_ctx) = ctx.walking();
            self.parking
                .find_space(walk_ctx.lots, to_lot, MAX_CHOSEN_WALK, |a, b| {
                    pedestrians.get_walking_distance(&walk_ctx, a, b)
                })
        };
        if park_at == car_at {
            return None; // Nothing to drive.
        }
        let mut legs = Vec::new();
        let mut cost = 0.0;
        if car_at != from_lot {
            let to_car = {
                let (pedestrians, walk_ctx) = ctx.walking();
                pedestrians.get_walking_distance(&walk_ctx, from_lot, car_at)
            };
            if to_car > MAX_CHOSEN_WALK {
                return None;
            }
            legs.push(Leg::Walk {
                from: ctx.lots.lots[&from_lot].center(),
                to: ctx.lots.lots[&car_at].center(),
            });
            cost += walk_cost(to_car);
        }
        let starts = ctx.lots.access_points(ctx.roads, car_at);
        let ends = ctx.lots.access_points(ctx.roads, park_at);
        let route = ctx
            .router
            .find_best_route(ctx.roads, &starts, &ends, false)?;
        legs.push(Leg::Car {
            from: car_at,
            to: park_at,
        });
        cost += (route.travel_time + CAR_OVERHEAD) * preference;
        if park_at != to_lot {
            legs.push(Leg::Walk {
                from: ctx.lots.lots[&park_at].center(),
                to: ctx.lots.lots[&to_lot].center(),
            });
            let (pedestrians, walk_ctx) = ctx.walking();
            cost += walk_cost(pedestrians.get_walking_distance(&walk_ctx, park_at, to_lot));
        }
        Some(Plan {
            mode: Mode::Car,
            legs,
            cost,
        })
    }

    /// Flow's yardstick for a trip between two lots: the drive on empty roads, plus the
    /// time a car trip takes beyond it (infinity if there's no road between them).
    fn drive_reference(&mut self, ctx: &mut CitizenCtx, from_lot: LotId, to_lot: LotId) -> f64 {
        let starts = ctx.lots.access_points(ctx.roads, from_lot);
        let ends = ctx.lots.access_points(ctx.roads, to_lot);
        let Some(route) = ctx.router.find_best_route(ctx.roads, &starts, &ends, false) else {
            return f64::INFINITY;
        };
        ctx.router.free_flow_time(ctx.roads, &route) + START_STOP_ALLOWANCE + CAR_ACCESS_TIME
    }

    fn set_off(&mut self, ctx: &CitizenCtx, id: CitizenId, to_lot: LotId, travelling: State) {
        let citizen = self.citizens.get_mut(&id).expect("a citizen");
        citizen.trip_lot_id = Some(to_lot);
        citizen.state = travelling;
        citizen.leave_now = false;
        if travelling == State::ToWork {
            citizen.last_commute_day = ctx.clock.day();
        }
    }

    /// Starts the trip's next leg, or ends the trip if there's none left.
    fn next_leg(&mut self, ctx: &mut CitizenCtx, id: CitizenId, out: &mut Outbox) {
        let citizen = self.citizens.get_mut(&id).expect("a citizen");
        citizen.vehicle_id = None;
        citizen.pedestrian_id = None;
        citizen.on_bus = false;
        let destination_exists = citizen
            .trip_lot_id
            .is_some_and(|lot| ctx.lots.lots.contains_key(&lot));
        if !destination_exists {
            // The destination went while they were on the way.
            self.finish_trip(ctx, id, false, out);
            return;
        }
        if citizen.legs.is_empty() {
            self.finish_trip(ctx, id, true, out);
            return;
        }
        let leg = citizen.legs.remove(0);
        match leg {
            Leg::Walk { from, to } => {
                let (pedestrians, walk_ctx) = ctx.walking();
                let started = pedestrians.walk_between(&walk_ctx, from, to, id.0, f64::INFINITY);
                if let Some(pedestrian) = started {
                    citizen.pedestrian_id = Some(pedestrian);
                    citizen.trip_ideal_time += pedestrians.pedestrians[&pedestrian].ideal_trip_time;
                    return;
                }
            }
            Leg::Car { from, to } => {
                // Gets in the car in its driveway; it pulls out when there is a gap
                // (`VehicleSystem::depart`).
                let (vehicles, mut tctx) = ctx.traffic();
                let started = vehicles.depart(
                    &mut tctx,
                    Some(from),
                    Vec::new(),
                    Some(to),
                    Vec::new(),
                    id.0,
                    Kind::Car,
                );
                if let Some(vehicle_id) = started {
                    let vehicle = vehicles
                        .vehicle_or_departure_mut(vehicle_id)
                        .expect("the vehicle just set off");
                    vehicle.scored_by_owner = true;
                    citizen.vehicle_id = Some(vehicle_id);
                    citizen.trip_ideal_time += vehicle.ideal_trip_time;
                    // The car leaves its space for the one taken at the other end.
                    self.parking.release(citizen.car_lot_id);
                    citizen.car_lot_id = to;
                    return;
                }
            }
            Leg::Bus { line, from, to } => {
                if ctx.transit.wait_for_bus(id, line, from, to) {
                    citizen.on_bus = true;
                    citizen.trip_ideal_time += ctx.transit.ride_ideal(line, from, to);
                    return;
                }
            }
        }
        // The way vanished since the trip was planned (a road, lot or bus line changed).
        if let Leg::Car { to, .. } = leg {
            self.parking.release(to);
        }
        self.finish_trip(ctx, id, false, out);
    }

    /// Reports the trip and puts the citizen at its destination (also after a failed
    /// trip, if the destination still exists, so the day carries on).
    fn finish_trip(
        &mut self,
        ctx: &mut CitizenCtx,
        id: CitizenId,
        completed: bool,
        out: &mut Outbox,
    ) {
        let citizen = self.citizens.get_mut(&id).expect("a citizen");
        for leg in &citizen.legs {
            if let Leg::Car { to, .. } = leg {
                self.parking.release(*to); // A space taken for a drive that won't happen.
            }
        }
        citizen.legs.clear();
        citizen.on_bus = false;
        citizen.trip_end_time = self.time;
        citizen.trip_ideal_time = citizen.trip_ideal_time.min(citizen.trip_reference_time);
        out.push(SimEvent::TripEnded {
            citizen: id,
            completed,
        });
        let destination_exists = citizen
            .trip_lot_id
            .is_some_and(|lot| ctx.lots.lots.contains_key(&lot));
        if completed || destination_exists {
            self.arrive(id);
        } else {
            let citizen = self.citizens.get_mut(&id).expect("a citizen");
            citizen.vehicle_id = None;
            citizen.pedestrian_id = None;
            citizen.state = State::AtHome;
            citizen.retry_at = self.time + RETRY_DELAY;
        }
    }

    pub fn on_vehicle_arrived(
        &mut self,
        vehicle: VehicleId,
        ctx: &mut CitizenCtx,
        out: &mut Outbox,
    ) {
        let Some(owner) = ctx.vehicles.vehicle_any(vehicle).map(|v| v.owner_id) else {
            return;
        };
        let id = CitizenId(owner);
        if self
            .citizens
            .get(&id)
            .is_some_and(|c| c.vehicle_id == Some(vehicle))
        {
            self.next_leg(ctx, id, out);
        }
    }

    pub fn on_pedestrian_arrived(
        &mut self,
        pedestrian: PedestrianId,
        ctx: &mut CitizenCtx,
        out: &mut Outbox,
    ) {
        let Some(owner) = ctx
            .pedestrians
            .pedestrian_any(pedestrian)
            .map(|p| p.owner_id)
        else {
            return;
        };
        let id = CitizenId(owner);
        if self
            .citizens
            .get(&id)
            .is_some_and(|c| c.pedestrian_id == Some(pedestrian))
        {
            self.next_leg(ctx, id, out);
        }
    }

    pub fn on_rider_arrived(
        &mut self,
        citizen: CitizenId,
        door: Vec2,
        ctx: &mut CitizenCtx,
        out: &mut Outbox,
    ) {
        if self.citizens.get(&citizen).is_some_and(|c| c.on_bus) {
            self.next_leg(ctx, citizen, out);
            // Walking on: from the bus door, across to the sidewalk.
            if let Some(pedestrian) = self.citizens[&citizen].pedestrian_id {
                ctx.pedestrians.start_from(pedestrian, door);
            }
        }
    }

    /// Their stop or line went, or no bus came: the trip fails.
    pub fn on_rider_stranded(
        &mut self,
        citizen: CitizenId,
        ctx: &mut CitizenCtx,
        out: &mut Outbox,
    ) {
        if self.citizens.get(&citizen).is_some_and(|c| c.on_bus) {
            self.finish_trip(ctx, citizen, false, out);
        }
    }

    /// The car ends up where it was going to park anyway (`Citizen::car_lot_id`).
    pub fn on_vehicle_failed(
        &mut self,
        vehicle: VehicleId,
        ctx: &mut CitizenCtx,
        out: &mut Outbox,
    ) {
        let Some(owner) = ctx.vehicles.vehicle_any(vehicle).map(|v| v.owner_id) else {
            return;
        };
        let id = CitizenId(owner);
        if self
            .citizens
            .get(&id)
            .is_some_and(|c| c.vehicle_id == Some(vehicle))
        {
            self.citizens[&id].vehicle_id = None;
            self.finish_trip(ctx, id, false, out);
        }
    }

    fn arrive(&mut self, id: CitizenId) {
        let citizen = self.citizens.get_mut(&id).expect("a citizen");
        citizen.vehicle_id = None;
        citizen.pedestrian_id = None;
        match citizen.state {
            State::ToWork => citizen.state = State::AtWork,
            State::ToShop => {
                citizen.state = State::AtShop;
                let minutes = f64::from(self.rng.randf_range(SHOP_MIN_MINUTES, SHOP_MAX_MINUTES));
                citizen.shop_until = self.time + minutes / SimClock::GAME_MINUTES_PER_SIM_SECOND;
            }
            _ => citizen.state = State::AtHome,
        }
    }

    /// `LotSystem.changed` → `_sync_with_lots`: keeps `RESIDENTS_PER_HOME` citizens per
    /// home, and drops people and jobs whose lots are gone.
    pub fn sync_with_lots(&mut self, ctx: &mut CitizenCtx, out: &mut Outbox) {
        self.workplaces.clear();
        self.shops.clear();
        for lot in ctx.lots.lots.values() {
            let place = Place {
                id: lot.id,
                lot_type: lot.lot_type,
                center: lot.center(),
            };
            if jobs_at(lot.lot_type).is_some() {
                self.workplaces.push(place);
            }
            if lot.lot_type == LotType::Commercial {
                self.shops.push(place);
            }
        }
        self.parking.sync(ctx.lots);

        let mut residents: HashMap<LotId, i32> = HashMap::new();
        let ids: Vec<CitizenId> = self.citizens.keys().copied().collect();
        for id in ids {
            let citizen = &self.citizens[&id];
            if !ctx.lots.lots.contains_key(&citizen.home_lot_id) {
                if let Some(vehicle) = citizen.vehicle_id {
                    ctx.vehicles.remove(ctx.roads, vehicle);
                }
                if citizen.on_bus {
                    ctx.transit.remove_rider(id);
                }
                if let Some(pedestrian) = citizen.pedestrian_id {
                    ctx.pedestrians.remove(pedestrian, ctx.crosswalks);
                }
                self.parking.release(citizen.car_lot_id);
                self.leave_job(id);
                self.job_seekers.shift_remove(&id);
                self.citizens.shift_remove(&id);
                continue;
            }
            *residents.entry(citizen.home_lot_id).or_insert(0) += 1;
            let citizen = self.citizens.get_mut(&id).expect("a citizen");
            if !ctx.lots.lots.contains_key(&citizen.car_lot_id) {
                // Its car park is gone: the car is towed home.
                citizen.car_lot_id = citizen.home_lot_id;
            }
            let trip_lot_exists = citizen
                .trip_lot_id
                .is_some_and(|lot| ctx.lots.lots.contains_key(&lot));
            if citizen.state == State::AtShop && !trip_lot_exists {
                citizen.state = State::AtHome;
            }
            if citizen.is_travelling() && !trip_lot_exists {
                // Heading for a place that's gone: they give up and go home. (A car on
                // the way is turned back by VehicleSystem, which fails it; walkers and
                // riders here.)
                if let Some(pedestrian) = citizen.pedestrian_id {
                    ctx.pedestrians.remove(pedestrian, ctx.crosswalks);
                    self.finish_trip(ctx, id, false, out);
                } else if citizen.on_bus {
                    ctx.transit.remove_rider(id);
                    self.finish_trip(ctx, id, false, out);
                }
            }
            let citizen = self.citizens.get_mut(&id).expect("a citizen");
            if let Some(work) = citizen.work_lot_id
                && !ctx.lots.lots.contains_key(&work)
            {
                self.jobs_taken.shift_remove(&work);
                citizen.work_lot_id = None;
                self.job_seekers.insert(id);
                if citizen.state == State::AtWork {
                    citizen.state = State::AtHome;
                }
            }
        }
        let homes: Vec<LotId> = ctx
            .lots
            .lots_of_type(LotType::Residential)
            .iter()
            .map(|lot| lot.id)
            .collect();
        for home in homes {
            for _ in 0..RESIDENTS_PER_HOME - residents.get(&home).copied().unwrap_or(0) {
                self.add_citizen(home);
            }
        }
    }

    fn add_citizen(&mut self, home_lot_id: LotId) {
        let mut citizen = Citizen::new(CitizenId(self.next_id), home_lot_id);
        self.next_id += 1;
        // A bell-ish spread (average of two uniform draws) around the rush-hour peaks.
        citizen.leave_home_hour =
            6.5 + (f64::from(self.rng.randf()) + f64::from(self.rng.randf())) * 0.5 * 2.5;
        citizen.leave_work_hour =
            16.0 + (f64::from(self.rng.randf()) + f64::from(self.rng.randf())) * 0.5 * 3.0;
        citizen.car_preference = f64::from(
            self.rng
                .randf_range(CAR_PREFERENCE_RANGE.0, CAR_PREFERENCE_RANGE.1),
        );
        citizen.has_car = f64::from(self.rng.randf()) < CAR_OWNERSHIP;
        self.job_seekers.insert(citizen.id);
        self.citizens.insert(citizen.id, citizen);
    }

    fn match_job_seekers(&mut self, lots: &LotSystem) {
        if self.job_seekers.is_empty() || self.workplaces.is_empty() {
            return;
        }
        let seekers: Vec<CitizenId> = self.job_seekers.iter().copied().collect();
        // Each seeker either gets a job (one more match) or ends the loop.
        for (matched, id) in seekers.into_iter().enumerate() {
            if matched >= JOB_MATCHES_PER_TICK {
                return;
            }
            let home = self.citizens[&id].home_lot_id;
            let position = lots.lots[&home].center();
            let Some(work) = self.nearest(&self.workplaces, position, true) else {
                return; // No free job anywhere.
            };
            self.citizens[&id].work_lot_id = Some(work);
            *self.jobs_taken.entry(work).or_insert(0) += 1;
            self.job_seekers.shift_remove(&id);
        }
    }

    fn leave_job(&mut self, id: CitizenId) {
        let citizen = self.citizens.get_mut(&id).expect("a citizen");
        if let Some(work) = citizen.work_lot_id
            && let Some(taken) = self.jobs_taken.get_mut(&work)
        {
            *taken -= 1;
        }
        citizen.work_lot_id = None;
    }

    /// Id of the lot in `candidates` closest to `position` (with a free job if
    /// `needs_free_job`), or `None`.
    fn nearest(&self, candidates: &[Place], position: Vec2, needs_free_job: bool) -> Option<LotId> {
        let mut best = None;
        let mut best_distance = f32::INFINITY;
        for lot in candidates {
            if needs_free_job
                && self.jobs_taken.get(&lot.id).copied().unwrap_or(0)
                    >= jobs_at(lot.lot_type).expect("workplaces have jobs")
            {
                continue;
            }
            let distance = position.distance_squared(lot.center);
            if f64::from(distance) < f64::from(best_distance) {
                best_distance = distance;
                best = Some(lot.id);
            }
        }
        best
    }

    /// A shop to stop at on the way home from `work_lot_id`: one of the `SHOP_CHOICES`
    /// nearest, at random, so the nearest one doesn't get everybody (never the shop they
    /// work at).
    pub fn pick_shop(&mut self, lots: &LotSystem, work_lot_id: LotId) -> Option<LotId> {
        let position = lots.lots[&work_lot_id].center();
        let mut by_distance: Vec<Place> = self
            .shops
            .iter()
            .filter(|shop| shop.id != work_lot_id)
            .copied()
            .collect();
        if by_distance.is_empty() {
            return None;
        }
        sort_custom(&mut by_distance, |a, b| {
            position.distance_squared(a.center) < position.distance_squared(b.center)
        });
        let choices = SHOP_CHOICES.min(by_distance.len());
        Some(by_distance[(self.rng.randi() % choices as i64) as usize].id)
    }
}

fn ride_leg(leg: &RideLeg) -> Leg {
    match *leg {
        RideLeg::Walk { from, to } => Leg::Walk { from, to },
        RideLeg::Bus { line, from, to } => Leg::Bus { line, from, to },
    }
}

fn walk_time(length: f64) -> f64 {
    length / PedestrianSystem::WALK_SPEED + PedestrianSystem::START_STOP_ALLOWANCE
}

/// Cost of walking to or from the car or a stop.
fn walk_cost(length: f64) -> f64 {
    walk_time(length) * WALK_WEIGHT
}
