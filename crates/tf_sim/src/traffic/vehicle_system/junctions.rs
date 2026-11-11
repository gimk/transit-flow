//! Pass 1 (what is ahead, junction requests) and junction entry: the "Pass 1" section of
//! `vehicle_system.gd` and the junction functions it calls.

use std::collections::HashMap;

use super::*;
use crate::roads::lane_builder;
use crate::traffic::traffic_lights::Light;

impl VehicleSystem {
    pub(super) fn prepare(&mut self, ctx: &mut TrafficCtx) {
        let lanes: Vec<LaneId> = self.occupied.keys().copied().collect();
        for lane_id in lanes {
            let queue = self.occupied[&lane_id].clone();
            for (i, &id) in queue.iter().enumerate() {
                self.vehicles[&id].stop_gap = f64::INFINITY;
                if i > 0 {
                    let leader = &self.vehicles[&queue[i - 1]];
                    let (leader_distance, leader_length, leader_speed) =
                        (leader.distance, leader.length, leader.speed);
                    let vehicle = &mut self.vehicles[&id];
                    vehicle.leader_gap =
                        leader_distance - vehicle.distance - (leader_length + vehicle.length) * 0.5;
                    vehicle.leader_speed = leader_speed;
                } else {
                    self.request_junction(ctx, id);
                    self.scan_ahead(ctx.roads, id);
                    self.yield_in_junction(ctx.roads, id);
                }
            }
        }
    }

    /// For the front-most vehicle on a lane: the nearest vehicle further along its route,
    /// and the first junction it hasn't been allowed into.
    fn scan_ahead(&mut self, roads: &RoadNetwork, id: VehicleId) {
        let vehicle = &self.vehicles[&id];
        let current = lane(roads, vehicle.lane.expect("on the road"));
        let mut leader_gap = f64::INFINITY;
        let mut leader_speed = 0.0;
        let mut consider = |gap: f64, speed: f64| {
            if gap < leader_gap {
                leader_gap = gap;
                leader_speed = speed;
            }
        };
        let mut ahead = current.length - vehicle.distance;
        // A vehicle that just turned off this lane in another direction is still right
        // ahead.
        for out in live_outgoing(roads, current) {
            if let Some(&last) = self.queue(out).last() {
                let back = &self.vehicles[&last];
                consider(
                    ahead + back.distance - (back.length + vehicle.length) * 0.5,
                    back.speed,
                );
            }
        }
        let horizon = vehicle.speed * vehicle.speed / (2.0 * DECELERATION)
            + LOOKAHEAD_MARGIN
            + vehicle.length;
        let mut stop_gap = vehicle.stop_gap;
        let changing = vehicle
            .next_lanes
            .first()
            .is_some_and(|&next| lane(roads, next).is_beside(current));
        if changing {
            // Still has to change lanes: it can't go on past the end of this one.
            stop_gap = ahead - vehicle.length * 0.5 - LANE_CHANGE_END_MARGIN;
        } else {
            for &next_id in &vehicle.next_lanes {
                if ahead > horizon {
                    break;
                }
                let next = lane(roads, next_id);
                if next.is_connector() && Some(next_id) != vehicle.granted_connector {
                    stop_gap = ahead - vehicle.length * 0.5;
                    break;
                }
                if let Some(&last) = self.queue(next_id).last() {
                    let back = &self.vehicles[&last];
                    consider(
                        ahead + back.distance - (back.length + vehicle.length) * 0.5,
                        back.speed,
                    );
                    break;
                }
                ahead += next.length;
            }
        }
        let vehicle = &mut self.vehicles[&id];
        vehicle.leader_gap = leader_gap;
        vehicle.leader_speed = leader_speed;
        vehicle.stop_gap = stop_gap;
    }

