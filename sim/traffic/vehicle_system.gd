class_name VehicleSystem
extends RefCounted
## Moves vehicles along their routes each tick, in two passes:
## 1. Prepare: for every vehicle, find the gap to whatever is ahead: the vehicle in front
##    on its lane or, for the front-most vehicle, along the next lanes of its route, and
##    any junction it hasn't been allowed into yet (a stop line). Vehicles reaching a stop
##    line ask to enter their junction.
## 2. Move: pick a speed (lane limits, slowing for turns and for the stop, following the
##    vehicle ahead) and move, never closer than MIN_GAP to the vehicle ahead and never
##    past a stop line. Gaps come from the start of the tick and vehicles only move
##    forward, so no two vehicles can overlap.
##
## Junctions. Conflicting connectors (paths that cross or merge) only share a short
## stretch, their conflict zone. A vehicle at a stop line may enter its connector if:
## - the junction's traffic light, if it has one, is green for its lane,
## - everyone on or allowed onto a conflicting connector will have left the shared zone
##   before this vehicle could reach it (_conflicts_clear), so movements that don't meet
##   flow in parallel and a waiting vehicle starts before the junction is empty,
## - traffic it must give way to (_must_yield) isn't coming too close (_gap_accepted),
## - the lane it leads to has room for the whole vehicle ("don't block the box"),
## - no vehicle that has waited longer at the same junction, and could go, wants a
##   conflicting move.
## At junctions without lights, drivers also slow down to look before the stop line
## (_entry_speed). Entry, once granted, is never taken back. Inside the junction, a vehicle still stops
## short of a conflict zone that an earlier entrant hasn't cleared (_yield_in_junction),
## in case the timing prediction was off.
##
## After any road or lot change every vehicle re-routes from where it is; a vehicle that
## can no longer reach its destination fails its trip.

signal vehicle_arrived(vehicle: Vehicle)
signal vehicle_failed(vehicle: Vehicle)

const ACCELERATION := 60.0  ## px/s²
## Comfortable braking, used to plan speeds ahead of turns, stops and queues.
const DECELERATION := 100.0  ## px/s²
## Hardest braking a vehicle may actually do when the plan changes suddenly.
const MAX_DECELERATION := 300.0  ## px/s²
## Bumper-to-bumper distance kept to the vehicle ahead when stopped.
const MIN_GAP := 4.0
## Extra following distance per unit of speed, in seconds.
const TIME_HEADWAY := 0.8
## Vehicles never crawl slower than this toward their own stop, so they always reach it.
const MIN_SPEED := 4.0
const ARRIVAL_EPSILON := 0.5
## Extra look-ahead beyond braking distance.
const LOOKAHEAD_MARGIN := 20.0
## How far a vehicle may be from a rebuilt lane to be moved onto it.
const RESNAP_DISTANCE := 2.0
## Below this a vehicle counts as stopped (for stuck detection).
const STOPPED_SPEED := 1.0
## Below this fraction of the lane's limit a vehicle counts as held up (for re-routing).
const SLOW_FRACTION := 0.3
## Patience. A trip's yardstick is its free-flow time plus a little for starting and
## stopping; past IMPATIENT_FACTOR x that + IMPATIENT_GRACE the driver is impatient (red),
## past GIVE_UP_FACTOR x that + GIVE_UP_GRACE they give up and the trip fails.
const START_STOP_ALLOWANCE := 2.0  ## Sim seconds.
const IMPATIENT_FACTOR := 2.0
const IMPATIENT_GRACE := 15.0
const GIVE_UP_FACTOR := 4.0
const GIVE_UP_GRACE := 60.0
## Safety margin between one vehicle leaving a conflict zone and the next reaching it.
const CONFLICT_MARGIN := 0.5  ## Sim seconds.
## Below this speed a vehicle's progress through a junction can't be predicted.
const MIN_PREDICT_SPEED := 2.0
## Driveways (_pull_out, _final_speed): speed when turning off the road into a lot, how
## far back from the curb a waiting vehicle is drawn, the width kept clear when crossing
## a lane, and how long a vehicle waits to turn across the road before traffic makes room
## (COURTESY_WAIT) or it gives up and goes another way (DRIVEWAY_GIVE_UP).
## A vehicle that has waited this much longer for the same nearly full exit lane gets
## the next space in it (see _can_enter).
const EXIT_FAIRNESS_WAIT := 5.0
const DRIVEWAY_SPEED := 15.0
const TRUCK_DRIVEWAY_SPEED := 10.0
const DRIVEWAY_SETBACK := 6.0
const CROSSING_HALF_WIDTH := 8.0
const COURTESY_WAIT := 3.0
const DRIVEWAY_GIVE_UP := 8.0
## Waiting on the road to turn in holds up the traffic behind, so drivers give up sooner.
const TURN_IN_GIVE_UP := 5.0
## Lane changes (_change_lanes): a vehicle that still has to change lanes stops this far
## short of its lane's end, and within MERGE_DISTANCE of it (or once stopped) it counts
## as merging, so the traffic beside it makes room.
const LANE_CHANGE_END_MARGIN := 2.0
const MERGE_DISTANCE := 60.0
## A merging vehicle stopped at the end of its lane this long gives up the lane change,
## goes on from the lane it's in, and re-routes (sim seconds).
const MISSED_LANE_WAIT := 2.0
## How fast a vehicle that changed lanes is drawn sliding across (px per sim second).
const LANE_CHANGE_SLIDE_SPEED := 20.0
## Gap acceptance (_gap_accepted): how far away, in seconds, approaching conflicting
## traffic must be before a driver enters, by turn. (Vehicles here drive about 2.5x
## faster in car lengths per second than real ones, so these are below real-world gaps.)
const CRITICAL_GAP := {
	Lane.Turn.LEFT: 2.5,
	Lane.Turn.U_TURN: 2.5,
	Lane.Turn.STRAIGHT: 2.0,
	Lane.Turn.RIGHT: 1.5,
}
## Who gives way at a junction (_must_yield): higher ranks yield to lower ones.
const YIELD_RANK := {
	Lane.Turn.STRAIGHT: 0,
	Lane.Turn.RIGHT: 1,
	Lane.Turn.LEFT: 2,
	Lane.Turn.U_TURN: 2,
}
## A waiting driver accepts ever smaller gaps, down to none after this long.
const GAP_SHRINK_TIME := 10.0  ## Sim seconds.
## At a junction without lights, drivers slow to this fraction of the speed limit by the
## stop line to look both ways, even when the junction is empty.
const LOOK_SPEED_FRACTION := 0.4

var vehicles: Dictionary[int, Vehicle] = {}

var _roads: RoadNetwork
var _router: Router
var _lots: LotSystem
var _signals: TrafficLights
var _next_id := 1
var _reroute_pending := false
var _time := 0.0
## Lanes with at least one vehicle (their queues live on Lane.queue, front-most first).
var _occupied_lanes: Dictionary[int, Lane] = {}
## Connector id -> vehicles on it or allowed onto it.
var _connector_vehicles: Dictionary[int, Array] = {}
## Lane id -> vehicles allowed through a junction into this lane that haven't reached it.
var _reserved_entries: Dictionary[int, int] = {}
## Junction node id -> vehicles waiting at its stop lines.
var _waiting: Dictionary[int, Array] = {}
## Increases with every junction entry granted; orders vehicles for _yield_in_junction.
var _entry_counter := 0
## Vehicles waiting in driveways to pull out (not on the road yet), oldest first.
var _departures: Array[Vehicle] = []
## Connector id -> { conflicting connector id -> must give way to it } (_must_yield, cached;
## connector ids change whenever their junction is rebuilt, so cleared on road changes).
var _yields: Dictionary[int, Dictionary] = {}
## Connector id -> _entry_speed (cached the same way).
var _entry_speeds: Dictionary[int, float] = {}


