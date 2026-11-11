//! `PedestrianSystem` (`sim/walking/pedestrian_system.gd`): people walking along
//! sidewalks door to door, crossing roads only at crosswalks (`WalkNetwork`). They don't
//! bump into each other; they meet cars only at crosswalks:
//! - at the curb a pedestrian waits until no car is in the junction on a path over the
//!   crosswalk (`VehicleSystem::is_crosswalk_busy`) and, at traffic lights, until the
//!   walk light (green for the traffic running alongside, `TrafficLights::is_walk_green`)
//!   is on;
//! - meanwhile cars give way to them: none enters a turn over a crosswalk with someone on
//!   it, or waiting at it with the right to go (`VehicleSystem::crosswalks_clear`).
//!
//! Waiting pedestrians also call the walk phase at traffic lights.
//!
//! The GDScript class holds the roads, lots, lights, vehicles and the shared
//! `Crosswalks`; here they come in a `WalkCtx` built by `Simulation::walking`. Pedestrians
//! who arrive move to `finished` (readable by the `PedestrianArrived` listeners and
//! tests) until the next tick starts.

use glam::Vec2;
use indexmap::IndexMap;

use crate::city::LotSystem;
use crate::events::{Outbox, SimEvent};
use crate::geom::GdVec2;
use crate::grid;
use crate::ids::{LotId, NodeId, PedestrianId, SegmentId};
use crate::roads::{ChangeSet, RoadNetwork};
use crate::traffic::{TrafficLights, VehicleSystem};
use crate::walking::crosswalks::{Crosswalks, NONE};
use crate::walking::pedestrian::Pedestrian;
use crate::walking::walk_network::WalkNetwork;

/// What pedestrians work with (the GDScript class holds references to these).
pub struct WalkCtx<'a> {
    pub roads: &'a RoadNetwork,
    pub lots: &'a LotSystem,
    pub signals: &'a TrafficLights,
    pub vehicles: &'a VehicleSystem,
    pub crosswalks: &'a mut Crosswalks,
}

#[derive(Debug)]
pub struct PedestrianSystem {
    pub pedestrians: IndexMap<PedestrianId, Pedestrian>,
    /// Pedestrians who arrived this tick, until `clear_finished`.
    pub finished: IndexMap<PedestrianId, Pedestrian>,
    pub network: WalkNetwork,
    next_id: i32,
    time: f64,
}

impl Default for PedestrianSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl PedestrianSystem {
    /// Walking pace (px per sim second): a sixth of a car on a local street, about the
    /// real ratio in town (5 km/h against 30).
    pub const WALK_SPEED: f64 = 15.0;
    /// Citizens walk trips up to this long (px) instead of driving.
    pub const MAX_WALK_DISTANCE: f64 = 4.0 * grid::CELL_SIZE as f64;
    /// Added to a walk's ideal time for setting off and arriving.
    pub const START_STOP_ALLOWANCE: f64 = 1.0;

    pub fn new() -> Self {
        Self {
            pedestrians: IndexMap::new(),
            finished: IndexMap::new(),
            network: WalkNetwork::new(),
            next_id: 1,
            time: 0.0,
        }
    }

    /// `RoadNetwork.changed` → its `WalkNetwork._on_roads_changed` (the walk network is
    /// created inside `PedestrianSystem._init`, so it listens in that place).
    pub fn on_roads_changed(&mut self, change: &ChangeSet, _out: &mut Outbox) {
        self.network.on_roads_changed(change);
    }

    /// A pedestrian on the map, or one who arrived this tick (event listeners).
    pub fn pedestrian_any(&self, id: PedestrianId) -> Option<&Pedestrian> {
        self.pedestrians.get(&id).or_else(|| self.finished.get(&id))
    }

    /// Walking distance between two lots, or infinity if there's no way on foot.
    pub fn get_walking_distance(
        &mut self,
        ctx: &WalkCtx,
        from_lot_id: LotId,
        to_lot_id: LotId,
    ) -> f64 {
        let (Some(from), Some(to)) = (
            ctx.lots.lots.get(&from_lot_id),
            ctx.lots.lots.get(&to_lot_id),
        ) else {
            return f64::INFINITY;
        };
        self.network
            .distance(ctx.roads, ctx.crosswalks, from.center(), to.center())
    }

    /// Starts walking from one lot to another. `None` if there's no way on foot, or it is
    /// longer than `max_distance`.
    pub fn walk(
        &mut self,
        ctx: &WalkCtx,
        from_lot_id: LotId,
        to_lot_id: LotId,
        owner_id: i32,
        max_distance: f64,
    ) -> Option<PedestrianId> {
        let (from, to) = (
            ctx.lots.lots.get(&from_lot_id)?,
            ctx.lots.lots.get(&to_lot_id)?,
        );
        let id = self.walk_between(ctx, from.center(), to.center(), owner_id, max_distance)?;
        self.pedestrians[&id].destination_lot_id = Some(to_lot_id);
        Some(id)
    }

    /// Starts walking between two places (e.g. a door and a bus stop). `None` if there's
    /// no way on foot, or it is longer than `max_distance`.
    pub fn walk_between(
        &mut self,
        ctx: &WalkCtx,
        from: Vec2,
        to: Vec2,
        owner_id: i32,
        max_distance: f64,
    ) -> Option<PedestrianId> {
        let path = self
            .network
            .find_path(ctx.roads, ctx.crosswalks, from, to)?;
        if path.length > max_distance {
            return None;
        }
        let mut pedestrian = Pedestrian::new(PedestrianId(self.next_id));
        self.next_id += 1;
        pedestrian.owner_id = owner_id;
        pedestrian.position = path.points[0];
        pedestrian.points = path.points;
        pedestrian.crossings = path.crossings;
        pedestrian.trip_start_time = self.time;
        pedestrian.ideal_trip_time = path.length / Self::WALK_SPEED + Self::START_STOP_ALLOWANCE;
        let id = pedestrian.id;
        self.pedestrians.insert(id, pedestrian);
        Some(id)
    }

