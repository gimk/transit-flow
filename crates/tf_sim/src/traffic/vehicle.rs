//! `Vehicle` (`sim/traffic/vehicle.gd`): a vehicle driving a route. Only `VehicleSystem`
//! creates, moves and removes vehicles. `distance` is the position of the vehicle's
//! center along its current lane.
//!
//! Godot holds lanes as objects (so a vehicle can finish a lane the network replaced);
//! here they are ids, looked up with `RoadNetwork::lane_any` (live or retired lanes).

use glam::Vec2;

use crate::ids::{LaneId, LotId, VehicleId};
use crate::roads::Lane;
use crate::routing::{LanePoint, Route};

pub const CAR_LENGTH: f64 = 11.0;
pub const CAR_WIDTH: f64 = 6.0;
pub const TRUCK_LENGTH: f64 = 17.0;
pub const TRUCK_WIDTH: f64 = 8.0;
/// A short city bus, the size of a truck (no longer: junction spacing allows for trucks).
pub const BUS_LENGTH: f64 = 17.0;
pub const BUS_WIDTH: f64 = 8.0;
/// The longest vehicle there is: assumed wherever a vehicle's length isn't known yet.
pub const MAX_LENGTH: f64 = TRUCK_LENGTH;

/// Godot's values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    #[default]
    Car = 0,
    Truck = 1,
    Bus = 2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Vehicle {
    pub id: VehicleId,
    pub kind: Kind,
    pub length: f64,
    pub width: f64,
    /// Who the trip belongs to (a citizen id, or a debug/traffic owner code; -1: none).
    pub owner_id: i32,
    /// Not scored by Flow on its own: one leg of an owner's longer trip, scored as a
    /// whole (citizens' cars), or a bus (its riders' trips are scored).
    pub scored_by_owner: bool,
    /// A bus in service (TransitSystem): at the end of each route (a stop) it stays put,
    /// `dwelling`, until it is given the route to its next stop
    /// (`VehicleSystem::set_route`). It never runs out of patience.
    pub in_service: bool,
    pub dwelling: bool,
    /// At a bus bay: dwelling off the lane (not in its queue, so traffic passes), and
    /// since when it has been waiting for a gap to pull back out (infinite: not yet sent
    /// on).
    pub in_bay: bool,
    pub leaving_bay_since: f64,
    /// Where the trip ends. If `destination_lot_id` is set, re-routing follows that
    /// lot's (possibly moved) access point; otherwise the fixed lane position below is
    /// used.
    pub destination_lot_id: Option<LotId>,
    pub destination_lane_id: Option<LaneId>,
    pub destination_distance: f64,

    /// The lane it is on (it may have been retired since: see the module doc).
    pub lane: Option<LaneId>,
    pub distance: f64,
    pub speed: f64,
    /// Lanes still to drive after the current one.
    pub next_lanes: Vec<LaneId>,
    /// Where to stop on the last lane of the route.
    pub end_distance: f64,

    /// Junction access: the connector this vehicle has been allowed into next (`None`:
    /// none), and the one it is asking for while it waits at the stop line, since when.
    pub granted_connector: Option<LaneId>,
    pub requested_connector: Option<LaneId>,
    pub waiting_since: f64,
    /// When it was let into its current or next junction (VehicleSystem's grant
    /// counter).
    pub entry_order: i64,

    /// Trip timing (sim seconds): when it set off, how long it would take with empty
    /// roads, and when it ended. Drives patience and the Flow score.
    pub trip_start_time: f64,
    pub ideal_trip_time: f64,
    pub trip_end_time: f64,
    /// Running late (shown in red); `gave_up` once patience ran out and the trip was
    /// abandoned.
    pub impatient: bool,
    pub gave_up: bool,

    /// Per-tick scratch filled by VehicleSystem before vehicles move: bumper-to-bumper
    /// gap to the vehicle ahead and its speed, and the distance to a stop line it may not
    /// cross.
    pub leader_gap: f64,
    pub leader_speed: f64,
    pub stop_gap: f64,
    /// After a lane change: how far the vehicle still is from its new lane's center (px,
    /// positive to the right). It steers across as it drives on (`VehicleSystem::slide`);
    /// drawing only. The slide started `slide_from` px out and is `slide_progress`
    /// (0..1) done.
    pub lateral_offset: f64,
    pub slide_from: f64,
    pub slide_progress: f64,
    /// Yaw from steering across (radians, positive to the right), added to the lane
    /// heading.
    pub slide_yaw: f64,
    /// Waiting to change lanes and running out of road: vehicles on the target lane make
    /// room.
    pub merging: bool,
    /// Driveways (`VehicleSystem::depart`). Before it pulls out, the vehicle waits off the
    /// road: `route` is the way it will take, `origin_lot_id` the lot it leaves (`None`:
    /// it appears on the road itself, e.g. trucks arriving by highway).
    pub route: Option<Route>,
    pub origin_lot_id: Option<LotId>,
    pub origin_points: Vec<LanePoint>,
    /// Lanes it must cross to get into or out of a driveway on the far side of the road,
    /// and where along each (one entry per lane); computed when needed.
    pub crossing_lanes: Vec<LaneId>,
    pub crossing_distances: Vec<f32>,
    /// Whether the arrival crossing was worked out for the current route.
    pub arrival_checked: bool,
    /// When it started waiting to pull out or to turn into its driveway (infinite: not
    /// waiting).
    pub driveway_wait_since: f64,
    /// How long the vehicle has been (nearly) stationary, in sim seconds.
    pub stopped_time: f64,
    /// How long it has been crawling well below the speed limit (held up), in sim
    /// seconds.
    pub slow_time: f64,

    /// Pose after the last two ticks, for interpolated rendering.
    pub position: Vec2,
    pub rotation: f64,
    pub previous_position: Vec2,
    pub previous_rotation: f64,
}