func _init(roads: RoadNetwork, router: Router, lots: LotSystem, signals: TrafficLights) -> void:
	_roads = roads
	_signals = signals
	_router = router
	_lots = lots
	# Bound methods, not lambdas: a lambda capturing self would form a reference cycle
	# through the signal and leak the whole simulation.
	_roads.changed.connect(_on_roads_changed)
	_lots.changed.connect(_request_reroute)


# --- Public API -------------------------------------------------------------

## Puts a new vehicle at the start of `route` right away (debug traffic, tests). Returns
## null if there's no room there. `destination_lot_id` may be -1 for trips to a plain
## lane position. City trips use depart() instead.
func spawn(route: Route, destination_lot_id: int, owner_id: int, kind := Vehicle.Kind.CAR) -> Vehicle:
	var lane: Lane = _roads.lanes.get(route.lane_ids[0])
	var vehicle := _new_vehicle(owner_id, kind, destination_lot_id, route)
	if lane == null or not can_spawn_at(lane.id, route.start_distance, vehicle.length):
		return null
	_put_on_road(vehicle, route)
	return vehicle


## Starts a trip from a driveway. The vehicle waits off the road, at its lot, until it
## can pull out into a gap (see _pull_out): `origin_lot_id`'s driveway, or (-1) the lane
## positions `origin_points`, e.g. a highway entering the map. It goes to
## `destination_lot_id`'s driveway, or (-1) the quickest of `destination_points`.
## Returns null if there's no way there. The trip's clock starts now: waiting to pull
## out counts.
func depart(origin_lot_id: int, origin_points: Array[Dictionary], destination_lot_id: int,
		destination_points: Array[Dictionary], owner_id: int, kind := Vehicle.Kind.CAR) -> Vehicle:
	if origin_lot_id != -1:
		origin_points = _lots.get_access_points(origin_lot_id)
	if destination_lot_id != -1:
		destination_points = _lots.get_access_points(destination_lot_id)
	var route := _router.find_best_route(origin_points, destination_points)
	if route == null:
		return null
	var vehicle := _new_vehicle(owner_id, kind, destination_lot_id, route)
	vehicle.origin_lot_id = origin_lot_id
	vehicle.origin_points = origin_points
	vehicle.driveway_wait_since = _time
	_plan_departure(vehicle, route)
	_departures.append(vehicle)
	_place_waiting_departures()
	return vehicle


## Vehicles waiting in driveways to pull out, in the order they set off.
func get_waiting_departures() -> Array[Vehicle]:
	return _departures


func _new_vehicle(owner_id: int, kind: Vehicle.Kind, destination_lot_id: int, route: Route) -> Vehicle:
	var vehicle := Vehicle.new()
	vehicle.set_kind(kind)
	vehicle.id = _next_id
	_next_id += 1
	vehicle.owner_id = owner_id
	vehicle.destination_lot_id = destination_lot_id
	vehicle.destination_lane_id = route.lane_ids[-1]
	vehicle.destination_distance = route.end_distance
	vehicle.trip_start_time = _time
	vehicle.ideal_trip_time = route.get_free_flow_time(_roads) + START_STOP_ALLOWANCE
	return vehicle


func _put_on_road(vehicle: Vehicle, route: Route) -> void:
	vehicle.lane = _roads.lanes[route.lane_ids[0]]
	vehicle.distance = route.start_distance
	vehicle.next_lanes = _to_lanes(route.lane_ids.slice(1))
	vehicle.end_distance = route.end_distance
	vehicle.route = null
	vehicle.crossing_lanes.clear()
	vehicle.crossing_distances.clear()
	vehicle.arrival_checked = false
	vehicle.driveway_wait_since = INF
	var drawn_at := vehicle.position
	var drawn_rotation := vehicle.rotation
	_update_pose(vehicle)
	vehicle.previous_position = vehicle.position if vehicle.origin_lot_id == -1 else drawn_at
	vehicle.previous_rotation = vehicle.rotation if vehicle.origin_lot_id == -1 else drawn_rotation
	vehicles[vehicle.id] = vehicle
	_enqueue(vehicle)


## True if a vehicle could appear at `distance` along the lane without touching another
## one, and with enough room for the vehicle behind to brake.
func can_spawn_at(lane_id: int, distance: float, length := Vehicle.MAX_LENGTH) -> bool:
	var lane: Lane = _roads.lanes.get(lane_id)
	if lane == null:
		return false
	for other: Vehicle in lane.queue:
		var ahead_by := other.distance - distance
		var needed := (other.length + length) * 0.5 + MIN_GAP
		if ahead_by < 0.0:
			needed += other.speed * other.speed / (2.0 * MAX_DECELERATION)
		if absf(ahead_by) < needed:
			return false
	return true


## Removes a vehicle without reporting an arrival or failure (e.g. its owner is gone).
func remove(vehicle_id: int) -> void:
	var vehicle: Vehicle = vehicles.get(vehicle_id)
	if vehicle != null:
		_detach(vehicle)
		vehicles.erase(vehicle_id)
		return
	for waiting in _departures:
		if waiting.id == vehicle_id:
			_departures.erase(waiting)
			return


## Drops every vehicle and breaks the vehicle <-> lane reference cycles. Call before
## discarding the simulation.
func dispose() -> void:
	for vehicle: Vehicle in vehicles.values():
		vehicle.lane.queue.clear()
		vehicle.next_lanes.clear()
		vehicle.granted_connector = null
		vehicle.requested_connector = null
	vehicles.clear()
	_departures.clear()
	_occupied_lanes.clear()
	_connector_vehicles.clear()
	_waiting.clear()
	_yields.clear()
	_entry_speeds.clear()


## Lanes that currently have vehicles on them.
func get_occupied_lanes() -> Array:
	return _occupied_lanes.values()


## Number of vehicles that haven't moved for at least `seconds`.
func get_stuck_count(seconds := 30.0) -> int:
	var count := 0
	for vehicle: Vehicle in vehicles.values():
		if vehicle.stopped_time >= seconds:
			count += 1
	return count


## Vehicles on a lane, front-most first (read-only view for debugging and tests).
func get_lane_queue(lane_id: int) -> Array:
	var lane: Lane = _roads.lanes.get(lane_id)
	return lane.queue if lane != null else []


func step(dt: float) -> void:
	_time += dt
	if _reroute_pending:
		_reroute_pending = false
		_reroute_all()
	_pull_out()
	_change_lanes()
	_prepare()
	_make_room_for_mergers()
	_keep_crossings_clear()
	var arrived: Array[Vehicle] = []
	var gave_up: Array[Vehicle] = []
	for vehicle: Vehicle in vehicles.values():
		vehicle.previous_position = vehicle.position
		vehicle.previous_rotation = vehicle.rotation
		vehicle.lateral_offset = move_toward(vehicle.lateral_offset, 0.0, LANE_CHANGE_SLIDE_SPEED * dt)
		_update_speed(vehicle, dt)
		if _advance(vehicle, dt):
			arrived.append(vehicle)
		_update_pose(vehicle)
		vehicle.stopped_time = vehicle.stopped_time + dt if vehicle.speed < STOPPED_SPEED else 0.0
		var slow := vehicle.speed < SLOW_FRACTION * vehicle.lane.speed_limit
		vehicle.slow_time = vehicle.slow_time + dt if slow else 0.0
		var elapsed := _time - vehicle.trip_start_time
		if elapsed > vehicle.ideal_trip_time * IMPATIENT_FACTOR + IMPATIENT_GRACE:
			vehicle.impatient = true
			if elapsed > vehicle.ideal_trip_time * GIVE_UP_FACTOR + GIVE_UP_GRACE:
				gave_up.append(vehicle)
	for vehicle in arrived:
		_detach(vehicle)
		vehicles.erase(vehicle.id)
		vehicle.trip_end_time = _time
		vehicle_arrived.emit(vehicle)
	for vehicle in gave_up:
		if vehicles.has(vehicle.id):
			vehicle.gave_up = true
			_fail(vehicle)


