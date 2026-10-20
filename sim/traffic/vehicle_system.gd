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
## Junctions (uncontrolled for now). A vehicle may enter a connector only if:
## - no connector crossing or merging with it is in use (or promised to someone),
## - the lane it leads to has room for the whole vehicle ("don't block the box"),
## - no vehicle that has waited longer at the same junction wants a conflicting move.
## Entry, once granted, is never taken back.
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

var vehicles: Dictionary[int, Vehicle] = {}

var _roads: RoadNetwork
var _router: Router
var _lots: LotSystem
var _next_id := 1
var _reroute_pending := false
var _time := 0.0
## Lanes with at least one vehicle (their queues live on Lane.queue, front-most first).
var _occupied_lanes: Dictionary[int, Lane] = {}
## Connector id -> number of vehicles on it or allowed onto it.
var _connector_users: Dictionary[int, int] = {}
## Lane id -> vehicles allowed through a junction into this lane that haven't reached it.
var _reserved_entries: Dictionary[int, int] = {}
## Junction node id -> vehicles waiting at its stop lines.
var _waiting: Dictionary[int, Array] = {}


func _init(roads: RoadNetwork, router: Router, lots: LotSystem) -> void:
	_roads = roads
	_router = router
	_lots = lots
	# Bound methods, not lambdas: a lambda capturing self would form a reference cycle
	# through the signal and leak the whole simulation.
	_roads.changed.connect(_on_roads_changed)
	_lots.changed.connect(_request_reroute)


# --- Public API -------------------------------------------------------------

## Puts a new vehicle at the start of `route`. Returns null if there's no room there.
## `destination_lot_id` may be -1 for trips to a plain lane position.
func spawn(route: Route, destination_lot_id: int, owner_id: int) -> Vehicle:
	var lane: Lane = _roads.lanes.get(route.lane_ids[0])
	if lane == null or not can_spawn_at(lane.id, route.start_distance):
		return null
	var vehicle := Vehicle.new()
	vehicle.id = _next_id
	_next_id += 1
	vehicle.owner_id = owner_id
	vehicle.destination_lot_id = destination_lot_id
	vehicle.destination_lane_id = route.lane_ids[-1]
	vehicle.destination_distance = route.end_distance
	vehicle.lane = lane
	vehicle.distance = route.start_distance
	vehicle.next_lanes = _to_lanes(route.lane_ids.slice(1))
	vehicle.end_distance = route.end_distance
	_update_pose(vehicle)
	vehicle.previous_position = vehicle.position
	vehicle.previous_rotation = vehicle.rotation
	vehicles[vehicle.id] = vehicle
	_enqueue(vehicle)
	return vehicle


## True if a vehicle could appear at `distance` along the lane without touching another
## one, and with enough room for the vehicle behind to brake.
func can_spawn_at(lane_id: int, distance: float) -> bool:
	var lane: Lane = _roads.lanes.get(lane_id)
	if lane == null:
		return false
	for other: Vehicle in lane.queue:
		var ahead_by := other.distance - distance
		var needed := Vehicle.LENGTH + MIN_GAP
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


## Drops every vehicle and breaks the vehicle <-> lane reference cycles. Call before
## discarding the simulation.
func dispose() -> void:
	for vehicle: Vehicle in vehicles.values():
		vehicle.lane.queue.clear()
		vehicle.next_lanes.clear()
		vehicle.granted_connector = null
		vehicle.requested_connector = null
	vehicles.clear()
	_occupied_lanes.clear()
	_waiting.clear()


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
	_prepare()
	var arrived: Array[Vehicle] = []
	for vehicle: Vehicle in vehicles.values():
		vehicle.previous_position = vehicle.position
		vehicle.previous_rotation = vehicle.rotation
		_update_speed(vehicle, dt)
		if _advance(vehicle, dt):
			arrived.append(vehicle)
		_update_pose(vehicle)
		vehicle.stopped_time = vehicle.stopped_time + dt if vehicle.speed < STOPPED_SPEED else 0.0
	for vehicle in arrived:
		_detach(vehicle)
		vehicles.erase(vehicle.id)
		vehicle_arrived.emit(vehicle)


