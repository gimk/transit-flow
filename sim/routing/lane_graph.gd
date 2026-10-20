class_name LaneGraph
extends AStar2D
## A* graph with one point per lane (road lanes and connectors alike), placed at the
## lane's end, and one directed connection per allowed lane -> lane transition: on
## into the lanes a lane leads to, or sideways into the lane next door (a lane change).
## The cost of a step is the travel time of the lane being entered (a fixed penalty for
## a lane change), so A* minimises time rather than distance.

## Extra cost per lane as a fraction of its free-flow time (Phase 2c congestion
## feedback). Missing entries mean free flow.
var congestion: Dictionary[int, float] = {}

## Route cost of changing to the lane next door, in sim seconds: enough that drivers
## don't weave for nothing, small enough to pick the lane their turn needs.
const LANE_CHANGE_COST := 1.0

var _lanes: Dictionary[int, Lane] = {}
var _max_speed := 1.0


func rebuild(lanes: Dictionary[int, Lane]) -> void:
	clear()
	_lanes = lanes
	_max_speed = 1.0
	for lane: Lane in lanes.values():
		add_point(lane.id, lane.curve.get_point_position(lane.curve.point_count - 1))
		_max_speed = maxf(_max_speed, lane.speed_limit)
	# Road lanes side by side going the same way: a lane change, to the lane next door.
	var groups: Dictionary[Vector2i, Array] = {}
	for lane: Lane in lanes.values():
		if not lane.is_connector():
			var key := Vector2i(lane.segment_id, lane.from_node_id)
			if not groups.has(key):
				groups[key] = []
			groups[key].append(lane)
		for next_id in lane.outgoing_lane_ids:
			connect_points(lane.id, next_id, false)
	for group: Array in groups.values():
		for a: Lane in group:
			for b: Lane in group:
				if absi(a.index_from_center - b.index_from_center) == 1:
					connect_points(a.id, b.id, false)


## Expected time to drive the whole lane, congestion included.
func get_lane_cost(lane_id: int) -> float:
	return _lanes[lane_id].get_travel_time() * (1.0 + congestion.get(lane_id, 0.0))


## Cost of moving from one lane onto the next one of a path: a lane change costs a
## fixed penalty (the vehicle drives on alongside, not the whole new lane again);
## anything else costs the whole lane entered.
func get_step_cost(from_id: int, to_id: int) -> float:
	if _lanes[from_id].is_beside(_lanes[to_id]):
		return LANE_CHANGE_COST
	return get_lane_cost(to_id)


func _compute_cost(from_id: int, to_id: int) -> float:
	return get_step_cost(from_id, to_id)


## Straight-line distance at the fastest speed anywhere: never overestimates,
## so A* still finds the quickest route.
func _estimate_cost(from_id: int, end_id: int) -> float:
	return get_point_position(from_id).distance_to(get_point_position(end_id)) / _max_speed