# --- Driveways --------------------------------------------------------------------

## Each lot's first waiting vehicle pulls out when the lane it joins has a gap: nobody
## alongside, and traffic coming up behind at least a critical gap away (a right turn's
## gap to join the near lane; a left turn's to cross to the far one, where the lanes it
## crosses need a gap too). It joins at driveway speed, so the traffic behind it brakes.
## A vehicle that has waited DRIVEWAY_GIVE_UP to cross to the far lane turns onto the
## near one instead and finds its way from there.
func _pull_out() -> void:
	if _departures.is_empty():
		return
	var tried := {}
	var i := 0
	while i < _departures.size():
		var vehicle := _departures[i]
		var key := vehicle.origin_lot_id if vehicle.origin_lot_id != -1 else -1 - vehicle.id
		if tried.has(key):
			i += 1
			continue
		tried[key] = true
		if _can_pull_out(vehicle):
			_departures.remove_at(i)
			_put_on_road(vehicle, vehicle.route)
			continue
		if not vehicle.crossing_lanes.is_empty() and _time - vehicle.driveway_wait_since > DRIVEWAY_GIVE_UP:
			var near_side := _router.find_best_route([vehicle.origin_points[0]], _get_destination(vehicle))
			if near_side != null:
				_plan_departure(vehicle, near_side)
		i += 1
	_place_waiting_departures()


## Sets the route a waiting vehicle will pull out onto, and the lanes it must cross first
## if that starts on the far side of the road.
func _plan_departure(vehicle: Vehicle, route: Route) -> void:
	vehicle.route = route
	vehicle.destination_lane_id = route.lane_ids[-1]
	vehicle.destination_distance = route.end_distance
	vehicle.crossing_lanes.clear()
	vehicle.crossing_distances.clear()
	if vehicle.origin_lot_id != -1:
		_add_crossing(vehicle, vehicle.origin_lot_id, _roads.lanes[route.lane_ids[0]])


## If `lane` is on the far side of the road from `lot_id`, the lanes between (those on
## the lot's side) become the vehicle's crossing lanes.
func _add_crossing(vehicle: Vehicle, lot_id: int, lane: Lane) -> void:
	var near := _lots.get_access(lot_id)
	if near.is_empty():
		return
	var near_lane: Lane = _roads.lanes.get(near.lane_id)
	if near_lane == null or lane == near_lane or lane.is_beside(near_lane):
		return
	for lane_id in _roads.segments[near_lane.segment_id].lane_ids:
		var other := _roads.lanes[lane_id]
		if other == near_lane or other.is_beside(near_lane):
			vehicle.crossing_lanes.append(other)
			vehicle.crossing_distances.append(near.distance)


func _can_pull_out(vehicle: Vehicle) -> bool:
	var lane: Lane = _roads.lanes.get(vehicle.route.lane_ids[0])
	if lane == null:
		return false
	var turn := Lane.Turn.RIGHT if vehicle.crossing_lanes.is_empty() else Lane.Turn.LEFT
	var gap := _waited_gap(CRITICAL_GAP[turn], vehicle.driveway_wait_since)
	if not _clear_to_join(lane, vehicle.route.start_distance, vehicle.length, gap):
		return false
	for i in vehicle.crossing_lanes.size():
		if not _clear_to_cross(vehicle.crossing_lanes[i], vehicle.crossing_distances[i], gap):
			return false
	return true


## The gap a driver still insists on after waiting since `since` (it shrinks to half over
## GAP_SHRINK_TIME, like at junctions, but never to nothing: pulling out is never forced).
func _waited_gap(gap: float, since: float) -> float:
	return gap * (1.0 - 0.5 * clampf((_time - since) / GAP_SHRINK_TIME, 0.0, 1.0))


## Room to join `lane` at `at` at driveway speed: nobody alongside, nothing coming up from
## behind within `gap` seconds, and not where vehicles let out of the junction will arrive.
func _clear_to_join(lane: Lane, at: float, length: float, gap: float) -> bool:
	var promised: int = _reserved_entries.get(lane.id, 0)
	if at - length * 0.5 < MIN_GAP + promised * (Vehicle.MAX_LENGTH + MIN_GAP):
		return false
	for other: Vehicle in lane.queue:
		var ahead_by := other.distance - at
		var clearance := absf(ahead_by) - (other.length + length) * 0.5 - MIN_GAP
		if clearance < 0.0:
			return false
		if ahead_by < 0.0 and other.speed > STOPPED_SPEED:
			if clearance / other.speed < gap or other.speed * other.speed / (2.0 * DECELERATION) > clearance:
				return false
	return true


## Room to drive across `lane` at `at`: nobody on the crossing point, and nothing coming
## within `gap` seconds.
func _clear_to_cross(lane: Lane, at: float, gap: float) -> bool:
	for other: Vehicle in lane.queue:
		var ahead_by := other.distance - at
		if absf(ahead_by) < other.length * 0.5 + CROSSING_HALF_WIDTH:
			return false
		if ahead_by < 0.0 and other.speed > STOPPED_SPEED:
			if (-ahead_by - other.length * 0.5 - CROSSING_HALF_WIDTH) / other.speed < gap:
				return false
	return true


## Where waiting vehicles are drawn: on their lot, nose to the road, one behind the other.
func _place_waiting_departures() -> void:
	var waiting_at := {}
	for vehicle in _departures:
		var near := _lots.get_access(vehicle.origin_lot_id) if vehicle.origin_lot_id != -1 else {}
		if near.is_empty():
			var start: Lane = _roads.lanes.get(vehicle.route.lane_ids[0])
			if start != null:
				var pose := start.sample_pose(vehicle.route.start_distance)
				vehicle.position = pose.origin
				vehicle.rotation = pose.get_rotation()
			continue
		var curb := _roads.lanes[near.lane_id].sample_pose(near.distance).origin
		var lot_center := _lots.lots[vehicle.origin_lot_id].get_center()
		var out := (curb - lot_center).normalized()
		var place: int = waiting_at.get(vehicle.origin_lot_id, 0)
		waiting_at[vehicle.origin_lot_id] = place + 1
		var back := DRIVEWAY_SETBACK + vehicle.length * 0.5 + place * (Vehicle.MAX_LENGTH + MIN_GAP)
		vehicle.position = curb - out * back
		vehicle.rotation = out.angle()
		vehicle.previous_position = vehicle.position
		vehicle.previous_rotation = vehicle.rotation


## How fast the vehicle should be going when it reaches the end of its trip: driveway
## speed, or a stop if it has to wait to turn across the road.
func _final_speed(vehicle: Vehicle) -> float:
	if _must_cross_to_arrive(vehicle):
		return 0.0
	return TRUCK_DRIVEWAY_SPEED if vehicle.kind == Vehicle.Kind.TRUCK else DRIVEWAY_SPEED


## Is the vehicle's driveway on the far side of the road from the lane its trip ends on?
## (Worked out once per route.)
func _must_cross_to_arrive(vehicle: Vehicle) -> bool:
	if not vehicle.arrival_checked:
		vehicle.arrival_checked = true
		vehicle.crossing_lanes.clear()
		vehicle.crossing_distances.clear()
		if vehicle.destination_lot_id != -1 and _lots.lots.has(vehicle.destination_lot_id):
			var final_lane := vehicle.lane if vehicle.next_lanes.is_empty() else vehicle.next_lanes[-1]
			_add_crossing(vehicle, vehicle.destination_lot_id, final_lane)
	return not vehicle.crossing_lanes.is_empty()