# --- Pass 1: what is ahead, junction requests ----------------------------------

func _prepare() -> void:
	for lane: Lane in _occupied_lanes.values():
		var queue := lane.queue
		for i in queue.size():
			var vehicle: Vehicle = queue[i]
			vehicle.stop_gap = INF
			if i > 0:
				var leader: Vehicle = queue[i - 1]
				vehicle.leader_gap = leader.distance - vehicle.distance - Vehicle.LENGTH
				vehicle.leader_speed = leader.speed
			else:
				_request_junction(vehicle)
				_scan_ahead(vehicle)


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
			_consider_leader(vehicle, ahead + queue.back().distance - Vehicle.LENGTH, queue.back().speed)
	var horizon := vehicle.speed * vehicle.speed / (2.0 * DECELERATION) + LOOKAHEAD_MARGIN + Vehicle.LENGTH
	for next in vehicle.next_lanes:
		if ahead > horizon:
			return
		if next.is_connector() and next != vehicle.granted_connector:
			vehicle.stop_gap = ahead - Vehicle.LENGTH * 0.5
			return
		var queue := next.queue
		if not queue.is_empty():
			_consider_leader(vehicle, ahead + queue.back().distance - Vehicle.LENGTH, queue.back().speed)
			return
		ahead += next.length


func _consider_leader(vehicle: Vehicle, gap: float, speed: float) -> void:
	if gap < vehicle.leader_gap:
		vehicle.leader_gap = gap
		vehicle.leader_speed = speed


## A front-most vehicle close enough to its next junction asks to enter it.
func _request_junction(vehicle: Vehicle) -> void:
	if vehicle.next_lanes.is_empty():
		return
	var connector := vehicle.next_lanes[0]
	if not connector.is_connector() or connector == vehicle.granted_connector:
		return
	var to_line := vehicle.lane.length - vehicle.distance - Vehicle.LENGTH * 0.5
	if to_line > vehicle.speed * vehicle.speed / (2.0 * DECELERATION) + LOOKAHEAD_MARGIN:
		return
	if _can_enter(vehicle, connector):
		_grant(vehicle, connector)
	else:
		_start_waiting(vehicle, connector)


func _can_enter(vehicle: Vehicle, connector: Lane) -> bool:
	for other_id in connector.conflicting_lane_ids:
		if _connector_users.get(other_id, 0) > 0:
			return false
	if not _exit_has_room(connector):
		return false
	var since := vehicle.waiting_since if vehicle.requested_connector == connector else _time
	for other: Vehicle in _waiting.get(connector.from_node_id, []):
		if other == vehicle or other.waiting_since >= since:
			continue
		# Let an earlier arrival go first, unless it couldn't go anyway.
		var wanted := other.requested_connector
		if connector.conflicting_lane_ids.has(wanted.id) and _exit_has_room(wanted):
			return false
	return true


## Is there room for one more vehicle at the start of the lane this connector leads to,
## counting vehicles already allowed in but not there yet?
func _exit_has_room(connector: Lane) -> bool:
	var exit_lane := connector.outgoing_lanes[0]
	var queue := exit_lane.queue
	var reserved: int = _reserved_entries.get(exit_lane.id, 0)
	var needed := Vehicle.LENGTH + MIN_GAP
	if exit_lane.length < needed:
		# Too short to ever fit a whole vehicle: allow one at a time.
		return queue.is_empty() and reserved == 0
	var free := exit_lane.length
	if not queue.is_empty():
		free = queue.back().distance - Vehicle.LENGTH * 0.5
	return free - reserved * needed >= needed


