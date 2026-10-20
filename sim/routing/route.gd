class_name Route
extends RefCounted
## A path through the network: drive `lane_ids` in order, entering the first lane at
## `start_distance` along it and stopping on the last lane at `end_distance`.

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


## Travel time with no traffic at all (speed limits only), in sim seconds: the yardstick
## for patience and the Flow score.
func get_free_flow_time(roads: RoadNetwork) -> float:
	var time := 0.0
	var last := lane_ids.size() - 1
	for i in lane_ids.size():
		var lane := roads.lanes[lane_ids[i]]
		var from := start_distance if i == 0 else 0.0
		var to := end_distance if i == last else lane.length
		time += maxf(to - from, 0.0) / lane.speed_limit
	return time


## The driven path as one polyline (for debug drawing and tests).
func get_polyline(roads: RoadNetwork) -> PackedVector2Array:
	var points := PackedVector2Array()
	var last := lane_ids.size() - 1
	for i in lane_ids.size():
		var lane := roads.lanes[lane_ids[i]]
		var lane_points := LaneBuilder.get_polyline(lane.curve)
		var from := start_distance if i == 0 else 0.0
		var to := end_distance if i == last else INF
		points.append_array(LaneBuilder.sub_polyline(lane_points, from, to))
	return points