impl Default for Vehicle {
    fn default() -> Self {
        Self {
            id: VehicleId(0),
            kind: Kind::Car,
            length: CAR_LENGTH,
            width: CAR_WIDTH,
            owner_id: -1,
            scored_by_owner: false,
            in_service: false,
            dwelling: false,
            in_bay: false,
            leaving_bay_since: f64::INFINITY,
            destination_lot_id: None,
            destination_lane_id: None,
            destination_distance: 0.0,
            lane: None,
            distance: 0.0,
            speed: 0.0,
            next_lanes: Vec::new(),
            end_distance: 0.0,
            granted_connector: None,
            requested_connector: None,
            waiting_since: f64::INFINITY,
            entry_order: 0,
            trip_start_time: 0.0,
            ideal_trip_time: 0.0,
            trip_end_time: 0.0,
            impatient: false,
            gave_up: false,
            leader_gap: f64::INFINITY,
            leader_speed: 0.0,
            stop_gap: f64::INFINITY,
            lateral_offset: 0.0,
            slide_from: 0.0,
            slide_progress: 1.0,
            slide_yaw: 0.0,
            merging: false,
            route: None,
            origin_lot_id: None,
            origin_points: Vec::new(),
            crossing_lanes: Vec::new(),
            crossing_distances: Vec::new(),
            arrival_checked: false,
            driveway_wait_since: f64::INFINITY,
            stopped_time: 0.0,
            slow_time: 0.0,
            position: Vec2::ZERO,
            rotation: 0.0,
            previous_position: Vec2::ZERO,
            previous_rotation: 0.0,
        }
    }
}

impl Vehicle {
    pub fn set_kind(&mut self, kind: Kind) {
        self.kind = kind;
        (self.length, self.width) = match kind {
            Kind::Truck => (TRUCK_LENGTH, TRUCK_WIDTH),
            Kind::Bus => (BUS_LENGTH, BUS_WIDTH),
            Kind::Car => (CAR_LENGTH, CAR_WIDTH),
        };
    }

    pub fn is_on_last_lane(&self) -> bool {
        self.next_lanes.is_empty()
    }

    /// Distance left before the end of the current lane (`lane`, the one it is on), or
    /// before the stop on the last lane.
    pub fn remaining_on_lane(&self, lane: &Lane) -> f64 {
        (if self.is_on_last_lane() {
            self.end_distance
        } else {
            lane.length
        }) - self.distance
    }
}
