//! `Citizen` (`sim/city/citizen.gd`): a lightweight person record. Citizens are only
//! simulated (as a vehicle or a pedestrian) while travelling; the rest of the time they
//! are a state plus a personal daily schedule. A trip is a list of legs: walked, driven
//! or by bus (`CitizenSystem`).

use glam::Vec2;

use crate::ids::{CitizenId, LineId, LotId, PedestrianId, StopId, VehicleId};

/// Godot's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum State {
    AtHome,
    ToWork,
    AtWork,
    ToShop,
    AtShop,
    ToHome,
}

/// How a leg is travelled; a trip's mode is its main one (a car or bus leg in it).
/// Godot's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Mode {
    Walk = 0,
    Car = 1,
    Bus = 2,
}

impl Mode {
    /// `MODE_NAMES`.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Car => "Car",
            Mode::Bus => "Bus",
            Mode::Walk => "Walk",
        }
    }
}

/// One leg of a trip (the dictionaries in `Citizen.legs`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Leg {
    /// `{ "mode": WALK, "from": Vector2, "to": Vector2 }`.
    Walk { from: Vec2, to: Vec2 },
    /// `{ "mode": CAR, "from": lot id, "to": lot id }`.
    Car { from: LotId, to: LotId },
    /// `{ "mode": BUS, "line": line id, "from": stop id, "to": stop id }`.
    Bus {
        line: LineId,
        from: StopId,
        to: StopId,
    },
}

impl Leg {
    pub fn mode(&self) -> Mode {
        match self {
            Leg::Walk { .. } => Mode::Walk,
            Leg::Car { .. } => Mode::Car,
            Leg::Bus { .. } => Mode::Bus,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Citizen {
    pub id: CitizenId,
    pub home_lot_id: LotId,
    pub work_lot_id: Option<LotId>,
    pub state: State,
    /// The current leg's vehicle or pedestrian.
    pub vehicle_id: Option<VehicleId>,
    pub pedestrian_id: Option<PedestrianId>,
    /// Lot the current trip heads to.
    pub trip_lot_id: Option<LotId>,

    /// The citizen's car, if they own one: the lot it is parked at (or heading to, while
    /// it is driven). They can only drive from where it is.
    pub has_car: bool,
    pub car_lot_id: LotId,
    /// Personal taste for driving: multiplies the car's cost in mode choice (below 1
    /// likes it).
    pub car_preference: f64,

    /// The current trip: legs still to go (the next one first), its main mode, when it
    /// started, its ideal (free-flow) time summed over the legs started so far, and the
    /// yardstick Flow caps that ideal at (the drive between the two places on empty
    /// roads, `CitizenSystem`).
    pub legs: Vec<Leg>,
    pub trip_mode: Mode,
    pub trip_start_time: f64,
    pub trip_ideal_time: f64,
    pub trip_reference_time: f64,
    pub trip_end_time: f64,
    /// Waiting for or riding a bus (`TransitSystem`).
    pub on_bus: bool,

    /// Personal schedule, in hours of the day.
    pub leave_home_hour: f64,
    pub leave_work_hour: f64,
    /// Day on which the citizen last set off to work (they go once a day).
    pub last_commute_day: i32,
    /// Debug: leave home as soon as possible, ignoring the schedule (once).
    pub leave_now: bool,
    /// Evening shopping: decided once per day when leaving work.
    pub going_shopping: bool,
    pub shopping_decided_day: i32,
    /// Sim time until which the citizen stays at the shop.
    pub shop_until: f64,
    /// Sim time before which no new trip is attempted (after a failed start).
    pub retry_at: f64,
}

impl Citizen {
    pub fn new(id: CitizenId, home_lot_id: LotId) -> Self {
        Self {
            id,
            home_lot_id,
            work_lot_id: None,
            state: State::AtHome,
            vehicle_id: None,
            pedestrian_id: None,
            trip_lot_id: None,
            has_car: true,
            car_lot_id: home_lot_id,
            car_preference: 1.0,
            legs: Vec::new(),
            trip_mode: Mode::Car,
            trip_start_time: 0.0,
            trip_ideal_time: 0.0,
            trip_reference_time: f64::INFINITY,
            trip_end_time: 0.0,
            on_bus: false,
            leave_home_hour: 7.5,
            leave_work_hour: 17.5,
            last_commute_day: 0,
            leave_now: false,
            going_shopping: false,
            shopping_decided_day: 0,
            shop_until: 0.0,
            retry_at: 0.0,
        }
    }

    pub fn is_travelling(&self) -> bool {
        matches!(self.state, State::ToWork | State::ToShop | State::ToHome)
    }
}
