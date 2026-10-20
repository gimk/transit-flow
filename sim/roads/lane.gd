class_name Lane
extends RefCounted
## One directed path a vehicle can follow, from `from_node_id` to `to_node_id`.
## Road lanes and the connector lanes that turn through a junction are both Lanes,
## so vehicles only ever need to know how to follow a Curve2D.

enum Turn { NONE, STRAIGHT, RIGHT, LEFT, U_TURN }

var id: int
## Owning segment, or -1 for a connector inside a junction.
var segment_id: int
## For connectors both are the junction node.
var from_node_id: int
var to_node_id: int
## 0 = the lane next to the road's center line, increasing toward the curb.
var index_from_center: int
var curve: Curve2D
var length: float
var speed_limit: float
## The movement a connector performs; NONE for road lanes.
var turn := Turn.NONE
## Lanes a vehicle may continue onto from this lane's end.
var outgoing_lane_ids: Array[int] = []


func _init(p_id: int, p_segment_id: int, p_from_node_id: int, p_to_node_id: int,
		p_index_from_center: int, p_curve: Curve2D, p_speed_limit: float) -> void:
	id = p_id
	segment_id = p_segment_id
	from_node_id = p_from_node_id
	to_node_id = p_to_node_id
	index_from_center = p_index_from_center
	speed_limit = p_speed_limit
	set_curve(p_curve)


func set_curve(p_curve: Curve2D) -> void:
	curve = p_curve
	length = p_curve.get_baked_length()


func is_connector() -> bool:
	return segment_id == -1


## Free-flow time to drive the whole lane, in sim seconds.
func get_travel_time() -> float:
	return length / speed_limit
