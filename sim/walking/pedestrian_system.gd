class_name PedestrianSystem
extends RefCounted
## People walking: along sidewalks door to door, crossing roads only at crosswalks
## (WalkNetwork). They don't bump into each other; they meet cars only at crosswalks:
## - at the curb a pedestrian waits until no car is in the junction on a path over the
##   crosswalk (VehicleSystem.is_crosswalk_busy) and, at traffic lights, until the walk
##   light (green for the traffic running alongside, TrafficLights.is_walk_green) is on;
## - meanwhile cars give way to them: none enters a turn over a crosswalk with someone
##   on it, or waiting at it with the right to go (VehicleSystem._crosswalks_clear).
## Waiting pedestrians also call the walk phase at traffic lights.

signal pedestrian_arrived(pedestrian: Pedestrian)

## Walking pace (px per sim second): a sixth of a car on a local street, about the real
## ratio in town (5 km/h against 30).
const WALK_SPEED := 15.0
## Citizens walk trips up to this long (px) instead of driving.
const MAX_WALK_DISTANCE := 4.0 * Grid.CELL_SIZE
## Added to a walk's ideal time for setting off and arriving.
const START_STOP_ALLOWANCE := 1.0

var pedestrians: Dictionary[int, Pedestrian] = {}
var network: WalkNetwork
var crosswalks: Crosswalks

var _roads: RoadNetwork
var _lots: LotSystem
var _signals: TrafficLights
var _vehicles: VehicleSystem
var _next_id := 1
var _time := 0.0


func _init(roads: RoadNetwork, lots: LotSystem, signals: TrafficLights, vehicles: VehicleSystem,
		p_crosswalks: Crosswalks) -> void:
	_roads = roads
	_lots = lots
	_signals = signals
	_vehicles = vehicles
	crosswalks = p_crosswalks
	network = WalkNetwork.new(roads)


## Walking distance between two lots, or INF if there's no way on foot.
func get_walking_distance(from_lot_id: int, to_lot_id: int) -> float:
	if not (_lots.lots.has(from_lot_id) and _lots.lots.has(to_lot_id)):
		return INF
	return network.get_distance(_lots.lots[from_lot_id].get_center(), _lots.lots[to_lot_id].get_center())


## Starts walking from one lot to another. Returns null if there's no way on foot, or
## it is longer than `max_distance`.
func walk(from_lot_id: int, to_lot_id: int, owner_id: int, max_distance := INF) -> Pedestrian:
	if not (_lots.lots.has(from_lot_id) and _lots.lots.has(to_lot_id)):
		return null
	var pedestrian := walk_between(_lots.lots[from_lot_id].get_center(), _lots.lots[to_lot_id].get_center(), owner_id, max_distance)
	if pedestrian != null:
		pedestrian.destination_lot_id = to_lot_id
	return pedestrian


## Starts walking between two places (e.g. a door and a bus stop). Returns null if there's
## no way on foot, or it is longer than `max_distance`.
func walk_between(from: Vector2, to: Vector2, owner_id: int, max_distance := INF) -> Pedestrian:
	var path := network.find_path(from, to)
	if path.is_empty() or path.length > max_distance:
		return null
	var pedestrian := Pedestrian.new()
	pedestrian.id = _next_id
	_next_id += 1
	pedestrian.owner_id = owner_id
	pedestrian.points = path.points
	pedestrian.crossings = path.crossings
	pedestrian.position = path.points[0]
	pedestrian.trip_start_time = _time
	pedestrian.ideal_trip_time = path.length / WALK_SPEED + START_STOP_ALLOWANCE
	pedestrians[pedestrian.id] = pedestrian
	return pedestrian


## Removes a pedestrian without reporting an arrival (e.g. their home is gone).
func remove(pedestrian_id: int) -> void:
	var pedestrian: Pedestrian = pedestrians.get(pedestrian_id)
	if pedestrian != null:
		_stop_crossing(pedestrian)
		pedestrians.erase(pedestrian_id)


func step(time: float, dt: float) -> void:
	_time = time
	var arrived: Array[Pedestrian] = []
	for pedestrian: Pedestrian in pedestrians.values():
		if _walk(pedestrian, WALK_SPEED * dt):
			arrived.append(pedestrian)
	for pedestrian in arrived:
		pedestrians.erase(pedestrian.id)
		pedestrian.trip_end_time = _time
		pedestrian_arrived.emit(pedestrian)


## Moves a pedestrian `distance` along its route, stopping at the curb of a crosswalk it
## may not cross yet. Returns true when it reached the end.
func _walk(pedestrian: Pedestrian, distance: float) -> bool:
	var points := pedestrian.points
	while distance > 0.0:
		if pedestrian.leg >= points.size() - 1:
			return true
		var crossing := pedestrian.crossings[pedestrian.leg]
		if crossing != Crosswalks.NONE and pedestrian.on_crosswalk == Crosswalks.NONE:
			if not _may_cross(crossing):
				if pedestrian.waiting_at == Crosswalks.NONE:
					pedestrian.waiting_at = crossing
					crosswalks.add_waiting(crossing)
				break
			if pedestrian.waiting_at != Crosswalks.NONE:
				crosswalks.remove_waiting(pedestrian.waiting_at)
				pedestrian.waiting_at = Crosswalks.NONE
			pedestrian.on_crosswalk = crossing
			crosswalks.add_walker(crossing)
		var a := points[pedestrian.leg]
		var b := points[pedestrian.leg + 1]
		var remaining := a.distance_to(b) - pedestrian.leg_progress
		if distance < remaining:
			pedestrian.leg_progress += distance
			distance = 0.0
		else:
			distance -= remaining
			if pedestrian.on_crosswalk != Crosswalks.NONE:
				crosswalks.remove_walker(pedestrian.on_crosswalk)
				pedestrian.on_crosswalk = Crosswalks.NONE
			pedestrian.leg += 1
			pedestrian.leg_progress = 0.0
	if pedestrian.leg >= points.size() - 1:
		pedestrian.position = points[-1]
		return true
	var from := points[pedestrian.leg]
	pedestrian.position = from.move_toward(points[pedestrian.leg + 1], pedestrian.leg_progress)
	return false


## May a pedestrian at the curb step onto `crossing` now?
func _may_cross(crossing: Vector2i) -> bool:
	var node: RoadNode = _roads.nodes.get(crossing.x)
	if node == null:
		return true
	if node.has_lights() and not _signals.is_walk_green(node, crossing.y):
		return false
	return not _vehicles.is_crosswalk_busy(crossing)


func _stop_crossing(pedestrian: Pedestrian) -> void:
	if pedestrian.on_crosswalk != Crosswalks.NONE:
		crosswalks.remove_walker(pedestrian.on_crosswalk)
		pedestrian.on_crosswalk = Crosswalks.NONE
	if pedestrian.waiting_at != Crosswalks.NONE:
		crosswalks.remove_waiting(pedestrian.waiting_at)
		pedestrian.waiting_at = Crosswalks.NONE
