//! Driveways: the "Driveways" section of `vehicle_system.gd` (waiting to pull out,
//! crossing to the far lane, turning in, and the traffic letting drivers in and out).
//! Vehicles waiting to pull out aren't on the road yet: they live in `departures`, so
//! the functions about them take the vehicle itself.

use super::*;

impl VehicleSystem {
    /// Each lot's first waiting vehicle pulls out when the lane it joins has a gap: nobody
    /// alongside, and traffic coming up behind at least a critical gap away (a right
    /// turn's gap to join the near lane; a left turn's to cross to the far one, where the
    /// lanes it crosses need a gap too). It joins at driveway speed, so the traffic behind
    /// it brakes. A vehicle that has waited DRIVEWAY_GIVE_UP to cross to the far lane turns
    /// onto the near one instead and finds its way from there.
    pub(super) fn pull_out(&mut self, ctx: &mut TrafficCtx) {
        if self.departures.is_empty() {
            return;
        }
        let mut tried = std::collections::HashSet::new();
        let mut i = 0;
        while i < self.departures.len() {
            let vehicle = &self.departures[i];
            let key = vehicle.origin_lot_id.map_or(-1 - vehicle.id.0, |lot| lot.0);
            if !tried.insert(key) {
                i += 1;
                continue;
            }
            if self.can_pull_out(ctx.roads, vehicle) {
                let mut vehicle = self.departures.remove(i);
                let route = vehicle
                    .route
                    .take()
                    .expect("a waiting vehicle has its route");
                self.put_on_road(ctx.roads, vehicle, route);
                continue;
            }
            if !vehicle.crossing_lanes.is_empty()
                && self.time - vehicle.driveway_wait_since > DRIVEWAY_GIVE_UP
            {
                let destination = Self::destination(ctx, &self.departures[i]);
                let near = self.departures[i].origin_points[0];
                if let Some(near_side) =
                    ctx.router
                        .find_best_route(ctx.roads, &[near], &destination, false)
                {
                    Self::plan_departure(ctx, &mut self.departures[i], near_side);
                }
            }
            i += 1;
        }
        self.place_waiting_departures(ctx);
    }

    /// Sets the route a waiting vehicle will pull out onto, and the lanes it must cross
    /// first if that starts on the far side of the road.
    pub(super) fn plan_departure(ctx: &mut TrafficCtx, vehicle: &mut Vehicle, mut route: Route) {
        // A driveway right at a junction (e.g. a corner lot by a wide avenue, whose lanes
        // start well back from the junction) joins just clear of it, where the whole
        // vehicle fits.
        let first = &ctx.roads.lanes[&route.lane_ids[0]];
        let earliest = vehicle.length * 0.5 + MIN_GAP;
        if route.start_distance < earliest
            && (route.lane_ids.len() > 1 || route.end_distance > earliest)
        {
            route.start_distance = earliest.min(first.length);
        }
        vehicle.destination_lane_id = route.lane_ids.last().copied();
        vehicle.destination_distance = route.end_distance;
        vehicle.crossing_lanes.clear();
        vehicle.crossing_distances.clear();
        let start = route.lane_ids[0];
        vehicle.route = Some(route);
        if let Some(lot) = vehicle.origin_lot_id {
            Self::add_crossing(ctx, vehicle, lot, start);
        }
    }

    /// If `lane_id` is on the far side of the road from `lot_id`, the lanes between (those
    /// on the lot's side) become the vehicle's crossing lanes.
    fn add_crossing(ctx: &mut TrafficCtx, vehicle: &mut Vehicle, lot_id: LotId, lane_id: LaneId) {
        let Some(near) = ctx.lots.access(ctx.roads, lot_id) else {
            return;
        };
        let Some(near_lane) = ctx.roads.lanes.get(&near.lane_id) else {
            return;
        };
        let target = lane(ctx.roads, lane_id);
        if target.id == near_lane.id || target.is_beside(near_lane) {
            return;
        }
        let segment = &ctx.roads.segments[&near_lane.segment_id.expect("a road lane")];
        for other_id in &segment.lane_ids {
            let other = &ctx.roads.lanes[other_id];
            if other.id == near_lane.id || other.is_beside(near_lane) {
                vehicle.crossing_lanes.push(*other_id);
                vehicle.crossing_distances.push(near.distance as f32);
            }
        }
    }