func _grant(vehicle: Vehicle, connector: Lane) -> void:
	_stop_waiting(vehicle)
	vehicle.granted_connector = connector
	_increment(_connector_users, connector.id)
	_increment(_reserved_entries, connector.outgoing_lane_ids[0])


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
	if vehicle.is_on_last_lane():
		target = minf(target, maxf(sqrt(2.0 * DECELERATION * maxf(ahead, 0.0)), MIN_SPEED))
	else:
		var horizon := vehicle.speed * vehicle.speed / (2.0 * DECELERATION) + LOOKAHEAD_MARGIN
		var last_index := vehicle.next_lanes.size() - 1
		for i in vehicle.next_lanes.size():
			if ahead > horizon:
				break
			var next := vehicle.next_lanes[i]
			target = minf(target, sqrt(next.speed_limit * next.speed_limit + 2.0 * DECELERATION * maxf(ahead, 0.0)))
			if i == last_index:
				var stop := sqrt(2.0 * DECELERATION * maxf(ahead + vehicle.end_distance, 0.0))
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
			vehicle.distance = vehicle.end_distance
			return true
		if move < remaining:
			vehicle.distance += move
			return false
		move -= maxf(remaining, 0.0)
		_hop(vehicle, vehicle.next_lanes.pop_front())
	return false


## Moves the vehicle from the end of its lane onto the start of `next`.
func _hop(vehicle: Vehicle, next: Lane) -> void:
	_dequeue(vehicle)
	if vehicle.lane.is_connector():
		_release_connector(vehicle.lane)
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
		_release_connector(vehicle.lane)
	_release_grant(vehicle)
	_stop_waiting(vehicle)


func _release_connector(connector: Lane) -> void:
	_decrement(_connector_users, connector.id)
	_decrement(_reserved_entries, connector.outgoing_lane_ids[0])


func _release_grant(vehicle: Vehicle) -> void:
	if vehicle.granted_connector != null:
		_release_connector(vehicle.granted_connector)
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


func _request_reroute() -> void:
	_reroute_pending = true


func _reroute_all() -> void:
	for vehicle: Vehicle in vehicles.values():
		_stop_waiting(vehicle)
		var old_lane := vehicle.lane
		var ok := _reroute(vehicle)
		if vehicle.lane != old_lane:
			# Re-snapped onto a rebuilt lane: move it to that lane's queue.
			var current := vehicle.lane
			vehicle.lane = old_lane
			_dequeue(vehicle)
			if old_lane.is_connector():
				_release_connector(old_lane)
			vehicle.lane = current
			_enqueue(vehicle)
			if current.is_connector():
				_increment(_connector_users, current.id)
				_increment(_reserved_entries, current.outgoing_lane_ids[0])
		# A grant for a junction no longer next on the route (or no longer existing) is void.
		var granted := vehicle.granted_connector
		if granted != null and (not ok or vehicle.next_lanes.is_empty()
				or vehicle.next_lanes[0] != granted or _roads.lanes.get(granted.id) != granted):
			_release_grant(vehicle)
		if not ok:
			_detach(vehicle)
			vehicles.erase(vehicle.id)
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
			var route := _router.find_route(exit_lane_id, 0.0, destination.lane_id, destination.distance)
			if route != null:
				vehicle.next_lanes = _to_lanes(route.lane_ids)
				vehicle.end_distance = route.end_distance
				return true
		# Its exit was rebuilt too: try the new connector now under it.
		if _resnap(vehicle):
			return _route_from_current_lane(vehicle, destination)
	return false


func _get_destination(vehicle: Vehicle) -> Dictionary:
	if vehicle.destination_lot_id != -1:
		return _lots.get_access(vehicle.destination_lot_id)
	if _roads.lanes.has(vehicle.destination_lane_id):
		return { "lane_id": vehicle.destination_lane_id, "distance": vehicle.destination_distance }
	return {}


func _route_from_current_lane(vehicle: Vehicle, destination: Dictionary) -> bool:
	var route := _router.find_route(vehicle.lane.id, vehicle.distance, destination.lane_id, destination.distance)
	if route == null:
		return false
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