func _can_turn_in(vehicle: Vehicle) -> bool:
	# Unlike pulling out, a driver waiting on the road to turn in shrinks the gap they need
	# all the way, like at a junction: they are holding everyone up.
	var since := vehicle.driveway_wait_since if vehicle.driveway_wait_since < INF else _time
	var gap: float = CRITICAL_GAP[Lane.Turn.LEFT] * (1.0 - clampf((_time - since) / GAP_SHRINK_TIME, 0.0, 1.0))
	for i in vehicle.crossing_lanes.size():
		if not _clear_to_cross(vehicle.crossing_lanes[i], vehicle.crossing_distances[i], gap):
			return false
	return true


## A vehicle that has waited TURN_IN_GIVE_UP to turn across the road into its driveway
## drives on and comes back round to it from the near side.
func _give_up_turning_in(vehicle: Vehicle) -> void:
	var near := _lots.get_access(vehicle.destination_lot_id)
	var route := _router.find_route_onward(vehicle.lane.id, [near]) if not near.is_empty() else null
	if route == null:
		return
	vehicle.next_lanes = _to_lanes(route.lane_ids)
	vehicle.end_distance = route.end_distance
	vehicle.arrival_checked = false
	vehicle.driveway_wait_since = INF


## Traffic lets waiting vehicles out and in: once one has waited COURTESY_WAIT, the next
## vehicle on each lane it must join or cross stops short of it, if it can comfortably.
func _keep_crossings_clear() -> void:
	for vehicle: Vehicle in vehicles.values():
		if vehicle.driveway_wait_since == INF:
			continue
		if _time - vehicle.driveway_wait_since > TURN_IN_GIVE_UP:
			_give_up_turning_in(vehicle)
			continue
		if _time - vehicle.driveway_wait_since > COURTESY_WAIT:
			_hold_back_for_crossing(vehicle)
	# The first vehicle waiting at each lot is let out the same way.
	var seen := {}
	for vehicle in _departures:
		var key := vehicle.origin_lot_id if vehicle.origin_lot_id != -1 else -1 - vehicle.id
		if seen.has(key) or _time - vehicle.driveway_wait_since <= COURTESY_WAIT:
			seen[key] = true
			continue
		seen[key] = true
		_hold_back_for_crossing(vehicle)
		var join: Lane = _roads.lanes.get(vehicle.route.lane_ids[0])
		if join != null:
			_hold_back(join, vehicle.route.start_distance - vehicle.length * 0.5 - MIN_GAP)


func _hold_back_for_crossing(vehicle: Vehicle) -> void:
	for i in vehicle.crossing_lanes.size():
		_hold_back(vehicle.crossing_lanes[i], vehicle.crossing_distances[i] - CROSSING_HALF_WIDTH)


## The first vehicle on `lane` short of `stop_at` stops there, if it can comfortably.
func _hold_back(lane: Lane, stop_at: float) -> void:
	for other: Vehicle in lane.queue:
		if other.distance >= stop_at:
			continue
		var gap := stop_at - other.distance - other.length * 0.5
		if other.speed * other.speed / (2.0 * DECELERATION) + MIN_GAP <= gap and gap < other.leader_gap:
			other.leader_gap = gap
			other.leader_speed = 0.0
		break


# --- Lane changes -----------------------------------------------------------------

## Vehicles whose route continues on the lane beside theirs (Lane.is_beside) move across
## as soon as there is a safe gap there: room for the whole vehicle, and the vehicle that
## ends up behind can brake to its speed comfortably. Until then they drive on, and stop
## at the end of their lane if they must (_scan_ahead), where they count as merging and
## the traffic beside makes room (_make_room_for_mergers).
func _change_lanes() -> void:
	for vehicle: Vehicle in vehicles.values():
		vehicle.merging = false
		if vehicle.next_lanes.is_empty() or not vehicle.next_lanes[0].is_beside(vehicle.lane):
			continue
		var target := vehicle.next_lanes[0]
		var target_distance := _distance_beside(vehicle, target)
		if _has_room_to_change(vehicle, target, target_distance):
			_move_across(vehicle, target, target_distance)
			continue
		var partner := _swap_partner(vehicle, target, target_distance)
		if partner != null:
			var partner_distance := _distance_beside(partner, vehicle.lane)
			var lane := vehicle.lane
			_move_across(vehicle, target, target_distance)
			_move_across(partner, lane, partner_distance)
		elif vehicle.get_remaining_on_lane() < MERGE_DISTANCE or vehicle.speed < STOPPED_SPEED:
			vehicle.merging = true
			if vehicle.stopped_time > MISSED_LANE_WAIT and vehicle.lane.queue[0] == vehicle \
					and vehicle.get_remaining_on_lane() < vehicle.length + MERGE_DISTANCE:
				_carry_on_without_changing(vehicle)


## Two vehicles side by side, each wanting the other's lane (common in jams: a left
## turner in the right lane next to a right turner in the left one), swap places when
## neither would then be too close to anyone else. Returns the vehicle to swap with.
func _swap_partner(vehicle: Vehicle, target: Lane, at: float) -> Vehicle:
	for other: Vehicle in target.queue:
		if other.next_lanes.is_empty() or other.next_lanes[0] != vehicle.lane:
			continue
		if absf(other.distance - at) > (other.length + vehicle.length) * 0.5 + MIN_GAP:
			continue
		var other_at := _distance_beside(other, vehicle.lane)
		if _has_room_to_change(vehicle, target, at, other) and _has_room_to_change(other, vehicle.lane, other_at, vehicle):
			return other
	return null


## A driver who couldn't get into the lane they needed before the end of the road goes
## on through the junction from the lane they're in and finds a new way from there.
func _carry_on_without_changing(vehicle: Vehicle) -> void:
	var route := _router.find_route_onward(vehicle.lane.id, _get_destination(vehicle))
	if route == null:
		return
	vehicle.next_lanes = _to_lanes(route.lane_ids)
	vehicle.end_distance = route.end_distance
	vehicle.merging = false


## Where alongside `target` the vehicle is (lanes of a straight road line up exactly),
## kept far enough from its end for the whole vehicle to fit.
func _distance_beside(vehicle: Vehicle, target: Lane) -> float:
	var at := vehicle.distance
	if not (vehicle.lane.is_straight and target.is_straight):
		at = target.curve.get_closest_offset(vehicle.position)
	return clampf(at, 0.0, maxf(target.length - vehicle.length * 0.5, 0.0))


## `ignored`: a vehicle not to count (the one swapping places with this one).
func _has_room_to_change(vehicle: Vehicle, target: Lane, at: float, ignored: Vehicle = null) -> bool:
	var half := vehicle.length * 0.5
	# Not right at the start, where vehicles come out of the junction (and space is
	# promised to those let in), nor past the end.
	var promised: int = _reserved_entries.get(target.id, 0)
	if at - half < MIN_GAP + promised * (Vehicle.MAX_LENGTH + MIN_GAP) or at + half > target.length:
		return false
	for other: Vehicle in target.queue:
		if other == ignored:
			continue
		var ahead_by := other.distance - at
		var needed := (other.length + vehicle.length) * 0.5 + MIN_GAP
		if ahead_by >= 0.0:
			# It goes in front of `other`: must be able to brake to its speed.
			needed += maxf(vehicle.speed * vehicle.speed - other.speed * other.speed, 0.0) / (2.0 * DECELERATION)
		else:
			# `other` ends up behind it and must be able to brake to its speed.
			needed += maxf(other.speed * other.speed - vehicle.speed * vehicle.speed, 0.0) / (2.0 * DECELERATION)
		if absf(ahead_by) < needed:
			return false
	return true


