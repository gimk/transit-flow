class_name StressTraffic
extends RefCounted
## Debug load generator: keeps about `target_count` vehicles on random trips between
## random positions on road lanes. Used by the F6 debug key and the traffic benchmark.

## Owner id for stress vehicles (citizens use ids >= 1).
const OWNER_ID := -2
## At most this many new vehicles per tick, so traffic ramps up instead of popping in.
const MAX_SPAWNS_PER_TICK := 20
## Random trips are re-drawn this many times per spawn attempt before giving up.
const ATTEMPTS_PER_SPAWN := 3

var target_count := 0
var count := 0

var _roads: RoadNetwork
var _router: Router
var _vehicles: VehicleSystem
var _rng := RandomNumberGenerator.new()
var _road_lanes: Array[Lane] = []
var _road_lanes_dirty := true


func _init(roads: RoadNetwork, router: Router, vehicles: VehicleSystem, seed := 0) -> void:
	_roads = roads
	_router = router
	_vehicles = vehicles
	_rng.seed = seed
	_roads.changed.connect(_on_roads_changed)
	_vehicles.vehicle_arrived.connect(_on_vehicle_gone)
	_vehicles.vehicle_failed.connect(_on_vehicle_gone)


func step() -> void:
	if count >= target_count:
		return
	if _road_lanes_dirty:
		_road_lanes.clear()
		for lane: Lane in _roads.lanes.values():
			if not lane.is_connector():
				_road_lanes.append(lane)
		_road_lanes_dirty = false
	if _road_lanes.size() < 2:
		return
	for i in mini(target_count - count, MAX_SPAWNS_PER_TICK):
		for attempt in ATTEMPTS_PER_SPAWN:
			if _try_spawn():
				count += 1
				break


func _try_spawn() -> bool:
	var from: Lane = _road_lanes[_rng.randi() % _road_lanes.size()]
	var to: Lane = _road_lanes[_rng.randi() % _road_lanes.size()]
	var route := _router.find_route(from.id, _rng.randf() * from.length, to.id, _rng.randf() * to.length)
	return route != null and _vehicles.spawn(route, -1, OWNER_ID) != null


func _on_vehicle_gone(vehicle: Vehicle) -> void:
	if vehicle.owner_id == OWNER_ID:
		count -= 1


func _on_roads_changed(_node_ids: Array, _segment_ids: Array) -> void:
	_road_lanes_dirty = true