    fn can_pull_out(&self, roads: &RoadNetwork, vehicle: &Vehicle) -> bool {
        let route = vehicle
            .route
            .as_ref()
            .expect("a waiting vehicle has its route");
        let Some(join) = roads.lanes.get(&route.lane_ids[0]) else {
            return false;
        };
        let turn = if vehicle.crossing_lanes.is_empty() {
            Turn::Right
        } else {
            Turn::Left
        };
        let gap = self.waited_gap(critical_gap(turn), vehicle.driveway_wait_since);
        if !self.clear_to_join(join.id, route.start_distance, vehicle.length, gap) {
            return false;
        }
        for (i, &crossing) in vehicle.crossing_lanes.iter().enumerate() {
            if !self.clear_to_cross(crossing, f64::from(vehicle.crossing_distances[i]), gap) {
                return false;
            }
        }
        true
    }

    /// The gap a driver still insists on after waiting since `since` (it shrinks to half
    /// over GAP_SHRINK_TIME, like at junctions, but never to nothing: pulling out is never
    /// forced).
    pub(super) fn waited_gap(&self, gap: f64, since: f64) -> f64 {
        gap * (1.0 - 0.5 * ((self.time - since) / GAP_SHRINK_TIME).clamp(0.0, 1.0))
    }

    /// Room to join `lane` at `at` at driveway speed: nobody alongside, nothing coming up
    /// from behind within `gap` seconds, and not where vehicles let out of the junction
    /// will arrive.
    pub(super) fn clear_to_join(&self, lane_id: LaneId, at: f64, length: f64, gap: f64) -> bool {
        let promised = f64::from(self.reserved_entries.get(&lane_id).copied().unwrap_or(0));
        if at - length * 0.5 < MIN_GAP + promised * (MAX_LENGTH + MIN_GAP) {
            return false;
        }
        for other_id in self.queue(lane_id) {
            let other = &self.vehicles[other_id];
            let ahead_by = other.distance - at;
            let clearance = ahead_by.abs() - (other.length + length) * 0.5 - MIN_GAP;
            if clearance < 0.0 {
                return false;
            }
            if ahead_by < 0.0
                && other.speed > STOPPED_SPEED
                && (clearance / other.speed < gap
                    || other.speed * other.speed / (2.0 * DECELERATION) > clearance)
            {
                return false;
            }
        }
        true
    }

    /// Room to drive across `lane` at `at`: nobody on the crossing point, and nothing
    /// coming within `gap` seconds.
    fn clear_to_cross(&self, lane_id: LaneId, at: f64, gap: f64) -> bool {
        for other_id in self.queue(lane_id) {
            let other = &self.vehicles[other_id];
            let ahead_by = other.distance - at;
            if ahead_by.abs() < other.length * 0.5 + CROSSING_HALF_WIDTH {
                return false;
            }
            if ahead_by < 0.0
                && other.speed > STOPPED_SPEED
                && (-ahead_by - other.length * 0.5 - CROSSING_HALF_WIDTH) / other.speed < gap
            {
                return false;
            }
        }
        true
    }

