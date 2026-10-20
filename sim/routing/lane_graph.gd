class_name LaneGraph
extends AStar2D
## A* graph with one point per lane (road lanes and connectors alike), placed at the
## lane's end, and one directed connection per allowed lane -> lane transition.
## The cost of a step is the travel time of the lane being entered, so A* minimises
## time rather than distance.

## Extra cost per lane as a fraction of its free-flow time (Phase 2c congestion
## feedback). Missing entries mean free flow.
var congestion: Dictionary[int, float] = {}

var _lanes: Dictionary[int, Lane] = {}
var _max_speed := 1.0


func rebuild(lanes: Dictionary[int, Lane]) -> void:
	clear()
	_lanes = lanes
	_max_speed = 1.0
	for lane: Lane in lanes.values():
		add_point(lane.id, lane.curve.get_point_position(lane.curve.point_count - 1))
		_max_speed = maxf(_max_speed, lane.speed_limit)
	for lane: Lane in lanes.values():
		for next_id in lane.outgoing_lane_ids:
			connect_points(lane.id, next_id, false)


## Expected time to drive the whole lane, congestion included.
func get_lane_cost(lane_id: int) -> float:
	return _lanes[lane_id].get_travel_time() * (1.0 + congestion.get(lane_id, 0.0))


func _compute_cost(_from_id: int, to_id: int) -> float:
	return get_lane_cost(to_id)


## Straight-line distance at the fastest speed anywhere: never overestimates,
## so A* still finds the quickest route.
func _estimate_cost(from_id: int, end_id: int) -> float:
	return get_point_position(from_id).distance_to(get_point_position(end_id)) / _max_speed
