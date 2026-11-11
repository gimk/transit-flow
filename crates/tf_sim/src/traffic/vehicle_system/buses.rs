//! Buses on the road: the "Bus bays" section of `vehicle_system.gd`, `pull_into_bay`, and
//! drivers pulling out round a bus stopped in their lane (`pass_stopped_buses`).

use super::*;

impl VehicleSystem {
    /// A bus that has just stopped (dwelling) at a bus bay pulls into it: off its lane, so
    /// the traffic behind drives past while it dwells.
    pub fn pull_into_bay(&mut self, roads: &RoadNetwork, id: VehicleId) {
        let vehicle = &self.vehicles[&id];
        if vehicle.in_bay || !vehicle.dwelling {
            return;
        }
        self.dequeue(id);
        let vehicle = &mut self.vehicles[&id];
        vehicle.in_bay = true;
        Self::start_slide(vehicle, BAY_OFFSET);
        Self::update_pose(roads, vehicle);
    }

    /// A bus sent on from a bay pulls back into its lane when there is a gap, like a car
    /// from a driveway (`clear_to_join`), the gap it insists on shrinking as it waits.
    pub(super) fn leave_bays(&mut self, _ctx: &mut TrafficCtx) {
        let ids: Vec<VehicleId> = self.vehicles.keys().copied().collect();
        for id in ids {
            let vehicle = &self.vehicles[&id];
            if vehicle.leaving_bay_since == f64::INFINITY {
                continue;
            }
            let gap = self.waited_gap(critical_gap(Turn::Right), vehicle.leaving_bay_since);
            let lane_id = vehicle.lane.expect("on the road");
            if self.clear_to_join(lane_id, vehicle.distance, vehicle.length, gap) {
                let vehicle = &mut self.vehicles[&id];
                vehicle.in_bay = false;
                vehicle.dwelling = false;
                vehicle.leaving_bay_since = f64::INFINITY;
                self.enqueue(id);
            }
        }
    }

    /// Drivers let a bus waiting to pull out of a bay go after BAY_COURTESY_WAIT (yield to
    /// the bus): the next vehicle coming up stops short of it, if it comfortably can.
    pub(super) fn let_buses_out(&mut self, _ctx: &mut TrafficCtx) {
        let ids: Vec<VehicleId> = self.vehicles.keys().copied().collect();
        for id in ids {
            let vehicle = &self.vehicles[&id];
            if vehicle.leaving_bay_since == f64::INFINITY
                || self.time - vehicle.leaving_bay_since < BAY_COURTESY_WAIT
            {
                continue;
            }
            let lane_id = vehicle.lane.expect("on the road");
            let (distance, length) = (vehicle.distance, vehicle.length);
            if self.space_opening(lane_id, distance, length) {
                self.hold_back(lane_id, distance - length * 0.5 - MIN_GAP);
            }
        }
    }

    /// Drivers right behind a bus dwelling at its stop pull out round it: they change into
    /// the lane beside (going the same way; not a bus lane, for cars), if their way goes
    /// on from there. Lane changes do the rest (`change_lanes`).
    pub(super) fn pass_stopped_buses(&mut self, ctx: &mut TrafficCtx) {
        let lanes: Vec<LaneId> = self.occupied.keys().copied().collect();
        for lane_id in lanes {
            let Some(queue) = self.occupied.get(&lane_id).cloned() else {
                continue;
            };
            let current = lane(ctx.roads, lane_id);
            for i in 1..queue.len() {
                let bus = &self.vehicles[&queue[i - 1]];
                let vehicle = &self.vehicles[&queue[i]];
                if !bus.dwelling
                    || vehicle.dwelling
                    || bus.distance - vehicle.distance > vehicle.length + PASS_DISTANCE
                {
                    continue;
                }
                if vehicle
                    .next_lanes
                    .first()
                    .is_some_and(|&next| lane(ctx.roads, next).is_beside(current))
                {
                    continue; // Already moving across.
                }
                if vehicle.is_on_last_lane() && vehicle.end_distance < bus.distance {
                    continue; // Its stop is before the bus.
                }
                self.pull_out_round(ctx, queue[i], current);
            }
        }
    }

    fn pull_out_round(&mut self, ctx: &mut TrafficCtx, id: VehicleId, current: &Lane) {
        let segment = &ctx.roads.segments[&current.segment_id.expect("a road lane")];
        for side_id in &segment.lane_ids {
            let side = &ctx.roads.lanes[side_id];
            if !side.is_beside(current)
                || (side.index_from_center - current.index_from_center).abs() != 1
            {
                continue;
            }
            let bus = is_bus(&self.vehicles[&id]);
            if side.bus_only && !bus {
                continue;
            }
            let at = self.distance_beside(ctx.roads, id, side);
            let destination = Self::destination(ctx, &self.vehicles[&id]);
            let Some(route) = ctx.router.find_best_route(
                ctx.roads,
                &[LanePoint::new(side.id, at)],
                &destination,
                bus,
            ) else {
                continue;
            };
            // A new way on: any junction it was let into or waiting for no longer applies.
            self.release_grant(ctx.roads, id);
            self.stop_waiting(id);
            let vehicle = &mut self.vehicles[&id];
            vehicle.next_lanes = std::iter::once(side.id)
                .chain(route.lane_ids[1..].iter().copied())
                .collect();
            vehicle.end_distance = route.end_distance;
            vehicle.arrival_checked = false;
            return;
        }
    }
}
