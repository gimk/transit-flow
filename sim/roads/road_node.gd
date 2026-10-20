class_name RoadNode
extends RefCounted
## A point where road segments meet: a dead end, a corner or (from Phase 2) an intersection.

var id: int
var position: Vector2
## -1 tunnel/subway, 0 ground, 1+ bridges. Roads only connect on the same layer.
var elevation_layer: int
var segment_ids: Array[int] = []


func _init(p_id: int, p_position: Vector2, p_elevation_layer: int) -> void:
	id = p_id
	position = p_position
	elevation_layer = p_elevation_layer


func get_degree() -> int:
	return segment_ids.size()
