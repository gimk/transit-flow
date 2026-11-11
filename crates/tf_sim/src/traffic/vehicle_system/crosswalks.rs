//! Cars and pedestrians: giving way at junction crosswalks (`crosswalks_clear`,
//! `is_crosswalk_busy`) and at mid-block crosswalks by bus stops (the "Mid-block
//! crosswalks" section of `vehicle_system.gd`).

use glam::IVec2;

use super::*;
use crate::ids::SegmentId;

impl VehicleSystem {
    /// Drivers stop short of a mid-block crosswalk with someone on it or waiting at it, if
    /// they comfortably can (those too close drive over first: `mid_block_busy` keeps the
    /// people at the curb meanwhile). The first driver held up by people waiting, once it
    /// has stood there CROSSWALK_PATIENCE, goes when the crosswalk is empty.
    pub(super) fn give_way_at_mid_block(&mut self, ctx: &mut TrafficCtx) {
        let crosswalks = ctx.crosswalks;
        if crosswalks.mid_block.is_empty() || crosswalks.is_empty() {
            return;
        }
        for (key, crossing) in &crosswalks.mid_block {
            let walkers = crosswalks.has_walkers(*key);
            if !walkers && !crosswalks.has_waiting(*key) {
                continue;
            }
            for &(lane_id, distance) in &crossing.lanes {
                if !ctx.roads.lanes.contains_key(&lane_id) {
                    continue;
                }
                let line = distance - MID_BLOCK_HALF_WIDTH;
                let Some(first) = self.first_short_of(lane_id, line) else {
                    continue;
                };
                if !walkers && self.vehicles[&first].stopped_time >= CROSSWALK_PATIENCE {
                    continue;
                }
                self.hold_back(lane_id, line);
            }
        }
    }

    /// Must people at the curb of mid-block crosswalk `crosswalk` wait? Yes while a
    /// vehicle is on it, or too close to stop for it, or has stood at it
    /// CROSSWALK_PATIENCE (it goes).
    fn mid_block_busy(
        &self,
        roads: &RoadNetwork,
        crosswalks: &Crosswalks,
        crosswalk: IVec2,
    ) -> bool {
        let Some(crossing) = crosswalks.mid_block.get(&crosswalk) else {
            return false;
        };
        for &(lane_id, distance) in &crossing.lanes {
            if !roads.lanes.contains_key(&lane_id) {
                continue;
            }
            let line = distance - MID_BLOCK_HALF_WIDTH;
            let far = distance + MID_BLOCK_HALF_WIDTH;
            for vehicle_id in self.queue(lane_id) {
                let vehicle = &self.vehicles[vehicle_id];
                if vehicle.distance - vehicle.length * 0.5 > far {
                    continue; // Past it.
                }
                let to_line = line - vehicle.distance - vehicle.length * 0.5;
                if to_line < 0.0 {
                    return true; // On it (standing in a queue, maybe).
                }
                // (Drivers stop for people only when they comfortably can, MIN_GAP short:
                // hold_back.)
                if vehicle.speed > STOPPED_SPEED
                    && to_line
                        <= vehicle.speed * vehicle.speed / (2.0 * DECELERATION) + MIN_GAP + 2.0
                {
                    return true;
                }
                if vehicle.stopped_time >= CROSSWALK_PATIENCE && to_line <= MIN_GAP * 2.0 {
                    return true;
                }
                break; // The vehicles behind are further away.
            }
        }
        false
    }

    /// The vehicle on `lane` nearest to `line` without having reached it.
    fn first_short_of(&self, lane_id: LaneId, line: f64) -> Option<VehicleId> {
        self.queue(lane_id).iter().copied().find(|id| {
            let vehicle = &self.vehicles[id];
            vehicle.distance + vehicle.length * 0.5 <= line
        })
    }

    /// Pedestrians first: a turn passes over the crosswalks of the road it comes from and
    /// the road it goes into; it may not be entered while someone is on one of them, or
    /// waiting at it with the right to go (with the walk light at lights). Without lights,
    /// a driver who has waited CROSSWALK_PATIENCE goes once the crosswalk is empty: the
    /// people at the curb let them (otherwise a steady stream of walkers would hold them up
    /// for ever).
    pub(super) fn crosswalks_clear(
        &self,
        ctx: &TrafficCtx,
        id: VehicleId,
        connector_id: LaneId,
    ) -> bool {
        let crosswalks = ctx.crosswalks;
        if crosswalks.is_empty() {
            return true;
        }
        let connector = lane(ctx.roads, connector_id);
        let Some(node) = ctx.roads.nodes.get(&connector.from_node_id) else {
            return true;
        };
        if node.degree() < 3 {
            return true;
        }
        let vehicle = &self.vehicles[&id];
        let waited = vehicle.requested_connector == Some(connector_id)
            && self.time - vehicle.waiting_since >= CROSSWALK_PATIENCE;
        for segment_id in Self::roads_crossed(ctx.roads, connector) {
            let crosswalk = IVec2::new(node.id.0, segment_id.0);
            if crosswalks.has_walkers(crosswalk) {
                return false;
            }
            if crosswalks.has_waiting(crosswalk) {
                if node.has_lights() && ctx.signals.is_walk_green(ctx.roads, node, segment_id) {
                    return false;
                }
                if !node.has_lights() && !waited {
                    return false;
                }
            }
        }
        true
    }

    /// The roads (segment ids) whose crosswalks a turn passes over: the one it comes from
    /// and the one it goes into.
    fn roads_crossed(roads: &RoadNetwork, connector: &Lane) -> Vec<SegmentId> {
        let exit = lane(roads, connector.outgoing_lane_ids[0]);
        let mut crossed: Vec<SegmentId> = exit.segment_id.into_iter().collect();
        if let Some(feeder) = connector.feeder_lane_id.and_then(|id| roads.lanes.get(&id)) {
            crossed.extend(feeder.segment_id);
        }
        crossed
    }

    /// Must pedestrians at the curb of the crosswalk across road `crosswalk.y` at junction
    /// `crosswalk.x` wait for a car? Yes while one is in (or allowed into) a turn over it,
    /// and, without lights, while a driver who has waited CROSSWALK_PATIENCE for such a
    /// turn is at the stop line (see `crosswalks_clear`). Mid-block crosswalks:
    /// `mid_block_busy`.
    pub fn is_crosswalk_busy(
        &self,
        roads: &RoadNetwork,
        crosswalks: &Crosswalks,
        crosswalk: IVec2,
    ) -> bool {
        if Crosswalks::is_mid_block(crosswalk) {
            return self.mid_block_busy(roads, crosswalks, crosswalk);
        }
        let Some(node) = roads.nodes.get(&NodeId(crosswalk.x)) else {
            return false;
        };
        let crossed = SegmentId(crosswalk.y);
        for connector_id in &node.connector_lane_ids {
            if !self.connector_vehicles.contains_key(connector_id) {
                continue;
            }
            if Self::roads_crossed(roads, &roads.lanes[connector_id]).contains(&crossed) {
                return true;
            }
        }
        if !node.has_lights() {
            for vehicle_id in self.waiting.get(&node.id).into_iter().flatten() {
                let vehicle = &self.vehicles[vehicle_id];
                if self.time - vehicle.waiting_since >= CROSSWALK_PATIENCE {
                    let requested = lane(
                        roads,
                        vehicle.requested_connector.expect("waiting for a turn"),
                    );
                    if Self::roads_crossed(roads, requested).contains(&crossed) {
                        return true;
                    }
                }
            }
        }
        false
    }
}
