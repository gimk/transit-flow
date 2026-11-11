//! Lane changes: the "Lane changes" section of `vehicle_system.gd` (moving across in
//! gaps, swaps, zipper merging, giving up at the end of the lane, steering across).

use super::*;

impl VehicleSystem {
    /// Vehicles whose route continues on the lane beside theirs (`Lane::is_beside`) move
    /// across as soon as there is a safe gap there: room for the whole vehicle, and the
    /// vehicle that ends up behind can brake to its speed comfortably. Until then they
    /// drive on, and stop at the end of their lane if they must (`scan_ahead`), where they
    /// count as merging and the traffic beside makes room (`make_room_for_mergers`).
    pub(super) fn change_lanes(&mut self, ctx: &mut TrafficCtx) {
        let roads = ctx.roads;
        let ids: Vec<VehicleId> = self.vehicles.keys().copied().collect();
        for id in ids {
            let vehicle = &mut self.vehicles[&id];
            vehicle.merging = false;
            if vehicle.dwelling {
                continue; // A bus at its stop changes lanes once it drives on.
            }
            let current = lane(roads, vehicle.lane.expect("on the road"));
            let Some(&target_id) = vehicle.next_lanes.first() else {
                continue;
            };
            let target = lane(roads, target_id);
            if !target.is_beside(current) {
                continue;
            }
            let target_distance = self.distance_beside(roads, id, target);
            if self.has_room_to_change(id, target, target_distance, None) {
                self.move_across(roads, id, target_id, target_distance);
                continue;
            }
            if let Some(partner) = self.swap_partner(roads, id, target, target_distance) {
                let partner_distance = self.distance_beside(roads, partner, current);
                let current_id = current.id;
                self.move_across(roads, id, target_id, target_distance);
                self.move_across(roads, partner, current_id, partner_distance);
                continue;
            }
            let vehicle = &self.vehicles[&id];
            let remaining = vehicle.remaining_on_lane(current);
            if remaining < MERGE_DISTANCE || vehicle.speed < STOPPED_SPEED {
                let front = self.queue(current.id).first() == Some(&id);
                let vehicle = &mut self.vehicles[&id];
                vehicle.merging = true;
                if vehicle.stopped_time > MISSED_LANE_WAIT
                    && front
                    && remaining < vehicle.length + MERGE_DISTANCE
                {
                    self.carry_on_without_changing(ctx, id);
                }
            }
        }
    }

    /// Two vehicles side by side, each wanting the other's lane (common in jams: a left
    /// turner in the right lane next to a right turner in the left one), swap places when
    /// neither would then be too close to anyone else. Returns the vehicle to swap with.
    fn swap_partner(
        &self,
        roads: &RoadNetwork,
        id: VehicleId,
        target: &Lane,
        at: f64,
    ) -> Option<VehicleId> {
        let vehicle = &self.vehicles[&id];
        let current_id = vehicle.lane.expect("on the road");
        let current = lane(roads, current_id);
        for &other_id in self.queue(target.id) {
            let other = &self.vehicles[&other_id];
            if other.next_lanes.first() != Some(&current_id) {
                continue;
            }
            if (other.distance - at).abs() > (other.length + vehicle.length) * 0.5 + MIN_GAP {
                continue;
            }
            let other_at = self.distance_beside(roads, other_id, current);
            if self.has_room_to_change(id, target, at, Some(other_id))
                && self.has_room_to_change(other_id, current, other_at, Some(id))
            {
                return Some(other_id);
            }
        }
        None
    }

    /// A driver who couldn't get into the lane they needed before the end of the road
    /// goes on through the junction from the lane they're in and finds a new way from
    /// there.
    fn carry_on_without_changing(&mut self, ctx: &mut TrafficCtx, id: VehicleId) {
        let destination = Self::destination(ctx, &self.vehicles[&id]);
        let vehicle = &self.vehicles[&id];
        let current = vehicle.lane.expect("on the road");
        let Some(route) =
            ctx.router
                .find_route_onward(ctx.roads, current, &destination, is_bus(vehicle))
        else {
            return;
        };
        let vehicle = &mut self.vehicles[&id];
        vehicle.next_lanes = route.lane_ids;
        vehicle.end_distance = route.end_distance;
        vehicle.merging = false;
    }

    /// Where alongside `target` the vehicle is (lanes of a straight road line up exactly),
    /// kept far enough from its end for the whole vehicle to fit.
    pub(super) fn distance_beside(&self, roads: &RoadNetwork, id: VehicleId, target: &Lane) -> f64 {
        let vehicle = &self.vehicles[&id];
        let current = lane(roads, vehicle.lane.expect("on the road"));
        let mut at = vehicle.distance;
        if !(current.is_straight && target.is_straight) {
            at = f64::from(target.curve().closest_offset(vehicle.position));
        }
        at.clamp(0.0, (target.length - vehicle.length * 0.5).max(0.0))
    }

