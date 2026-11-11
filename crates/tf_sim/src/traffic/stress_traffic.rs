//! `StressTraffic` (`sim/traffic/stress_traffic.gd`): debug load generator: keeps about
//! `target_count` vehicles on random trips between random positions on road lanes. Used
//! by the F6 debug key and the traffic benchmark.

use crate::events::Outbox;
use crate::ids::LaneId;
use crate::rng::SimRng;
use crate::roads::{ChangeSet, RoadNetwork};
use crate::routing::Router;
use crate::traffic::vehicle::Kind;
use crate::traffic::vehicle_system::VehicleSystem;

/// Owner id for stress vehicles (citizens use ids >= 1).
pub const OWNER_ID: i32 = -2;
/// At most this many new vehicles per tick, so traffic ramps up instead of popping in.
pub const MAX_SPAWNS_PER_TICK: usize = 20;
/// Random trips are re-drawn this many times per spawn attempt before giving up.
pub const ATTEMPTS_PER_SPAWN: usize = 3;

#[derive(Clone, Debug)]
pub struct StressTraffic {
    pub target_count: usize,
    pub count: usize,
    rng: SimRng,
    road_lanes: Vec<LaneId>,
    road_lanes_dirty: bool,
}

impl Default for StressTraffic {
    fn default() -> Self {
        Self::with_seed(0)
    }
}

impl StressTraffic {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_seed(seed: u64) -> Self {
        Self {
            target_count: 0,
            count: 0,
            rng: SimRng::new(seed),
            road_lanes: Vec::new(),
            road_lanes_dirty: true,
        }
    }

    pub fn step(&mut self, roads: &RoadNetwork, router: &mut Router, vehicles: &mut VehicleSystem) {
        if self.count >= self.target_count {
            return;
        }
        if self.road_lanes_dirty {
            self.road_lanes = roads
                .lanes
                .values()
                .filter(|lane| !lane.is_connector())
                .map(|lane| lane.id)
                .collect();
            self.road_lanes_dirty = false;
        }
        if self.road_lanes.len() < 2 {
            return;
        }
        for _ in 0..(self.target_count - self.count).min(MAX_SPAWNS_PER_TICK) {
            for _ in 0..ATTEMPTS_PER_SPAWN {
                if self.try_spawn(roads, router, vehicles) {
                    self.count += 1;
                    break;
                }
            }
        }
    }

    fn try_spawn(
        &mut self,
        roads: &RoadNetwork,
        router: &mut Router,
        vehicles: &mut VehicleSystem,
    ) -> bool {
        let size = self.road_lanes.len() as i64;
        let from = &roads.lanes[&self.road_lanes[(self.rng.randi() % size) as usize]];
        let to = &roads.lanes[&self.road_lanes[(self.rng.randi() % size) as usize]];
        let from_distance = f64::from(self.rng.randf()) * from.length;
        let to_distance = f64::from(self.rng.randf()) * to.length;
        let Some(route) =
            router.find_route(roads, from.id, from_distance, to.id, to_distance, false)
        else {
            return false;
        };
        vehicles
            .spawn(roads, route, None, OWNER_ID, Kind::Car)
            .is_some()
    }

    /// `vehicle_arrived` / `vehicle_failed` (`_on_vehicle_gone`), with the vehicle's owner.
    pub fn on_vehicle_gone(&mut self, owner_id: Option<i32>, _out: &mut Outbox) {
        if owner_id == Some(OWNER_ID) {
            self.count -= 1;
        }
    }

    pub fn on_roads_changed(&mut self, _change: &ChangeSet, _out: &mut Outbox) {
        self.road_lanes_dirty = true;
    }
}
