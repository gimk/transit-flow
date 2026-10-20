class_name Router
extends RefCounted
## Finds quickest lane-level routes between two positions on road lanes.
## The lane graph is rebuilt lazily on the first query after a network change, and lane
## paths are cached per (from lane, to lane): most trips share a few such pairs.

var graph := LaneGraph.new()

var _roads: RoadNetwork
var _graph_dirty := true
var _path_cache: Dictionary[Vector2i, PackedInt64Array] = {}


func _init(roads: RoadNetwork) -> void:
	_roads = roads
	_roads.changed.connect(_on_roads_changed)


## Quickest route from `from_distance` along lane `from_lane_id` to `to_distance` along
## lane `to_lane_id`, or null if the destination can't be reached.
func find_route(from_lane_id: int, from_distance: float, to_lane_id: int, to_distance: float) -> Route:
	if not (_roads.lanes.has(from_lane_id) and _roads.lanes.has(to_lane_id)):
		return null
	_ensure_graph()
	var path: PackedInt64Array
	if from_lane_id == to_lane_id and to_distance >= from_distance:
		path = PackedInt64Array([from_lane_id])
	else:
		path = _get_lane_path(from_lane_id, to_lane_id)
	if path.is_empty():
		return null
	return Route.new(path, from_distance, to_distance, _travel_time(path, from_distance, to_distance))


## Quickest route from any of `starts` to any of `ends` (each a { "lane_id", "distance" }
## position, e.g. both lanes in front of a building), or null if none connects.
func find_best_route(starts: Array[Dictionary], ends: Array[Dictionary]) -> Route:
	var best: Route = null
	for start in starts:
		for end in ends:
			var route := find_route(start.lane_id, start.distance, end.lane_id, end.distance)
			if route != null and (best == null or route.travel_time < best.travel_time):
				best = route
	return best


## Drops cached paths, e.g. after congestion costs change.
func invalidate_cache() -> void:
	_path_cache.clear()


func _on_roads_changed(_node_ids: Array, _segment_ids: Array) -> void:
	_graph_dirty = true
	_path_cache.clear()


func _ensure_graph() -> void:
	if _graph_dirty:
		graph.rebuild(_roads.lanes)
		_graph_dirty = false


func _get_lane_path(from_lane_id: int, to_lane_id: int) -> PackedInt64Array:
	var key := Vector2i(from_lane_id, to_lane_id)
	if _path_cache.has(key):
		return _path_cache[key]
	var path: PackedInt64Array
	if from_lane_id == to_lane_id:
		# The destination is behind us on the same lane: leave it and loop back around.
		var best_cost := INF
		for next_id in _roads.lanes[from_lane_id].outgoing_lane_ids:
			var loop := graph.get_id_path(next_id, to_lane_id)
			if loop.is_empty():
				continue
			var cost := _path_cost(loop)
			if cost < best_cost:
				best_cost = cost
				path = PackedInt64Array([from_lane_id]) + loop
	else:
		path = graph.get_id_path(from_lane_id, to_lane_id)
	_path_cache[key] = path
	return path


func _path_cost(path: PackedInt64Array) -> float:
	var cost := 0.0
	for lane_id in path:
		cost += graph.get_lane_cost(lane_id)
	return cost


## Whole-lane costs, minus the part of the first lane before the start
## and the part of the last lane after the destination.
func _travel_time(path: PackedInt64Array, from_distance: float, to_distance: float) -> float:
	var first := _roads.lanes[path[0]]
	var last := _roads.lanes[path[-1]]
	var time := _path_cost(path)
	time -= graph.get_lane_cost(first.id) * clampf(from_distance / first.length, 0.0, 1.0)
	time -= graph.get_lane_cost(last.id) * (1.0 - clampf(to_distance / last.length, 0.0, 1.0))
	return maxf(time, 0.0)
