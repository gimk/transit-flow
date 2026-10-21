class_name Router
extends RefCounted
## Finds quickest lane-level routes between two positions on road lanes.
## The lane graph is rebuilt lazily on the first query after a network change, and lane
## paths are cached per (from lane, to lane): most trips share a few such pairs.
## Buses route on a graph of their own once there are bus lanes (`bus` arguments): cars
## may not drive along them (see LaneGraph).

## The graph for everyone but buses (its congestion costs are shared with the bus graph).
var graph := LaneGraph.new()

var _roads: RoadNetwork
var _graph_dirty := true
## The graph for buses, used when there are bus lanes (otherwise `graph` serves).
var _bus_graph := LaneGraph.new()
var _has_bus_lanes := false
## Vector4i(from lane, to lane, loop back, for buses) -> lane path.
var _path_cache: Dictionary[Vector4i, PackedInt64Array] = {}


func _init(roads: RoadNetwork) -> void:
	_roads = roads
	_roads.changed.connect(_on_roads_changed)


## Quickest route from `from_distance` along lane `from_lane_id` to `to_distance` along
## lane `to_lane_id`, or null if the destination can't be reached. A route may change
## lanes along a road (consecutive lanes beside each other, see Lane.is_beside). `bus`:
## for a bus, which may use bus lanes.
func find_route(from_lane_id: int, from_distance: float, to_lane_id: int, to_distance: float, bus := false) -> Route:
	if not (_roads.lanes.has(from_lane_id) and _roads.lanes.has(to_lane_id)):
		return null
	_ensure_graph()
	var path: PackedInt64Array
	var from_lane := _roads.lanes[from_lane_id]
	var to_lane := _roads.lanes[to_lane_id]
	var same_road := from_lane_id == to_lane_id or from_lane.is_beside(to_lane)
	if from_lane_id == to_lane_id and to_distance >= from_distance:
		path = PackedInt64Array([from_lane_id])
	else:
		# Behind us on the same road (this lane or one beside it): go round the block.
		path = _get_lane_path(from_lane_id, to_lane_id, same_road and to_distance < from_distance, bus)
	if path.is_empty():
		return null
	return Route.new(path, from_distance, to_distance, _travel_time(path, from_distance, to_distance, _graph_for(bus)))


## Quickest route from any of `starts` to any of `ends` (each a { "lane_id", "distance" }
## position, e.g. both lanes in front of a building; an optional "penalty" adds expected
## delay, like turning across the road), or null if none connects.
func find_best_route(starts: Array[Dictionary], ends: Array[Dictionary], bus := false) -> Route:
	var best: Route = null
	var best_cost := INF
	for start in starts:
		for end in ends:
			var route := find_route(start.lane_id, start.distance, end.lane_id, end.distance, bus)
			if route == null:
				continue
			var cost: float = route.travel_time + start.get("penalty", 0.0) + end.get("penalty", 0.0)
			if cost < best_cost:
				best = route
				best_cost = cost
	return best


## Quickest route to any of `ends` that leaves `from_lane_id` through its end, i.e. without
## changing lanes first (for a driver who missed the lane they needed). The route starts
## on the lane after it; null if there's none.
func find_route_onward(from_lane_id: int, ends: Array[Dictionary], bus := false) -> Route:
	var lane: Lane = _roads.lanes.get(from_lane_id)
	if lane == null:
		return null
	var starts: Array[Dictionary] = []
	for next_id in lane.outgoing_lane_ids:
		starts.append({ "lane_id": next_id, "distance": 0.0 })
	return find_best_route(starts, ends, bus)


## Travel time of a route on empty roads (Route.get_free_flow_time).
func get_free_flow_time(route: Route) -> float:
	return route.get_free_flow_time(_roads)


## Drops cached paths, e.g. after congestion costs change.
func invalidate_cache() -> void:
	_path_cache.clear()


func _on_roads_changed(_node_ids: Array, _segment_ids: Array) -> void:
	_graph_dirty = true
	_path_cache.clear()


func _ensure_graph() -> void:
	if _graph_dirty:
		_has_bus_lanes = _roads.lanes.values().any(func(lane: Lane) -> bool: return lane.bus_only)
		graph.rebuild(_roads.lanes, not _has_bus_lanes)
		if _has_bus_lanes:
			_bus_graph.rebuild(_roads.lanes, true)
		_graph_dirty = false


func _graph_for(bus: bool) -> LaneGraph:
	if not (bus and _has_bus_lanes):
		return graph
	# Same congestion as everyone (the monitor may have replaced the dictionary).
	_bus_graph.congestion = graph.congestion
	return _bus_graph


func _get_lane_path(from_lane_id: int, to_lane_id: int, loop_back: bool, bus: bool) -> PackedInt64Array:
	var for_buses := bus and _has_bus_lanes
	var key := Vector4i(from_lane_id, to_lane_id, int(loop_back), int(for_buses))
	if _path_cache.has(key):
		return _path_cache[key]
	var lane_graph := _graph_for(bus)
	var path: PackedInt64Array
	if loop_back:
		# The destination is behind us: leave the road and loop back around.
		var best_cost := INF
		for next_id in _roads.lanes[from_lane_id].outgoing_lane_ids:
			if not lane_graph.are_points_connected(from_lane_id, next_id, false):
				continue  # Not this way (a car on a bus lane).
			var loop := lane_graph.get_id_path(next_id, to_lane_id)
			if loop.is_empty():
				continue
			var cost := _path_cost(loop, lane_graph)
			if cost < best_cost:
				best_cost = cost
				path = PackedInt64Array([from_lane_id]) + loop
	else:
		path = lane_graph.get_id_path(from_lane_id, to_lane_id)
	_path_cache[key] = path
	return path


static func _path_cost(path: PackedInt64Array, lane_graph: LaneGraph) -> float:
	var cost := lane_graph.get_lane_cost(path[0])
	for i in range(1, path.size()):
		cost += lane_graph.get_step_cost(path[i - 1], path[i])
	return cost


## Per run of lanes driven side by side (Route.get_runs): the part of its last lane
## actually driven (from the start on the first run, to the destination on the last),
## plus a penalty per lane change.
func _travel_time(path: PackedInt64Array, from_distance: float, to_distance: float, lane_graph: LaneGraph) -> float:
	var time := 0.0
	var runs := Route.get_runs(path, _roads)
	for r in runs.size():
		var lane := _roads.lanes[path[runs[r].y]]
		var from_fraction := clampf(from_distance / lane.length, 0.0, 1.0) if r == 0 else 0.0
		var to_fraction := clampf(to_distance / lane.length, 0.0, 1.0) if r == runs.size() - 1 else 1.0
		time += lane_graph.get_lane_cost(lane.id) * maxf(to_fraction - from_fraction, 0.0)
		time += LaneGraph.LANE_CHANGE_COST * (runs[r].y - runs[r].x)
	return time