    /// Where waiting vehicles are drawn: on their lot, nose to the road, one behind the
    /// other.
    pub(super) fn place_waiting_departures(&mut self, ctx: &mut TrafficCtx) {
        let mut waiting_at: HashMap<LotId, i32> = HashMap::new();
        for i in 0..self.departures.len() {
            let origin = self.departures[i].origin_lot_id;
            let near = origin.and_then(|lot| ctx.lots.access(ctx.roads, lot));
            let vehicle = &mut self.departures[i];
            let Some(near) = near else {
                let route = vehicle
                    .route
                    .as_ref()
                    .expect("a waiting vehicle has its route");
                if let Some(start) = ctx.roads.lanes.get(&route.lane_ids[0]) {
                    let (position, angle) = start.sample_pose(route.start_distance);
                    vehicle.position = position;
                    vehicle.rotation = f64::from(angle);
                }
                continue;
            };
            let lot = origin.expect("near access comes from a lot");
            let (curb, _) = ctx.roads.lanes[&near.lane_id].sample_pose(near.distance);
            let lot_center = ctx.lots.lots[&lot].center();
            let out = (curb - lot_center).gd_normalized();
            let place = waiting_at.get(&lot).copied().unwrap_or(0);
            waiting_at.insert(lot, place + 1);
            // More than a few waiting are drawn piled up on the last spot.
            let back = DRIVEWAY_SETBACK
                + vehicle.length * 0.5
                + f64::from(place.min(DRAWN_DRIVEWAY_QUEUE - 1)) * (MAX_LENGTH + MIN_GAP);
            vehicle.position = curb - out * back as f32;
            vehicle.rotation = f64::from(out.gd_angle());
            vehicle.previous_position = vehicle.position;
            vehicle.previous_rotation = vehicle.rotation;
        }
    }

    /// How fast the vehicle should be going when it reaches the end of its trip: driveway
    /// speed, or a stop if it has to wait to turn across the road or is a bus at a stop.
    pub(super) fn final_speed(&mut self, ctx: &mut TrafficCtx, id: VehicleId) -> f64 {
        if self.vehicles[&id].in_service || self.must_cross_to_arrive(ctx, id) {
            return 0.0;
        }
        if self.vehicles[&id].kind == Kind::Truck {
            TRUCK_DRIVEWAY_SPEED
        } else {
            DRIVEWAY_SPEED
        }
    }

    /// Is the vehicle's driveway on the far side of the road from the lane its trip ends
    /// on? (Worked out once per route.)
    pub(super) fn must_cross_to_arrive(&mut self, ctx: &mut TrafficCtx, id: VehicleId) -> bool {
        let vehicle = &mut self.vehicles[&id];
        if !vehicle.arrival_checked {
            vehicle.arrival_checked = true;
            vehicle.crossing_lanes.clear();
            vehicle.crossing_distances.clear();
            if let Some(lot) = vehicle.destination_lot_id
                && ctx.lots.lots.contains_key(&lot)
            {
                let final_lane = *vehicle
                    .next_lanes
                    .last()
                    .unwrap_or(&vehicle.lane.expect("on the road"));
                Self::add_crossing(ctx, vehicle, lot, final_lane);
            }
        }
        !vehicle.crossing_lanes.is_empty()
    }

    pub(super) fn can_turn_in(&self, id: VehicleId) -> bool {
        // Unlike pulling out, a driver waiting on the road to turn in shrinks the gap they
        // need all the way, like at a junction: they are holding everyone up.
        let vehicle = &self.vehicles[&id];
        let since = if vehicle.driveway_wait_since < f64::INFINITY {
            vehicle.driveway_wait_since
        } else {
            self.time
        };
        let gap = critical_gap(Turn::Left)
            * (1.0 - ((self.time - since) / GAP_SHRINK_TIME).clamp(0.0, 1.0));
        for (i, &crossing) in vehicle.crossing_lanes.iter().enumerate() {
            if !self.clear_to_cross(crossing, f64::from(vehicle.crossing_distances[i]), gap) {
                return false;
            }
        }
        true
    }

    /// A vehicle that has waited TURN_IN_GIVE_UP to turn across the road into its
    /// driveway drives on and comes back round to it from the near side.
    fn give_up_turning_in(&mut self, ctx: &mut TrafficCtx, id: VehicleId) {
        if !self.vehicles[&id].is_on_last_lane() {
            // Re-routed since it started waiting: it isn't waiting to turn in any more.
            self.vehicles[&id].driveway_wait_since = f64::INFINITY;
            return;
        }
        let vehicle = &self.vehicles[&id];
        let lot = vehicle.destination_lot_id.expect("turning into a lot");
        let current = vehicle.lane.expect("on the road");
        let route = ctx.lots.access(ctx.roads, lot).and_then(|near| {
            ctx.router
                .find_route_onward(ctx.roads, current, &[near], false)
        });
        let Some(route) = route else {
            return;
        };
        let vehicle = &mut self.vehicles[&id];
        vehicle.next_lanes = route.lane_ids;
        vehicle.end_distance = route.end_distance;
        vehicle.arrival_checked = false;
        vehicle.driveway_wait_since = f64::INFINITY;
    }