func _move_across(vehicle: Vehicle, target: Lane, at: float) -> void:
	var old_position := vehicle.position
	_dequeue(vehicle)
	vehicle.lane = target
	vehicle.distance = at
	vehicle.next_lanes.pop_front()
	_enqueue(vehicle)
	# Drawn where it was, sliding over to the new lane's center.
	var pose := target.sample_pose(at)
	vehicle.lateral_offset = (old_position - pose.origin).dot(_right_of(pose.x))
	_update_pose(vehicle)


## A vehicle on the target lane coming up behind a merging vehicle holds back to let it
## in, if it can stop comfortably short of it (otherwise the next one will).
func _make_room_for_mergers() -> void:
	for merger: Vehicle in vehicles.values():
		if not merger.merging:
			continue
		var target := merger.next_lanes[0]
		var at := _distance_beside(merger, target)
		for other: Vehicle in target.queue:
			if other.distance >= at:
				continue
			# The first vehicle behind the merge point.
			var gap := at - merger.length * 0.5 - other.distance - other.length * 0.5
			if other.speed * other.speed / (2.0 * DECELERATION) + MIN_GAP <= gap and gap < other.leader_gap:
				other.leader_gap = gap
				other.leader_speed = 0.0
			break


# --- Pass 1: what is ahead, junction requests ----------------------------------

func _prepare() -> void:
	for lane: Lane in _occupied_lanes.values():
		var queue := lane.queue
		for i in queue.size():
			var vehicle: Vehicle = queue[i]
			vehicle.stop_gap = INF
			if i > 0:
				var leader: Vehicle = queue[i - 1]
				vehicle.leader_gap = leader.distance - vehicle.distance - (leader.length + vehicle.length) * 0.5
				vehicle.leader_speed = leader.speed
			else:
				_request_junction(vehicle)
				_scan_ahead(vehicle)
				_yield_in_junction(vehicle)


## For the front-most vehicle on a lane: the nearest vehicle further along its route,
## and the first junction it hasn't been allowed into.
func _scan_ahead(vehicle: Vehicle) -> void:
	vehicle.leader_gap = INF
	vehicle.leader_speed = 0.0
	var ahead := vehicle.lane.length - vehicle.distance
	# A vehicle that just turned off this lane in another direction is still right ahead.
	for out in vehicle.lane.outgoing_lanes:
		var queue := out.queue
		if not queue.is_empty():
			_consider_leader(vehicle, ahead + queue.back().distance - (queue.back().length + vehicle.length) * 0.5, queue.back().speed)
	var horizon := vehicle.speed * vehicle.speed / (2.0 * DECELERATION) + LOOKAHEAD_MARGIN + vehicle.length
	if not vehicle.next_lanes.is_empty() and vehicle.next_lanes[0].is_beside(vehicle.lane):
		# Still has to change lanes: it can't go on past the end of this one.
		vehicle.stop_gap = ahead - vehicle.length * 0.5 - LANE_CHANGE_END_MARGIN
		return
	for next in vehicle.next_lanes:
		if ahead > horizon:
			return
		if next.is_connector() and next != vehicle.granted_connector:
			vehicle.stop_gap = ahead - vehicle.length * 0.5
			return
		var queue := next.queue
		if not queue.is_empty():
			_consider_leader(vehicle, ahead + queue.back().distance - (queue.back().length + vehicle.length) * 0.5, queue.back().speed)
			return
		ahead += next.length


func _consider_leader(vehicle: Vehicle, gap: float, speed: float) -> void:
	if gap < vehicle.leader_gap:
		vehicle.leader_gap = gap
		vehicle.leader_speed = speed


## Safety net under the timing prediction in _conflicts_clear: a vehicle never enters a
## conflict zone while a vehicle let into the junction before it is still in the matching
## zone; it stops short of the zone instead. Vehicles only ever wait for earlier entrants,
## so two vehicles can't end up waiting for each other.
func _yield_in_junction(vehicle: Vehicle) -> void:
	var on_connector := vehicle.lane.is_connector()
	var connector := vehicle.lane if on_connector else vehicle.granted_connector
	if connector == null:
		return
	var front := vehicle.distance + vehicle.length * 0.5
	if not on_connector:
		front -= vehicle.lane.length
	for other_id in connector.conflicting_lane_ids:
		var my_zone: Vector2 = connector.conflict_zones[other_id]
		if front >= my_zone.x:
			continue
		for other: Vehicle in _connector_vehicles.get(other_id, []):
			if other.entry_order >= vehicle.entry_order:
				continue
			var other_on_it := other.lane.id == other_id
			var other_connector := other.lane if other_on_it else other.granted_connector
			var along := other.distance if other_on_it else other.distance - other.lane.length
			if along - other.length * 0.5 < other_connector.conflict_zones[connector.id].y:
				vehicle.stop_gap = minf(vehicle.stop_gap, my_zone.x - front - 1.0)


## A front-most vehicle close enough to its next junction asks to enter it.
func _request_junction(vehicle: Vehicle) -> void:
	if vehicle.next_lanes.is_empty():
		return
	var connector := vehicle.next_lanes[0]
	if not connector.is_connector() or connector == vehicle.granted_connector:
		return
	var to_line := vehicle.lane.length - vehicle.distance - vehicle.length * 0.5
	if to_line > vehicle.speed * vehicle.speed / (2.0 * DECELERATION) + LOOKAHEAD_MARGIN:
		return
	if _control_allows(vehicle, connector) and _can_enter(vehicle, connector):
		_grant(vehicle, connector)
	else:
		_start_waiting(vehicle, connector)


func _can_enter(vehicle: Vehicle, connector: Lane) -> bool:
	if not _conflicts_clear(vehicle, connector) or not _gap_accepted(vehicle, connector):
		return false
	if not _exit_has_room(connector, vehicle):
		return false
	var since := vehicle.waiting_since if vehicle.requested_connector == connector else _time
	for other: Vehicle in _waiting.get(connector.from_node_id, []):
		if other == vehicle or other.waiting_since >= since:
			continue
		var wanted := other.requested_connector
		# Space in a nearly full exit lane goes to whoever has waited for it longest, even
		# if they can't take it this moment (their light is red): otherwise traffic that
		# happens to have green when space frees up takes it every time, and they starve.
		if wanted.outgoing_lanes[0] == connector.outgoing_lanes[0] \
				and other.waiting_since < since - EXIT_FAIRNESS_WAIT \
				and not _exit_has_room(connector, vehicle, Vehicle.MAX_LENGTH + MIN_GAP):
			return false
		# Let an earlier arrival go first, unless it couldn't go anyway.
		if not connector.conflicting_lane_ids.has(wanted.id):
			continue
		if _control_allows(other, wanted) and _conflicts_clear(other, wanted) \
				and _gap_accepted(other, wanted) and _exit_has_room(wanted, other):
			return false
	return true