    /// Safety net under the timing prediction in `conflicts_clear`: a vehicle never
    /// enters a conflict zone while a vehicle let into the junction before it is still in
    /// the matching zone; it stops short of the zone instead. Vehicles only ever wait for
    /// earlier entrants, so two vehicles can't end up waiting for each other.
    fn yield_in_junction(&mut self, roads: &RoadNetwork, id: VehicleId) {
        let vehicle = &self.vehicles[&id];
        let current = lane(roads, vehicle.lane.expect("on the road"));
        let on_connector = current.is_connector();
        let Some(connector_id) = (if on_connector {
            Some(current.id)
        } else {
            vehicle.granted_connector
        }) else {
            return;
        };
        let connector = lane(roads, connector_id);
        let mut front = vehicle.distance + vehicle.length * 0.5;
        if !on_connector {
            front -= current.length;
        }
        let mut stop_gap = vehicle.stop_gap;
        for &other_id in &connector.conflicting_lane_ids {
            let my_zone = connector.conflict_zones[&other_id];
            if front >= f64::from(my_zone.x) {
                continue;
            }
            for user in self.connector_vehicles.get(&other_id).into_iter().flatten() {
                let other = &self.vehicles[user];
                if other.entry_order >= vehicle.entry_order {
                    continue;
                }
                let other_lane = lane(roads, other.lane.expect("on the road"));
                let other_on_it = other_lane.id == other_id;
                let other_connector = if other_on_it {
                    other_lane
                } else {
                    lane(roads, other.granted_connector.expect("allowed in"))
                };
                let along = if other_on_it {
                    other.distance
                } else {
                    other.distance - other_lane.length
                };
                if along - other.length * 0.5
                    < f64::from(other_connector.conflict_zones[&connector_id].y)
                {
                    stop_gap = stop_gap.min(f64::from(my_zone.x) - front - 1.0);
                }
            }
        }
        self.vehicles[&id].stop_gap = stop_gap;
    }

    /// A front-most vehicle close enough to its next junction asks to enter it.
    fn request_junction(&mut self, ctx: &mut TrafficCtx, id: VehicleId) {
        let vehicle = &self.vehicles[&id];
        let Some(&connector) = vehicle.next_lanes.first() else {
            return;
        };
        if !lane(ctx.roads, connector).is_connector()
            || Some(connector) == vehicle.granted_connector
        {
            return;
        }
        let current = lane(ctx.roads, vehicle.lane.expect("on the road"));
        let to_line = current.length - vehicle.distance - vehicle.length * 0.5;
        if to_line > vehicle.speed * vehicle.speed / (2.0 * DECELERATION) + LOOKAHEAD_MARGIN {
            return;
        }
        if self.control_allows(ctx, id, connector) && self.can_enter(ctx, id, connector) {
            self.grant(ctx.roads, id, connector);
        } else {
            self.start_waiting(ctx.roads, id, connector);
        }
    }

    fn can_enter(&mut self, ctx: &mut TrafficCtx, id: VehicleId, connector_id: LaneId) -> bool {
        if !self.conflicts_clear(ctx.roads, id, connector_id)
            || !self.gap_accepted(ctx, id, connector_id)
        {
            return false;
        }
        if !self.crosswalks_clear(ctx, id, connector_id) {
            return false;
        }
        if !self.exit_has_room(ctx.roads, connector_id, id, 0.0) {
            return false;
        }
        let roads = ctx.roads;
        let connector = lane(roads, connector_id);
        let vehicle = &self.vehicles[&id];
        let since = if vehicle.requested_connector == Some(connector_id) {
            vehicle.waiting_since
        } else {
            self.time
        };
        let waiting = self
            .waiting
            .get(&connector.from_node_id)
            .cloned()
            .unwrap_or_default();
        for other_id in waiting {
            let other = &self.vehicles[&other_id];
            if other_id == id || other.waiting_since >= since {
                continue;
            }
            let wanted_id = other.requested_connector.expect("waiting for a turn");
            let wanted = lane(roads, wanted_id);
            // Space in a nearly full exit lane goes to whoever has waited for it longest,
            // even if they can't take it this moment (their light is red): otherwise
            // traffic that happens to have green when space frees up takes it every time,
            // and they starve.
            if wanted.outgoing_lane_ids[0] == connector.outgoing_lane_ids[0]
                && other.waiting_since < since - EXIT_FAIRNESS_WAIT
                && !self.exit_has_room(roads, connector_id, id, MAX_LENGTH + MIN_GAP)
            {
                return false;
            }
            // Let an earlier arrival go first, unless it couldn't go anyway.
            if !connector.conflicting_lane_ids.contains(&wanted_id) {
                continue;
            }
            if self.control_allows(ctx, other_id, wanted_id)
                && self.conflicts_clear(roads, other_id, wanted_id)
                && self.gap_accepted(ctx, other_id, wanted_id)
                && self.exit_has_room(roads, wanted_id, other_id, 0.0)
            {
                return false;
            }
        }
        true
    }

