class_name CongestionMonitor
extends RefCounted
## Every UPDATE_INTERVAL, measures how much slower than its speed limit traffic is moving
## on each road lane, feeds that into routing costs (LaneGraph.congestion), and lets
## vehicles held up in slow traffic look for a better route. New trips pick up the new
## costs automatically, so a jam pushes traffic onto alternatives.

const UPDATE_INTERVAL := 5.0  ## Sim seconds.
## Weight of the newest measurement against the previous value (smooths flickering).
const SMOOTHING := 0.5
## A lane's travel time may be at most this many times its free-flow time.
const MAX_PENALTY := 10.0
## Values below this are dropped (the lane counts as free-flowing).
const MIN_PENALTY := 0.05
## Average speeds are floored at this, so a fully stopped queue maps to MAX_PENALTY-ish.
const MIN_MEASURED_SPEED := 5.0
const REROUTES_PER_UPDATE := 50

## Lane id -> extra travel time as a fraction of free-flow time (0 = free flow).
## Shared with the router's LaneGraph.
var congestion: Dictionary[int, float] = {}

var _roads: RoadNetwork
var _router: Router
var _vehicles: VehicleSystem
var _next_update := UPDATE_INTERVAL


func _init(roads: RoadNetwork, router: Router, vehicles: VehicleSystem) -> void:
	_roads = roads
	_router = router
	_vehicles = vehicles


func step(time: float) -> void:
	if time < _next_update:
		return
	_next_update = time + UPDATE_INTERVAL
	update()


## Measures now (normally called by step on its interval; public for tests and tools).
func update() -> void:
	var measured: Dictionary[int, float] = {}
	for lane: Lane in _vehicles.get_occupied_lanes():
		if lane.is_connector():
			continue
		var total_speed := 0.0
		for vehicle: Vehicle in lane.queue:
			total_speed += vehicle.speed
		var average := maxf(total_speed / lane.queue.size(), MIN_MEASURED_SPEED)
		measured[lane.id] = clampf(lane.speed_limit / average - 1.0, 0.0, MAX_PENALTY)

	var updated: Dictionary[int, float] = {}
	for lane_id in measured:
		updated[lane_id] = lerpf(congestion.get(lane_id, 0.0), measured[lane_id], SMOOTHING)
	for lane_id in congestion:
		if not measured.has(lane_id) and _roads.lanes.has(lane_id):
			updated[lane_id] = congestion[lane_id] * (1.0 - SMOOTHING)
	for lane_id in updated.keys():
		if updated[lane_id] < MIN_PENALTY:
			updated.erase(lane_id)

	congestion = updated
	_router.graph.congestion = congestion
	_router.invalidate_cache()
	_vehicles.reroute_delayed(REROUTES_PER_UPDATE)