    /// Traffic lets waiting vehicles out and in: once one has waited COURTESY_WAIT, the
    /// next vehicle on each lane it must join or cross stops short of it, if it can
    /// comfortably.
    pub(super) fn keep_crossings_clear(&mut self, ctx: &mut TrafficCtx) {
        let ids: Vec<VehicleId> = self.vehicles.keys().copied().collect();
        for id in ids {
            let since = self.vehicles[&id].driveway_wait_since;
            if since == f64::INFINITY {
                continue;
            }
            if self.time - since > TURN_IN_GIVE_UP {
                self.give_up_turning_in(ctx, id);
                continue;
            }
            if self.time - since > COURTESY_WAIT {
                let vehicle = &self.vehicles[&id];
                let crossings = (
                    vehicle.crossing_lanes.clone(),
                    vehicle.crossing_distances.clone(),
                );
                self.hold_back_for_crossing(&crossings.0, &crossings.1);
            }
        }
        // The first vehicle waiting at each lot is let out the same way; into a crawling
        // queue, almost at once (there will never be a gap otherwise).
        let mut seen = std::collections::HashSet::new();
        for i in 0..self.departures.len() {
            let vehicle = &self.departures[i];
            let key = vehicle.origin_lot_id.map_or(-1 - vehicle.id.0, |lot| lot.0);
            if !seen.insert(key) {
                continue;
            }
            let route = vehicle
                .route
                .as_ref()
                .expect("a waiting vehicle has its route");
            let Some(join) = ctx.roads.lanes.get(&route.lane_ids[0]) else {
                continue;
            };
            let at = route.start_distance;
            let stop_at = at - vehicle.length * 0.5 - MIN_GAP;
            let patience = if self.crawling_towards(join, stop_at) {
                QUICK_COURTESY_WAIT
            } else {
                COURTESY_WAIT
            };
            if self.time - vehicle.driveway_wait_since > patience
                && self.space_opening(join.id, at, vehicle.length)
            {
                let crossings = (
                    vehicle.crossing_lanes.clone(),
                    vehicle.crossing_distances.clone(),
                );
                self.hold_back_for_crossing(&crossings.0, &crossings.1);
                self.hold_back(join.id, stop_at);
            }
        }
    }

    /// Is the stretch of `lane` in front of `at` free, or is the vehicle there moving on?
    /// (If it stands still, holding back the traffic behind would only block the road.)
    pub(super) fn space_opening(&self, lane_id: LaneId, at: f64, length: f64) -> bool {
        for other_id in self.queue(lane_id) {
            let other = &self.vehicles[other_id];
            if other.distance >= at && other.distance - at < (other.length + length) * 0.5 + MIN_GAP
            {
                return other.speed > STOPPED_SPEED;
            }
        }
        true
    }

    /// Is the traffic coming up to `at` on `lane` slow (a queue rather than a stream)?
    fn crawling_towards(&self, join: &Lane, at: f64) -> bool {
        for other_id in self.queue(join.id) {
            let other = &self.vehicles[other_id];
            if other.distance < at {
                return other.speed < SLOW_FRACTION * join.speed_limit;
            }
        }
        false
    }

    fn hold_back_for_crossing(&mut self, lanes: &[LaneId], distances: &[f32]) {
        for (i, &crossing) in lanes.iter().enumerate() {
            self.hold_back(crossing, f64::from(distances[i]) - CROSSING_HALF_WIDTH);
        }
    }

    /// The first vehicle on `lane` short of `stop_at` stops there, if it can comfortably.
    pub(super) fn hold_back(&mut self, lane_id: LaneId, stop_at: f64) {
        let Some(queue) = self.occupied.get(&lane_id) else {
            return;
        };
        for other_id in queue {
            let other = &mut self.vehicles[other_id];
            if other.distance >= stop_at {
                continue;
            }
            let gap = stop_at - other.distance - other.length * 0.5;
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