    /// Gap acceptance: drivers don't only avoid vehicles already in the junction, they
    /// also give way to traffic that has priority over them (`must_yield`) and is coming
    /// toward the shared stretch: the front vehicle on each road feeding such a movement,
    /// heading for it (and with a light that lets it), must be at least CRITICAL_GAP
    /// seconds away; the gap needed depends on the turn (crossing oncoming traffic to turn
    /// left needs the most). A priority driver already waiting at its stop line counts
    /// too: it is about to go. The longer a driver has waited, the smaller the gap they
    /// accept, down to none, so a side street isn't starved for ever and a ring of drivers
    /// each yielding to the next (four cars at a four-way junction) breaks up, falling back
    /// on first come, first served.
    fn gap_accepted(&mut self, ctx: &mut TrafficCtx, id: VehicleId, connector_id: LaneId) -> bool {
        let roads = ctx.roads;
        let connector = lane(roads, connector_id);
        let vehicle = &self.vehicles[&id];
        let mut gap = critical_gap(connector.turn);
        if vehicle.requested_connector == Some(connector_id) {
            gap *= 1.0 - ((self.time - vehicle.waiting_since) / GAP_SHRINK_TIME).clamp(0.0, 1.0);
        }
        if gap <= 0.0 {
            return true;
        }
        for &other_connector_id in &connector.conflicting_lane_ids {
            let other_connector = &roads.lanes[&other_connector_id];
            let Some(feeder) = other_connector
                .feeder_lane_id
                .and_then(|id| roads.lanes.get(&id))
            else {
                continue;
            };
            let Some(&other_id) = self.queue(feeder.id).first() else {
                continue;
            };
            let other = &self.vehicles[&other_id];
            if other_id == id || other.granted_connector.is_some() {
                continue;
            }
            if other.next_lanes.first() != Some(&other_connector_id) {
                continue;
            }
            if !Self::yields_to(&mut self.yields, roads, connector, other_connector)
                || !self.control_allows(ctx, other_id, other_connector_id)
            {
                continue;
            }
            let other = &self.vehicles[&other_id];
            let distance = feeder.length - other.distance - other.length * 0.5
                + f64::from(other_connector.conflict_zones[&connector_id].x);
            if Self::time_to_cover(distance, other.speed, feeder.speed_limit) < gap {
                return false;
            }
        }
        true
    }

    /// `must_yield`, cached per connector pair (it only depends on the junction's
    /// geometry).
    fn yields_to(
        cache: &mut HashMap<(LaneId, LaneId), bool>,
        roads: &RoadNetwork,
        mine: &Lane,
        theirs: &Lane,
    ) -> bool {
        *cache
            .entry((mine.id, theirs.id))
            .or_insert_with(|| Self::must_yield(roads, mine, theirs))
    }

    /// Does a driver taking `mine` give way to one taking `theirs`? A driver coming from a
    /// bigger road (`RoadType::priority`) goes first; between equal roads, going straight
    /// beats turning right, which beats turning left (or U-turning) across traffic;
    /// between equal movements, priority to the right. The relation is never mutual, so
    /// two drivers can't each wait for the other.
    pub fn must_yield(roads: &RoadNetwork, mine: &Lane, theirs: &Lane) -> bool {
        let my_road = Self::road_priority(roads, mine);
        let their_road = Self::road_priority(roads, theirs);
        if my_road != their_road {
            return my_road < their_road;
        }
        let my_rank = yield_rank(mine.turn);
        let their_rank = yield_rank(theirs.turn);
        if my_rank != their_rank {
            return my_rank > their_rank;
        }
        // Traffic from my right crosses my heading right to left: with y growing downward,
        // that is a negative cross product.
        let my_heading = lane_builder::start_direction(mine.curve());
        let their_heading = lane_builder::start_direction(theirs.curve());
        my_heading.gd_cross(their_heading) < -0.5
    }