## Gap acceptance: drivers don't only avoid vehicles already in the junction, they also
## give way to traffic that has priority over them (_must_yield) and is coming toward
## the shared stretch: the front vehicle on each road feeding such a movement, heading
## for it (and with a light that lets it), must be at least CRITICAL_GAP seconds away;
## the gap needed depends on the turn (crossing oncoming traffic to turn left needs the
## most). A priority driver already waiting at its stop line counts too: it is about to
## go. The longer a driver has waited, the smaller the gap they accept, down to none, so
## a side street isn't starved for ever and a ring of drivers each yielding to the next
## (four cars at a four-way junction) breaks up, falling back on first come, first served.
func _gap_accepted(vehicle: Vehicle, connector: Lane) -> bool:
	var gap: float = CRITICAL_GAP.get(connector.turn, CRITICAL_GAP[Lane.Turn.STRAIGHT])
	if vehicle.requested_connector == connector:
		gap *= 1.0 - clampf((_time - vehicle.waiting_since) / GAP_SHRINK_TIME, 0.0, 1.0)
	if gap <= 0.0:
		return true
	for other_id in connector.conflicting_lane_ids:
		var other_connector := _roads.lanes[other_id]
		var feeder: Lane = _roads.lanes.get(other_connector.feeder_lane_id)
		if feeder == null or feeder.queue.is_empty():
			continue
		var other: Vehicle = feeder.queue[0]
		if other == vehicle or other.granted_connector != null:
			continue
		if other.next_lanes.is_empty() or other.next_lanes[0] != other_connector:
			continue
		if not _yields_to(connector, other_connector) or not _control_allows(other, other_connector):
			continue
		var distance := feeder.length - other.distance - other.length * 0.5 + other_connector.conflict_zones[connector.id].x
		if _time_to_cover(distance, other.speed, feeder.speed_limit) < gap:
			return false
	return true


## _must_yield, cached per connector pair (it only depends on the junction's geometry).
func _yields_to(mine: Lane, theirs: Lane) -> bool:
	if not _yields.has(mine.id):
		_yields[mine.id] = {}
	var cache: Dictionary = _yields[mine.id]
	if not cache.has(theirs.id):
		cache[theirs.id] = _must_yield(mine, theirs)
	return cache[theirs.id]


## Does a driver taking `mine` give way to one taking `theirs`? A driver coming from a
## bigger road (RoadType.priority) goes first; between equal roads, going straight beats
## turning right, which beats turning left (or U-turning) across traffic; between equal
## movements, priority to the right. The relation is never mutual, so two drivers can't
## each wait for the other.
func _must_yield(mine: Lane, theirs: Lane) -> bool:
	var my_road := _road_priority(mine)
	var their_road := _road_priority(theirs)
	if my_road != their_road:
		return my_road < their_road
	var my_rank: int = YIELD_RANK.get(mine.turn, 0)
	var their_rank: int = YIELD_RANK.get(theirs.turn, 0)
	if my_rank != their_rank:
		return my_rank > their_rank
	# Traffic from my right crosses my heading right to left: with y growing downward,
	# that is a negative cross product.
	var my_heading := LaneBuilder.start_direction(mine.curve)
	var their_heading := LaneBuilder.start_direction(theirs.curve)
	return my_heading.cross(their_heading) < -0.5


## Can `vehicle` enter `connector` now without meeting anyone where paths cross or
## merge? Every vehicle on, or allowed onto, a conflicting connector must be predicted to
## have left the shared stretch (rear past its end) CONFLICT_MARGIN before this
## vehicle's front could reach it, even if this vehicle accelerated flat out and the
## other one kept its current speed. So a waiting vehicle starts as soon as the way will
## be clear by the time it gets there, rather than waiting for the junction to empty.
func _conflicts_clear(vehicle: Vehicle, connector: Lane) -> bool:
	var to_line := maxf(vehicle.lane.length - vehicle.distance - vehicle.length * 0.5, 0.0)
	var top_speed := maxf(vehicle.lane.speed_limit, connector.speed_limit)
	for other_id in connector.conflicting_lane_ids:
		var users: Array = _connector_vehicles.get(other_id, [])
		if users.is_empty():
			continue
		var my_zone: Vector2 = connector.conflict_zones[other_id]
		var arrival := _time_to_cover(to_line + my_zone.x, vehicle.speed, top_speed)
		for other: Vehicle in users:
			var on_it := other.lane.id == other_id
			var other_connector := other.lane if on_it else other.granted_connector
			var their_zone: Vector2 = other_connector.conflict_zones[connector.id]
			# Position along the other connector; negative while still approaching it.
			var along := other.distance if on_it else other.distance - other.lane.length
			var rear := along - other.length * 0.5
			if rear >= their_zone.y:
				continue
			if other.speed < MIN_PREDICT_SPEED:
				return false
			if (their_zone.y - rear) / other.speed + CONFLICT_MARGIN > arrival:
				return false
	return true


## Shortest time to drive `distance` starting at `speed`, accelerating up to `top_speed`.
static func _time_to_cover(distance: float, speed: float, top_speed: float) -> float:
	if distance <= 0.0:
		return 0.0
	speed = minf(speed, top_speed)
	var accelerating_time := (top_speed - speed) / ACCELERATION
	var accelerating_distance := (speed + top_speed) * 0.5 * accelerating_time
	if distance <= accelerating_distance:
		return (sqrt(speed * speed + 2.0 * ACCELERATION * distance) - speed) / ACCELERATION
	return accelerating_time + (distance - accelerating_distance) / top_speed


## What the junction's control says: at traffic lights, green for that turn.
func _control_allows(_vehicle: Vehicle, connector: Lane) -> bool:
	var node: RoadNode = _roads.nodes.get(connector.from_node_id)
	if node == null or not node.has_lights():
		return true
	return _signals.get_light(node, connector.signal_phase) == TrafficLights.Light.GREEN


## Is there room for `vehicle` (plus `extra` px) at the start of the lane this connector
## leads to, counting vehicles already allowed in but not there yet (assumed to be the
## longest kind)?
func _exit_has_room(connector: Lane, vehicle: Vehicle, extra := 0.0) -> bool:
	var exit_lane := connector.outgoing_lanes[0]
	var queue := exit_lane.queue
	var reserved: int = _reserved_entries.get(exit_lane.id, 0)
	var needed := vehicle.length + MIN_GAP + extra
	if exit_lane.length < needed:
		# Too short to ever fit a whole vehicle: allow one at a time.
		return queue.is_empty() and reserved == 0
	var free := exit_lane.length
	if not queue.is_empty():
		free = queue.back().distance - queue.back().length * 0.5
	return free - reserved * (Vehicle.MAX_LENGTH + MIN_GAP) >= needed


func _grant(vehicle: Vehicle, connector: Lane) -> void:
	_stop_waiting(vehicle)
	_entry_counter += 1
	vehicle.entry_order = _entry_counter
	vehicle.granted_connector = connector
	_hold_connector(vehicle, connector)


func _start_waiting(vehicle: Vehicle, connector: Lane) -> void:
	if vehicle.requested_connector == connector:
		return
	_stop_waiting(vehicle)
	vehicle.requested_connector = connector
	vehicle.waiting_since = _time
	if not _waiting.has(connector.from_node_id):
		_waiting[connector.from_node_id] = []
	_waiting[connector.from_node_id].append(vehicle)


func _stop_waiting(vehicle: Vehicle) -> void:
	if vehicle.requested_connector == null:
		return
	var node_id := vehicle.requested_connector.from_node_id
	var waiting: Array = _waiting.get(node_id, [])
	waiting.erase(vehicle)
	if waiting.is_empty():
		_waiting.erase(node_id)
	vehicle.requested_connector = null
	vehicle.waiting_since = INF


# --- Pass 2: speed and movement ------------------------------------------------

