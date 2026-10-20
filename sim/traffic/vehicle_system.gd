class_name VehicleSystem
extends RefCounted
## Moves vehicles along their routes each tick: accelerate toward the lane's speed
## limit, brake in time for slower lanes ahead (turns) and for the final stop, and hop
## from lane to lane. After any road or lot change every vehicle is re-routed from where
## it is; a vehicle that can no longer reach its destination fails its trip.
## (No car-following yet: vehicles drive through each other until Phase 2b.)

signal vehicle_arrived(vehicle: Vehicle)
signal vehicle_failed(vehicle: Vehicle)

const ACCELERATION := 60.0  ## px/s²
const DECELERATION := 100.0  ## px/s²
## Vehicles never crawl slower than this, so they always reach their stop.
const MIN_SPEED := 4.0
const ARRIVAL_EPSILON := 0.5
## Extra look-ahead beyond braking distance when checking upcoming lanes.
const LOOKAHEAD_MARGIN := 20.0
## How far a vehicle may be from a rebuilt lane to be moved onto it.
const RESNAP_DISTANCE := 2.0

var vehicles: Dictionary[int, Vehicle] = {}

var _roads: RoadNetwork
var _router: Router
var _lots: LotSystem
var _next_id := 1
var _reroute_pending := false


func _init(roads: RoadNetwork, router: Router, lots: LotSystem) -> void:
	_roads = roads
	_router = router
	_lots = lots
	# Bound methods, not lambdas: a lambda capturing self would form a reference cycle
	# through the signal and leak the whole simulation.
	_roads.changed.connect(_on_roads_changed)
	_lots.changed.connect(_request_reroute)


func spawn(route: Route, destination_lot_id: int, owner_id: int) -> Vehicle:
	var vehicle := Vehicle.new()
	vehicle.id = _next_id
	_next_id += 1
	vehicle.owner_id = owner_id
	vehicle.destination_lot_id = destination_lot_id
	vehicle.lane = _roads.lanes[route.lane_ids[0]]
	vehicle.distance = route.start_distance
	vehicle.next_lane_ids = _to_int_array(route.lane_ids.slice(1))
	vehicle.end_distance = route.end_distance
	_update_pose(vehicle)
	vehicle.previous_position = vehicle.position
	vehicle.previous_rotation = vehicle.rotation
	vehicles[vehicle.id] = vehicle
	return vehicle


func _on_roads_changed(_node_ids: Array, _segment_ids: Array) -> void:
	_reroute_pending = true


func _request_reroute() -> void:
	_reroute_pending = true


## Removes a vehicle without reporting an arrival or failure (e.g. its owner is gone).
func remove(vehicle_id: int) -> void:
	vehicles.erase(vehicle_id)


func step(dt: float) -> void:
	if _reroute_pending:
		_reroute_pending = false
		_reroute_all()
	var arrived: Array[Vehicle] = []
	for vehicle: Vehicle in vehicles.values():
		vehicle.previous_position = vehicle.position
		vehicle.previous_rotation = vehicle.rotation
		_update_speed(vehicle, dt)
		if _advance(vehicle, vehicle.speed * dt):
			arrived.append(vehicle)
		_update_pose(vehicle)
	for vehicle in arrived:
		vehicles.erase(vehicle.id)
		vehicle_arrived.emit(vehicle)


## The fastest speed from which the vehicle can still brake down to every upcoming
## lane's limit (and to zero at its stop) in time: v = sqrt(v_limit² + 2·decel·gap).
## The gap is measured from where the vehicle will be after this tick's move, otherwise
## braking starts one tick late and turns are entered too fast.
func _update_speed(vehicle: Vehicle, dt: float) -> void:
	var ahead := vehicle.get_remaining_on_lane() - vehicle.speed * dt
	var target := vehicle.lane.speed_limit
	if vehicle.is_on_last_lane():
		target = minf(target, sqrt(2.0 * DECELERATION * maxf(ahead, 0.0)))
	else:
		var horizon := vehicle.speed * vehicle.speed / (2.0 * DECELERATION) + LOOKAHEAD_MARGIN
		var last_index := vehicle.next_lane_ids.size() - 1
		for i in vehicle.next_lane_ids.size():
			if ahead > horizon:
				break
			var next: Lane = _roads.lanes.get(vehicle.next_lane_ids[i])
			if next == null:
				break
			target = minf(target, sqrt(next.speed_limit * next.speed_limit + 2.0 * DECELERATION * maxf(ahead, 0.0)))
			if i == last_index:
				target = minf(target, sqrt(2.0 * DECELERATION * maxf(ahead + vehicle.end_distance, 0.0)))
			ahead += next.length
	target = maxf(target, MIN_SPEED)
	var rate := ACCELERATION if vehicle.speed < target else DECELERATION
	vehicle.speed = move_toward(vehicle.speed, target, rate * dt)


## Moves the vehicle `move` px along its route. Returns true when it reached its stop.
func _advance(vehicle: Vehicle, move: float) -> bool:
	while true:
		var remaining := vehicle.get_remaining_on_lane()
		if vehicle.is_on_last_lane() and remaining <= ARRIVAL_EPSILON + move:
			vehicle.distance = vehicle.end_distance
			return true
		if move < remaining:
			vehicle.distance += move
			return false
		move -= maxf(remaining, 0.0)
		vehicle.lane = _roads.lanes[vehicle.next_lane_ids.pop_front()]
		vehicle.distance = 0.0
	return false


func _update_pose(vehicle: Vehicle) -> void:
	var pose := vehicle.lane.curve.sample_baked_with_rotation(clampf(vehicle.distance, 0.0, vehicle.lane.length))
	vehicle.position = pose.origin
	vehicle.rotation = pose.get_rotation()


func _reroute_all() -> void:
	for vehicle: Vehicle in vehicles.values():
		if not _reroute(vehicle):
			vehicles.erase(vehicle.id)
			vehicle_failed.emit(vehicle)


## Finds a fresh route to the destination lot from where the vehicle is now.
func _reroute(vehicle: Vehicle) -> bool:
	var destination := _lots.get_access(vehicle.destination_lot_id)
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
				vehicle.next_lane_ids = _to_int_array(route.lane_ids)
				vehicle.end_distance = route.end_distance
				return true
		# Its exit was rebuilt too: try the new connector now under it.
		if _resnap(vehicle):
			return _route_from_current_lane(vehicle, destination)
	return false


func _route_from_current_lane(vehicle: Vehicle, destination: Dictionary) -> bool:
	var route := _router.find_route(vehicle.lane.id, vehicle.distance, destination.lane_id, destination.distance)
	if route == null:
		return false
	vehicle.next_lane_ids = _to_int_array(route.lane_ids.slice(1))
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


static func _to_int_array(ids: PackedInt64Array) -> Array[int]:
	var result: Array[int] = []
	for lane_id in ids:
		result.append(lane_id)
	return result