    /// Can `id` enter `connector` now without meeting anyone where paths cross or merge?
    /// Every vehicle on, or allowed onto, a conflicting connector must be predicted to have
    /// left the shared stretch (rear past its end) CONFLICT_MARGIN before this vehicle's
    /// front could reach it, even if this vehicle accelerated flat out and the other one
    /// kept its current speed. So a waiting vehicle starts as soon as the way will be clear
    /// by the time it gets there, rather than waiting for the junction to empty.
    fn conflicts_clear(&self, roads: &RoadNetwork, id: VehicleId, connector_id: LaneId) -> bool {
        let vehicle = &self.vehicles[&id];
        let current = lane(roads, vehicle.lane.expect("on the road"));
        let connector = lane(roads, connector_id);
        let to_line = (current.length - vehicle.distance - vehicle.length * 0.5).max(0.0);
        let top_speed = current.speed_limit.max(connector.speed_limit);
        for other_id in &connector.conflicting_lane_ids {
            let Some(users) = self.connector_vehicles.get(other_id) else {
                continue;
            };
            if users.is_empty() {
                continue;
            }
            let my_zone = connector.conflict_zones[other_id];
            let arrival =
                Self::time_to_cover(to_line + f64::from(my_zone.x), vehicle.speed, top_speed);
            for user in users {
                let other = &self.vehicles[user];
                let other_lane = lane(roads, other.lane.expect("on the road"));
                let on_it = other_lane.id == *other_id;
                let other_connector = if on_it {
                    other_lane
                } else {
                    lane(roads, other.granted_connector.expect("allowed in"))
                };
                let their_zone = other_connector.conflict_zones[&connector_id];
                // Position along the other connector; negative while still approaching it.
                let along = if on_it {
                    other.distance
                } else {
                    other.distance - other_lane.length
                };
                let rear = along - other.length * 0.5;
                if rear >= f64::from(their_zone.y) {
                    continue;
                }
                if other.speed < MIN_PREDICT_SPEED {
                    return false;
                }
                if (f64::from(their_zone.y) - rear) / other.speed + CONFLICT_MARGIN > arrival {
                    return false;
                }
            }
        }
        true
    }

    /// Shortest time to drive `distance` starting at `speed`, accelerating up to
    /// `top_speed`.
    pub(super) fn time_to_cover(distance: f64, speed: f64, top_speed: f64) -> f64 {
        if distance <= 0.0 {
            return 0.0;
        }
        let speed = speed.min(top_speed);
        let accelerating_time = (top_speed - speed) / ACCELERATION;
        let accelerating_distance = (speed + top_speed) * 0.5 * accelerating_time;
        if distance <= accelerating_distance {
            return ((speed * speed + 2.0 * ACCELERATION * distance).sqrt() - speed) / ACCELERATION;
        }
        accelerating_time + (distance - accelerating_distance) / top_speed
    }

    /// What the junction's control says: at traffic lights, green for that turn. A driver
    /// turning (not going straight) who has waited SNEAK_WAIT at the stop line may also go
    /// on amber, or in the first SNEAK_GRACE after the light turned red: the end of the
    /// green is when the crosswalk and oncoming traffic clear, so real turners "sneak"
    /// through then rather than wait for ever behind a busy crossing (the other traffic,
    /// just let go, waits for them: `conflicts_clear`).
    pub(super) fn control_allows(
        &self,
        ctx: &TrafficCtx,
        id: VehicleId,
        connector_id: LaneId,
    ) -> bool {
        let connector = lane(ctx.roads, connector_id);
        let Some(node) = ctx.roads.nodes.get(&connector.from_node_id) else {
            return true;
        };
        if !node.has_lights() {
            return true;
        }
        if ctx.signals.light(node, connector.signal_phase) == Light::Green {
            return true;
        }
        let vehicle = &self.vehicles[&id];
        connector.turn != Turn::Straight
            && vehicle.requested_connector == Some(connector_id)
            && self.time - vehicle.waiting_since >= SNEAK_WAIT
            && ctx
                .signals
                .recently_ended(node, connector.signal_phase, SNEAK_GRACE)
    }

    /// Is there room for `id` (plus `extra` px) at the start of the lane this connector
    /// leads to, counting vehicles already allowed in but not there yet (assumed to be the
    /// longest kind)?
    fn exit_has_room(
        &self,
        roads: &RoadNetwork,
        connector_id: LaneId,
        id: VehicleId,
        extra: f64,
    ) -> bool {
        let exit_lane = lane(roads, lane(roads, connector_id).outgoing_lane_ids[0]);
        let queue = self.queue(exit_lane.id);
        let reserved = f64::from(
            self.reserved_entries
                .get(&exit_lane.id)
                .copied()
                .unwrap_or(0),
        );
        let needed = self.vehicles[&id].length + MIN_GAP + extra;
        if exit_lane.length < needed {
            // Too short to ever fit a whole vehicle: allow one at a time.
            return queue.is_empty() && reserved == 0.0;
        }
        let free = match queue.last() {
            Some(last) => {
                let back = &self.vehicles[last];
                back.distance - back.length * 0.5
            }
            None => exit_lane.length,
        };
        free - reserved * (MAX_LENGTH + MIN_GAP) >= needed
    }

