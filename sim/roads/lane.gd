class_name Lane
extends RefCounted
## One directed path a vehicle can follow, from `from_node_id` to `to_node_id`.
## Road lanes and (from Phase 1b) turns inside intersections are both Lanes,
## so vehicles only ever need to know how to follow a Curve2D.

var id: int
var segment_id: int
var from_node_id: int
var to_node_id: int
## 0 = the lane next to the road's center line, increasing toward the curb.
var index_from_center: int
var curve: Curve2D
var length: float
var speed_limit: float


func _init(p_id: int, p_segment_id: int, p_from_node_id: int, p_to_node_id: int,
		p_index_from_center: int, p_curve: Curve2D, p_speed_limit: float) -> void:
	id = p_id
	segment_id = p_segment_id
	from_node_id = p_from_node_id
	to_node_id = p_to_node_id
	index_from_center = p_index_from_center
	curve = p_curve
	length = p_curve.get_baked_length()
	speed_limit = p_speed_limit
