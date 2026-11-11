//! `CongestionMonitor` (`sim/traffic/congestion_monitor.gd`): every UPDATE_INTERVAL,
//! measures how much slower than its speed limit traffic is moving on each road lane,
//! feeds that into routing costs (`Router::congestion`), and lets vehicles held up in
//! slow traffic look for a better route (a few per tick, so the route searches don't all
//! land on one frame). New trips pick up the new costs automatically, so a jam pushes
//! traffic onto alternatives.

use std::collections::VecDeque;

use indexmap::IndexMap;

use crate::events::Outbox;
use crate::ids::{LaneId, VehicleId};
use crate::traffic::vehicle_system::{TrafficCtx, VehicleSystem};

/// Sim seconds.
pub const UPDATE_INTERVAL: f64 = 5.0;
/// Weight of the newest measurement against the previous value (smooths flickering).
pub const SMOOTHING: f64 = 0.5;
/// A lane's travel time may be at most this many times its free-flow time.
pub const MAX_PENALTY: f64 = 10.0;
/// Values below this are dropped (the lane counts as free-flowing).
pub const MIN_PENALTY: f64 = 0.05;
/// Average speeds are floored at this, so a fully stopped queue maps to MAX_PENALTY-ish.
pub const MIN_MEASURED_SPEED: f64 = 5.0;
pub const REROUTES_PER_UPDATE: usize = 50;
pub const REROUTES_PER_TICK: usize = 3;

#[derive(Clone, Debug)]
pub struct CongestionMonitor {
    /// Lane id → extra travel time as a fraction of free-flow time (0 = free flow).
    /// Copied into the router's costs at every update.
    pub congestion: IndexMap<LaneId, f64>,
    next_update: f64,
    pending_reroutes: VecDeque<VehicleId>,
}

impl Default for CongestionMonitor {
    fn default() -> Self {
        Self {
            congestion: IndexMap::new(),
            next_update: UPDATE_INTERVAL,
            pending_reroutes: VecDeque::new(),
        }
    }
}

impl CongestionMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(
        &mut self,
        time: f64,
        vehicles: &mut VehicleSystem,
        ctx: &mut TrafficCtx,
        out: &mut Outbox,
    ) {
        for _ in 0..REROUTES_PER_TICK.min(self.pending_reroutes.len()) {
            let id = self.pending_reroutes.pop_front().expect("counted");
            vehicles.reroute_delayed_vehicle(ctx, id, out);
        }
        if time < self.next_update {
            return;
        }
        self.next_update = time + UPDATE_INTERVAL;
        self.update(vehicles, ctx);
    }

    /// Measures now (normally called by `step` on its interval; public for tests and
    /// tools).
    pub fn update(&mut self, vehicles: &VehicleSystem, ctx: &mut TrafficCtx) {
        let mut measured: IndexMap<LaneId, f64> = IndexMap::new();
        for lane_id in vehicles.occupied_lanes() {
            let lane = ctx
                .roads
                .lane_any(lane_id)
                .expect("an occupied lane exists");
            if lane.is_connector() {
                continue;
            }
            let queue = vehicles.queue(lane_id);
            let total_speed: f64 = queue.iter().map(|id| vehicles.vehicles[id].speed).sum();
            let average = (total_speed / queue.len() as f64).max(MIN_MEASURED_SPEED);
            measured.insert(
                lane_id,
                (lane.speed_limit / average - 1.0).clamp(0.0, MAX_PENALTY),
            );
        }

        let mut updated: IndexMap<LaneId, f64> = IndexMap::new();
        for (&lane_id, &value) in &measured {
            let previous = self.congestion.get(&lane_id).copied().unwrap_or(0.0);
            updated.insert(lane_id, previous + (value - previous) * SMOOTHING);
        }
        for (&lane_id, &value) in &self.congestion {
            if !measured.contains_key(&lane_id) && ctx.roads.lanes.contains_key(&lane_id) {
                updated.insert(lane_id, value * (1.0 - SMOOTHING));
            }
        }
        updated.retain(|_, value| *value >= MIN_PENALTY);

        self.congestion = updated;
        ctx.router.congestion = self.congestion.clone();
        ctx.router.invalidate_cache();
        self.pending_reroutes = vehicles.find_delayed(REROUTES_PER_UPDATE, 3.0).into();
    }
}