    fn grant(&mut self, roads: &RoadNetwork, id: VehicleId, connector: LaneId) {
        self.stop_waiting(id);
        self.entry_counter += 1;
        let vehicle = &mut self.vehicles[&id];
        vehicle.entry_order = self.entry_counter;
        vehicle.granted_connector = Some(connector);
        self.hold_connector(roads, id, connector);
    }

    pub(super) fn start_waiting(&mut self, roads: &RoadNetwork, id: VehicleId, connector: LaneId) {
        if self.vehicles[&id].requested_connector == Some(connector) {
            return;
        }
        self.stop_waiting(id);
        let time = self.time;
        let vehicle = &mut self.vehicles[&id];
        vehicle.requested_connector = Some(connector);
        vehicle.waiting_since = time;
        let node = lane(roads, connector).from_node_id;
        self.waiting.entry(node).or_default().push(id);
    }

    /// (The junction a vehicle waits at is the node its requested connector belongs to,
    /// remembered here by searching the waiting lists, since the connector may have been
    /// retired: Godot reads `from_node_id` off the old connector object.)
    pub(super) fn stop_waiting(&mut self, id: VehicleId) {
        let vehicle = &mut self.vehicles[&id];
        if vehicle.requested_connector.is_none() {
            return;
        }
        vehicle.requested_connector = None;
        vehicle.waiting_since = f64::INFINITY;
        let node = self
            .waiting
            .iter()
            .find(|(_, waiting)| waiting.contains(&id))
            .map(|(node, _)| *node);
        if let Some(node) = node {
            let waiting = &mut self.waiting[&node];
            if let Some(i) = waiting.iter().position(|&other| other == id) {
                waiting.remove(i);
            }
            if waiting.is_empty() {
                self.waiting.shift_remove(&node);
            }
        }
    }

    /// Fastest a vehicle should be going as it enters `connector`: the turn's own limit,
    /// and at a real junction (crossing movements) without lights, the speed of a driver
    /// looking both ways (LOOK_SPEED_FRACTION). Past the stop line it speeds up again.
    /// Drivers on the junction's priority road (the biggest road there, if another is
    /// smaller) don't slow down: the side roads give way to them. Cached per connector.
    pub(super) fn entry_speed_cached(
        cache: &mut HashMap<LaneId, f64>,
        roads: &RoadNetwork,
        connector: &Lane,
    ) -> f64 {
        if let Some(&speed) = cache.get(&connector.id) {
            return speed;
        }
        let mut speed = connector.speed_limit;
        let node = roads.nodes.get(&connector.from_node_id);
        if !connector.conflicting_lane_ids.is_empty()
            && let Some(node) = node
            && !node.has_lights()
            && !Self::on_priority_road(roads, node, connector)
        {
            let feeder = connector.feeder_lane_id.and_then(|id| roads.lanes.get(&id));
            let road_limit = feeder.map_or(connector.speed_limit, |feeder| feeder.speed_limit);
            speed = connector.speed_limit.min(road_limit * LOOK_SPEED_FRACTION);
        }
        cache.insert(connector.id, speed);
        speed
    }

    fn on_priority_road(
        roads: &RoadNetwork,
        node: &crate::roads::RoadNode,
        connector: &Lane,
    ) -> bool {
        let mine = Self::road_priority(roads, connector);
        let mut lowest = mine;
        for segment_id in &node.segment_ids {
            let priority = roads.segments[segment_id].road_type.priority;
            if priority > mine {
                return false;
            }
            lowest = lowest.min(priority);
        }
        lowest < mine
    }

    /// Priority of the road a connector's traffic comes from.
    fn road_priority(roads: &RoadNetwork, connector: &Lane) -> i32 {
        let Some(feeder) = connector.feeder_lane_id.and_then(|id| roads.lanes.get(&id)) else {
            return 0;
        };
        match feeder.segment_id.and_then(|id| roads.segments.get(&id)) {
            Some(segment) => segment.road_type.priority,
            None => 0,
        }
    }
}