## The fastest speed from which the vehicle can still brake, comfortably, down to every
## upcoming lane's limit, to its stop, to a stop line, and behind the vehicle ahead:
## v = sqrt(v_then² + 2·decel·gap). Gaps are measured from where the vehicle will be
## after this tick's move, otherwise braking starts one tick late.
func _update_speed(vehicle: Vehicle, dt: float) -> void:
	var travel := vehicle.speed * dt
	var ahead := vehicle.get_remaining_on_lane() - travel
	var target := vehicle.lane.speed_limit
	# At its destination it slows to driveway speed and turns off the road, or stops if it
	# has to wait for a gap to turn across the road.
	var final_speed := _final_speed(vehicle)
	if vehicle.is_on_last_lane():
		target = minf(target, maxf(sqrt(final_speed * final_speed + 2.0 * DECELERATION * maxf(ahead, 0.0)), MIN_SPEED))
	else:
		var horizon := vehicle.speed * vehicle.speed / (2.0 * DECELERATION) + LOOKAHEAD_MARGIN
		var last_index := vehicle.next_lanes.size() - 1
		for i in vehicle.next_lanes.size():
			if ahead > horizon:
				break
			var next := vehicle.next_lanes[i]
			var previous := vehicle.lane if i == 0 else vehicle.next_lanes[i - 1]
			if next.is_beside(previous):
				# A lane change: the lane beside starts level with the previous one's start.
				ahead -= previous.length
			else:
				var entry_speed := _entry_speed(next) if next.is_connector() else next.speed_limit
				target = minf(target, sqrt(entry_speed * entry_speed + 2.0 * DECELERATION * maxf(ahead, 0.0)))
			if i == last_index:
				var stop := sqrt(final_speed * final_speed + 2.0 * DECELERATION * maxf(ahead + vehicle.end_distance, 0.0))
				target = minf(target, maxf(stop, MIN_SPEED))
			ahead += next.length
	if vehicle.stop_gap < INF:
		target = minf(target, sqrt(2.0 * DECELERATION * maxf(vehicle.stop_gap - travel, 0.0)))
	if vehicle.leader_gap < INF:
		var gap := maxf(vehicle.leader_gap - MIN_GAP - travel, 0.0)
		var leader_speed := vehicle.leader_speed
		target = minf(target, sqrt(leader_speed * leader_speed + 2.0 * DECELERATION * gap))
		target = minf(target, leader_speed + gap / TIME_HEADWAY)
	var rate := ACCELERATION if vehicle.speed < target else MAX_DECELERATION
	vehicle.speed = move_toward(vehicle.speed, target, rate * dt)


## Fastest a vehicle should be going as it enters `connector`: the turn's own limit, and at
## a real junction (crossing movements) without lights, the speed of a driver looking both
## ways (LOOK_SPEED_FRACTION). Past the stop line it speeds up again.
## Drivers on the junction's priority road (the biggest road there, if another is smaller)
## don't slow down: the side roads give way to them. Cached per connector.
func _entry_speed(connector: Lane) -> float:
	if _entry_speeds.has(connector.id):
		return _entry_speeds[connector.id]
	var speed := connector.speed_limit
	var node: RoadNode = _roads.nodes.get(connector.from_node_id)
	if not connector.conflicting_lane_ids.is_empty() and node != null and not node.has_lights() \
			and not _on_priority_road(node, connector):
		var feeder: Lane = _roads.lanes.get(connector.feeder_lane_id)
		var road_limit := feeder.speed_limit if feeder != null else connector.speed_limit
		speed = minf(connector.speed_limit, road_limit * LOOK_SPEED_FRACTION)
	_entry_speeds[connector.id] = speed
	return speed


func _on_priority_road(node: RoadNode, connector: Lane) -> bool:
	var mine := _road_priority(connector)
	var lowest := mine
	for segment_id in node.segment_ids:
		var priority := _roads.segments[segment_id].road_type.priority
		if priority > mine:
			return false
		lowest = mini(lowest, priority)
	return lowest < mine


## Priority of the road a connector's traffic comes from.
func _road_priority(connector: Lane) -> int:
	var feeder: Lane = _roads.lanes.get(connector.feeder_lane_id)
	if feeder == null or not _roads.segments.has(feeder.segment_id):
		return 0
	return _roads.segments[feeder.segment_id].road_type.priority


## Moves the vehicle for one tick. Returns true when it reached its stop.
func _advance(vehicle: Vehicle, dt: float) -> bool:
	var move := vehicle.speed * dt
	var limit := minf(vehicle.leader_gap - MIN_GAP, vehicle.stop_gap)
	if move > limit:
		move = maxf(limit, 0.0)
		vehicle.speed = move / dt
	while true:
		var remaining := vehicle.get_remaining_on_lane()
		if vehicle.is_on_last_lane() and remaining <= ARRIVAL_EPSILON + move:
			if _must_cross_to_arrive(vehicle) and not _can_turn_in(vehicle):
				# Waiting for a gap to turn across the road into its driveway, holding up the
				# traffic behind it. (Only as far as its move allows: it stays on the road.)
				vehicle.distance += minf(move, maxf(remaining, 0.0))
				vehicle.speed = 0.0
				if vehicle.driveway_wait_since == INF:
					vehicle.driveway_wait_since = _time
				return false
			vehicle.distance = vehicle.end_distance
			return true
		if move < remaining:
			vehicle.distance += move
			return false
		if vehicle.next_lanes[0].is_beside(vehicle.lane):
			# Out of road before it could change lanes (stop_gap should prevent this).
			vehicle.distance = vehicle.lane.length
			vehicle.speed = 0.0
			return false
		move -= maxf(remaining, 0.0)
		_hop(vehicle, vehicle.next_lanes.pop_front())
	return false


## Moves the vehicle from the end of its lane onto the start of `next`.
func _hop(vehicle: Vehicle, next: Lane) -> void:
	_dequeue(vehicle)
	if vehicle.lane.is_connector():
		_release_connector(vehicle, vehicle.lane)
	if next == vehicle.granted_connector:
		# The grant becomes "using the connector": counts stay until it leaves.
		vehicle.granted_connector = null
	vehicle.lane = next
	vehicle.distance = 0.0
	_enqueue(vehicle)


func _update_pose(vehicle: Vehicle) -> void:
	var pose := vehicle.lane.sample_pose(vehicle.distance)
	vehicle.position = pose.origin
	vehicle.rotation = pose.get_rotation()
	if vehicle.lateral_offset != 0.0:
		vehicle.position += _right_of(pose.x) * vehicle.lateral_offset


## Unit vector to the right of a direction (y grows downward).
static func _right_of(direction: Vector2) -> Vector2:
	return Vector2(-direction.y, direction.x).normalized()


# --- Bookkeeping ---------------------------------------------------------------

## Inserts the vehicle into its lane's queue, keeping front-most first.
func _enqueue(vehicle: Vehicle) -> void:
	var queue := vehicle.lane.queue
	if queue.is_empty():
		_occupied_lanes[vehicle.lane.id] = vehicle.lane
	var index := queue.size()
	while index > 0 and queue[index - 1].distance < vehicle.distance:
		index -= 1
	queue.insert(index, vehicle)


func _dequeue(vehicle: Vehicle) -> void:
	var queue := vehicle.lane.queue
	if not queue.is_empty() and queue[0] == vehicle:
		queue.pop_front()
	else:
		queue.erase(vehicle)
	if queue.is_empty():
		_occupied_lanes.erase(vehicle.lane.id)


## Forgets everything the vehicle holds: its place in a queue, a connector it's on or was
## allowed into, a junction it's waiting at.
func _detach(vehicle: Vehicle) -> void:
	_dequeue(vehicle)
	if vehicle.lane.is_connector():
		_release_connector(vehicle, vehicle.lane)
	_release_grant(vehicle)
	_stop_waiting(vehicle)


## Records the vehicle as using (or about to use) the connector, and as heading into the
## lane after it.
func _hold_connector(vehicle: Vehicle, connector: Lane) -> void:
	if not _connector_vehicles.has(connector.id):
		_connector_vehicles[connector.id] = []
	_connector_vehicles[connector.id].append(vehicle)
	_increment(_reserved_entries, connector.outgoing_lane_ids[0])


