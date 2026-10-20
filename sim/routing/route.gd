class_name Route
extends RefCounted
## A path through the network: drive `lane_ids` in order, entering the first lane at
## `start_distance` along it and stopping on the last lane at `end_distance`.
## Consecutive lanes are either joined end to start (on through a junction) or beside
## each other on the same road (a lane change, see Lane.is_beside): a run of lanes side
## by side is driven once, moving across on the way.

var lane_ids: PackedInt64Array
var start_distance: float
var end_distance: float
## Free-flow (plus current congestion) travel time estimate, in sim seconds.
var travel_time: float


func _init(p_lane_ids: PackedInt64Array, p_start_distance: float, p_end_distance: float,
		p_travel_time: float) -> void:
	lane_ids = p_lane_ids
	start_distance = p_start_distance
	end_distance = p_end_distance
	travel_time = p_travel_time


## Groups a path into runs of lanes driven side by side: Vector2i(first index, last
## index) each. A path without lane changes has one run per lane.
static func get_runs(path: PackedInt64Array, roads: RoadNetwork) -> Array[Vector2i]:
	var runs: Array[Vector2i] = []
	var start := 0
	for i in path.size():
		var last := i == path.size() - 1
		if last or not roads.lanes[path[i]].is_beside(roads.lanes[path[i + 1]]):
			runs.append(Vector2i(start, i))
			start = i + 1
	return runs


## Travel time with no traffic at all (speed limits only), in sim seconds: the yardstick
## for patience and the Flow score.
func get_free_flow_time(roads: RoadNetwork) -> float:
	var time := 0.0
	var runs := get_runs(lane_ids, roads)
	for r in runs.size():
		var lane := roads.lanes[lane_ids[runs[r].y]]
		var from := start_distance if r == 0 else 0.0
		var to := end_distance if r == runs.size() - 1 else lane.length
		time += maxf(to - from, 0.0) / lane.speed_limit
	return time


## The driven path as one polyline (for debug drawing and tests); a lane change is drawn
## as a step across at the start of the run.
func get_polyline(roads: RoadNetwork) -> PackedVector2Array:
	var points := PackedVector2Array()
	var runs := get_runs(lane_ids, roads)
	for r in runs.size():
		var lane := roads.lanes[lane_ids[runs[r].y]]
		var lane_points := LaneBuilder.get_polyline(lane.curve)
		var from := start_distance if r == 0 else 0.0
		var to := end_distance if r == runs.size() - 1 else INF
		points.append_array(LaneBuilder.sub_polyline(lane_points, from, to))
	return points