    /// `ignored`: a vehicle not to count (the one swapping places with this one).
    fn has_room_to_change(
        &self,
        id: VehicleId,
        target: &Lane,
        at: f64,
        ignored: Option<VehicleId>,
    ) -> bool {
        let vehicle = &self.vehicles[&id];
        let half = vehicle.length * 0.5;
        // Not right at the start, where vehicles come out of the junction (and space is
        // promised to those let in), nor past the end.
        let promised = f64::from(self.reserved_entries.get(&target.id).copied().unwrap_or(0));
        if at - half < MIN_GAP + promised * (MAX_LENGTH + MIN_GAP) || at + half > target.length {
            return false;
        }
        for &other_id in self.queue(target.id) {
            if Some(other_id) == ignored {
                continue;
            }
            let other = &self.vehicles[&other_id];
            let ahead_by = other.distance - at;
            let mut needed = (other.length + vehicle.length) * 0.5 + MIN_GAP;
            if ahead_by >= 0.0 {
                // It goes in front of `other`: must be able to brake to its speed.
                needed += (vehicle.speed * vehicle.speed - other.speed * other.speed).max(0.0)
                    / (2.0 * DECELERATION);
            } else {
                // `other` ends up behind it and must be able to brake to its speed.
                needed += (other.speed * other.speed - vehicle.speed * vehicle.speed).max(0.0)
                    / (2.0 * DECELERATION);
            }
            if ahead_by.abs() < needed {
                return false;
            }
        }
        true
    }

    fn move_across(&mut self, roads: &RoadNetwork, id: VehicleId, target: LaneId, at: f64) {
        let old_position = self.vehicles[&id].position;
        self.dequeue(id);
        {
            let vehicle = &mut self.vehicles[&id];
            vehicle.lane = Some(target);
            vehicle.distance = at;
            vehicle.next_lanes.remove(0);
        }
        self.enqueue(id);
        // Drawn where it was, sliding over to the new lane's center.
        let (position, angle) = lane(roads, target).sample_pose(at);
        let offset = (old_position - position).dot(right_of(Vec2::from_angle(angle)));
        let vehicle = &mut self.vehicles[&id];
        Self::start_slide(vehicle, f64::from(offset));
        Self::update_pose(roads, vehicle);
    }

    /// Starts drawing the vehicle `offset` px right of its lane, to steer back onto it.
    pub(super) fn start_slide(vehicle: &mut Vehicle, offset: f64) {
        vehicle.lateral_offset = offset;
        vehicle.slide_from = offset;
        vehicle.slide_progress = if offset != 0.0 { 0.0 } else { 1.0 };
        vehicle.slide_yaw = 0.0;
    }

    /// Steers a vehicle a tick further across into its lane: the offset follows a
    /// smoothstep over the distance driven, and the nose points along it.
    pub(super) fn slide(vehicle: &mut Vehicle, dt: f64) {
        if vehicle.slide_progress >= 1.0 {
            vehicle.lateral_offset = 0.0;
            vehicle.slide_yaw = 0.0;
            return;
        }
        let length =
            LANE_CHANGE_MIN_LENGTH.max(vehicle.slide_from.abs() * LANE_CHANGE_LENGTH_RATIO);
        let step = (vehicle.speed * dt / length).max(dt / LANE_CHANGE_MAX_TIME);
        let p = (vehicle.slide_progress + step).min(1.0);
        vehicle.slide_progress = p;
        vehicle.lateral_offset = vehicle.slide_from * (1.0 - p * p * (3.0 - 2.0 * p));
        // d(offset)/d(distance): smoothstep's slope is 6p(1 - p).
        let slope = -vehicle.slide_from * 6.0 * p * (1.0 - p) / length;
        vehicle.slide_yaw = slope
            .atan()
            .clamp(-LANE_CHANGE_MAX_YAW, LANE_CHANGE_MAX_YAW);
    }

    /// A vehicle on the target lane coming up behind a merging vehicle holds back to let
    /// it in, if it can stop comfortably short of it (otherwise the next one will).
    pub(super) fn make_room_for_mergers(&mut self, ctx: &mut TrafficCtx) {
        let roads = ctx.roads;
        let mergers: Vec<VehicleId> = self
            .vehicles
            .values()
            .filter(|v| v.merging)
            .map(|v| v.id)
            .collect();
        for merger_id in mergers {
            let merger = &self.vehicles[&merger_id];
            let target = lane(roads, merger.next_lanes[0]);
            let at = self.distance_beside(roads, merger_id, target);
            let merger_length = merger.length;
            let queue = self.queue(target.id).to_vec();
            for other_id in queue {
                let other = &mut self.vehicles[&other_id];
                if other.distance >= at {
                    continue;
                }
                // The first vehicle behind the merge point.
                let gap = at - merger_length * 0.5 - other.distance - other.length * 0.5;
                if other.speed * other.speed / (2.0 * DECELERATION) + MIN_GAP <= gap
                    && gap < other.leader_gap
                {
                    other.leader_gap = gap;
                    other.leader_speed = 0.0;
                }
                break;
            }
        }
    }
}