func _release_connector(vehicle: Vehicle, connector: Lane) -> void:
	var users: Array = _connector_vehicles.get(connector.id, [])
	users.erase(vehicle)
	if users.is_empty():
		_connector_vehicles.erase(connector.id)
	_decrement(_reserved_entries, connector.outgoing_lane_ids[0])


func _release_grant(vehicle: Vehicle) -> void:
	if vehicle.granted_connector != null:
		_release_connector(vehicle, vehicle.granted_connector)
		vehicle.granted_connector = null


static func _increment(counts: Dictionary, key: int) -> void:
	counts[key] = counts.get(key, 0) + 1


static func _decrement(counts: Dictionary, key: int) -> void:
	var value: int = counts.get(key, 0) - 1
	if value > 0:
		counts[key] = value
	else:
		counts.erase(key)


# --- Re-routing after network changes -----------------------------------------------

func _on_roads_changed(_node_ids: Array, _segment_ids: Array) -> void:
	_reroute_pending = true
	_yields.clear()
	_entry_speeds.clear()


func _request_reroute() -> void:
	_reroute_pending = true


func _reroute_all() -> void:
	for vehicle: Vehicle in vehicles.values():
		_reroute_vehicle(vehicle)
	# Waiting in driveways: plan again from the lot (or fail if there is no way now).
	for vehicle: Vehicle in _departures.duplicate():
		var points := _lots.get_access_points(vehicle.origin_lot_id) if vehicle.origin_lot_id != -1 else vehicle.origin_points
		var route := _router.find_best_route(points, _get_destination(vehicle))
		if route == null:
			_departures.erase(vehicle)
			vehicle.trip_end_time = _time
			vehicle_failed.emit(vehicle)
		else:
			vehicle.origin_points = points
			_plan_departure(vehicle, route)


## Vehicles that have been held up (crawling) for at least `min_slow_time`, up to
## `max_count` of them.
func find_delayed(max_count: int, min_slow_time := 3.0) -> Array[Vehicle]:
	var found: Array[Vehicle] = []
	for vehicle: Vehicle in vehicles.values():
		if found.size() >= max_count:
			break
		if vehicle.slow_time >= min_slow_time:
			found.append(vehicle)
	return found


## Lets a held-up vehicle look for a better route (with the latest congestion costs).
## Skipped if the vehicle is gone or already allowed into a junction (it's committed).
func reroute_delayed_vehicle(vehicle: Vehicle) -> void:
	if vehicles.get(vehicle.id) != vehicle or vehicle.granted_connector != null:
		return
	vehicle.slow_time = 0.0
	_reroute_vehicle(vehicle)


## Re-routes one vehicle from where it is, fixing up its queue and junction bookkeeping;
## a vehicle that can't reach its destination any more fails its trip.
func _reroute_vehicle(vehicle: Vehicle) -> void:
	var waited_for := vehicle.requested_connector
	var waiting_since := vehicle.waiting_since
	_stop_waiting(vehicle)
	var old_lane := vehicle.lane
	var ok := _reroute(vehicle)
	if ok and not vehicle.next_lanes.is_empty() and vehicle.next_lanes[0] == waited_for \
			and _roads.lanes.get(waited_for.id) == waited_for:
		# Still waiting for the same turn: keep its place (and patience) in the line.
		_start_waiting(vehicle, waited_for)
		vehicle.waiting_since = waiting_since
	if vehicle.lane != old_lane:
		# Re-snapped onto a rebuilt lane: move it to that lane's queue.
		var current := vehicle.lane
		vehicle.lane = old_lane
		_dequeue(vehicle)
		if old_lane.is_connector():
			_release_connector(vehicle, old_lane)
		vehicle.lane = current
		_enqueue(vehicle)
		if current.is_connector():
			_hold_connector(vehicle, current)
	# A grant for a junction no longer next on the route (or no longer existing) is void.
	var granted := vehicle.granted_connector
	if granted != null and (not ok or vehicle.next_lanes.is_empty()
			or vehicle.next_lanes[0] != granted or _roads.lanes.get(granted.id) != granted):
		_release_grant(vehicle)
	if not ok:
		_fail(vehicle)


## Ends a vehicle's trip unsuccessfully.
func _fail(vehicle: Vehicle) -> void:
	_detach(vehicle)
	vehicles.erase(vehicle.id)
	vehicle.trip_end_time = _time
	vehicle_failed.emit(vehicle)


## Finds a fresh route to the destination from where the vehicle is now.
func _reroute(vehicle: Vehicle) -> bool:
	var destination := _get_destination(vehicle)
	if destination.is_empty():
		return false
	var lane_gone: bool = _roads.lanes.get(vehicle.lane.id) != vehicle.lane
	if lane_gone and not vehicle.lane.is_connector() and not _resnap(vehicle):
		return false
	if _roads.lanes.get(vehicle.lane.id) == vehicle.lane:
		return _route_from_current_lane(vehicle, destination)
	# On a connector that was just replaced: finish it, then continue from the lane it feeds.
	if vehicle.lane.is_connector():
		var exit_lane_id: int = vehicle.lane.outgoing_lane_ids[0]
		if _roads.lanes.has(exit_lane_id):
			var route := _router.find_best_route([{ "lane_id": exit_lane_id, "distance": 0.0 }], destination)
			if route != null:
				vehicle.next_lanes = _to_lanes(route.lane_ids)
				vehicle.end_distance = route.end_distance
				return true
		# Its exit was rebuilt too: try the new connector now under it.
		if _resnap(vehicle):
			return _route_from_current_lane(vehicle, destination)
	return false


## Lane positions the vehicle may end its trip at: in front of its destination lot (either
## side of the road), else where its route was headed.
func _get_destination(vehicle: Vehicle) -> Array[Dictionary]:
	if vehicle.destination_lot_id != -1:
		return _lots.get_access_points(vehicle.destination_lot_id)
	var points: Array[Dictionary] = []
	if _roads.lanes.has(vehicle.destination_lane_id):
		points.append({ "lane_id": vehicle.destination_lane_id, "distance": vehicle.destination_distance })
	return points


func _route_from_current_lane(vehicle: Vehicle, destination: Array[Dictionary]) -> bool:
	var route := _router.find_best_route([{ "lane_id": vehicle.lane.id, "distance": vehicle.distance }], destination)
	if route == null:
		return false
	var starts_with_lane_change := route.lane_ids.size() > 1 and vehicle.lane.is_beside(_roads.lanes[route.lane_ids[1]])
	if starts_with_lane_change and vehicle.get_remaining_on_lane() < MERGE_DISTANCE:
		# Too late to change lanes here: go on from this lane instead, if possible.
		var onward := _router.find_route_onward(vehicle.lane.id, destination)
		if onward != null:
			vehicle.next_lanes = _to_lanes(onward.lane_ids)
			vehicle.end_distance = onward.end_distance
			return true
	vehicle.next_lanes = _to_lanes(route.lane_ids.slice(1))
	vehicle.end_distance = route.end_distance
	return true


## The road under the vehicle was rebuilt (e.g. split by a new crossing): move it onto
## whichever lane now runs right under it in the same direction, if any.
func _resnap(vehicle: Vehicle) -> bool:
	var heading := Vector2.from_angle(vehicle.rotation)
	for lane: Lane in _roads.lanes.values():
		var offset := lane.curve.get_closest_offset(vehicle.position)
		var pose := lane.curve.sample_baked_with_rotation(offset)
		if pose.origin.distance_to(vehicle.position) <= RESNAP_DISTANCE and pose.x.normalized().dot(heading) > 0.9:
			vehicle.lane = lane
			vehicle.distance = offset
			return true
	return false


func _to_lanes(ids: PackedInt64Array) -> Array[Lane]:
	var result: Array[Lane] = []
	for lane_id in ids:
		result.append(_roads.lanes[lane_id])
	return result