    /// Makes a pedestrian who has just set off start from `point` instead (e.g. a bus
    /// door beside the curb), stepping across to where their walk began.
    pub fn start_from(&mut self, pedestrian_id: PedestrianId, point: Vec2) {
        let Some(pedestrian) = self.pedestrians.get_mut(&pedestrian_id) else {
            return;
        };
        if pedestrian.leg != 0 || pedestrian.leg_progress != 0.0 {
            return;
        }
        pedestrian.points.insert(0, point);
        pedestrian.crossings.insert(0, NONE);
        pedestrian.position = point;
    }

    /// Removes a pedestrian without reporting an arrival (e.g. their home is gone).
    pub fn remove(&mut self, pedestrian_id: PedestrianId, crosswalks: &mut Crosswalks) {
        if let Some(mut pedestrian) = self.pedestrians.shift_remove(&pedestrian_id) {
            Self::stop_crossing(&mut pedestrian, crosswalks);
        }
    }

    pub fn step(&mut self, time: f64, dt: f64, ctx: &mut WalkCtx, out: &mut Outbox) {
        self.time = time;
        let mut arrived: Vec<PedestrianId> = Vec::new();
        for pedestrian in self.pedestrians.values_mut() {
            if Self::walk_along(pedestrian, Self::WALK_SPEED * dt, ctx) {
                arrived.push(pedestrian.id);
            }
        }
        for id in arrived {
            let mut pedestrian = self.pedestrians.shift_remove(&id).expect("walking");
            pedestrian.trip_end_time = self.time;
            self.finished.insert(id, pedestrian);
            out.push(SimEvent::PedestrianArrived(id));
        }
    }

    /// Drops the pedestrians who arrived once their events have been delivered.
    pub fn clear_finished(&mut self) {
        self.finished.clear();
    }

    /// `_walk`: moves a pedestrian `distance` along its route, stopping at the curb of a
    /// crosswalk it may not cross yet. Returns true when it reached the end.
    fn walk_along(pedestrian: &mut Pedestrian, mut distance: f64, ctx: &mut WalkCtx) -> bool {
        while distance > 0.0 {
            if pedestrian.leg >= pedestrian.points.len() - 1 {
                return true;
            }
            let crossing = pedestrian.crossings[pedestrian.leg];
            if crossing != NONE && pedestrian.on_crosswalk == NONE {
                if !Self::may_cross(ctx, crossing) {
                    if pedestrian.waiting_at == NONE {
                        pedestrian.waiting_at = crossing;
                        ctx.crosswalks.add_waiting(crossing);
                    }
                    break;
                }
                if pedestrian.waiting_at != NONE {
                    ctx.crosswalks.remove_waiting(pedestrian.waiting_at);
                    pedestrian.waiting_at = NONE;
                }
                pedestrian.on_crosswalk = crossing;
                ctx.crosswalks.add_walker(crossing);
            }
            let a = pedestrian.points[pedestrian.leg];
            let b = pedestrian.points[pedestrian.leg + 1];
            let remaining = f64::from(a.gd_distance_to(b)) - pedestrian.leg_progress;
            if distance < remaining {
                pedestrian.leg_progress += distance;
                distance = 0.0;
            } else {
                distance -= remaining;
                if pedestrian.on_crosswalk != NONE {
                    ctx.crosswalks.remove_walker(pedestrian.on_crosswalk);
                    pedestrian.on_crosswalk = NONE;
                }
                pedestrian.leg += 1;
                pedestrian.leg_progress = 0.0;
            }
        }
        if pedestrian.leg >= pedestrian.points.len() - 1 {
            pedestrian.position = *pedestrian.points.last().expect("a route has points");
            return true;
        }
        let from = pedestrian.points[pedestrian.leg];
        pedestrian.position = from.gd_move_toward(
            pedestrian.points[pedestrian.leg + 1],
            pedestrian.leg_progress as f32,
        );
        false
    }

    /// May a pedestrian at the curb step onto `crossing` now?
    fn may_cross(ctx: &WalkCtx, crossing: glam::IVec2) -> bool {
        if Crosswalks::is_mid_block(crossing) {
            return !ctx
                .vehicles
                .is_crosswalk_busy(ctx.roads, ctx.crosswalks, crossing);
        }
        let Some(node) = ctx.roads.nodes.get(&NodeId(crossing.x)) else {
            return true;
        };
        if node.has_lights()
            && !ctx
                .signals
                .is_walk_green(ctx.roads, node, SegmentId(crossing.y))
        {
            return false;
        }
        !ctx.vehicles
            .is_crosswalk_busy(ctx.roads, ctx.crosswalks, crossing)
    }

    fn stop_crossing(pedestrian: &mut Pedestrian, crosswalks: &mut Crosswalks) {
        if pedestrian.on_crosswalk != NONE {
            crosswalks.remove_walker(pedestrian.on_crosswalk);
            pedestrian.on_crosswalk = NONE;
        }
        if pedestrian.waiting_at != NONE {
            crosswalks.remove_waiting(pedestrian.waiting_at);
            pedestrian.waiting_at = NONE;
        }
    }
}
