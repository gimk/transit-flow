class_name RoadSegment
extends RefCounted
## A stretch of road between two nodes. Its shape is a centerline Curve2D:
## a straight grid road is just a 2-point curve, so curved roads need no new data.

var id: int
var start_node_id: int
var end_node_id: int
var road_type: RoadType
var elevation_layer: int
var centerline: Curve2D
## Lanes derived from the centerline; rebuilt whenever the segment changes.
var lane_ids: Array[int] = []


func _init(p_id: int, p_start_node_id: int, p_end_node_id: int, p_road_type: RoadType,
		p_elevation_layer: int, p_centerline: Curve2D) -> void:
	id = p_id
	start_node_id = p_start_node_id
	end_node_id = p_end_node_id
	road_type = p_road_type
	elevation_layer = p_elevation_layer
	centerline = p_centerline


func get_other_node_id(node_id: int) -> int:
	return end_node_id if node_id == start_node_id else start_node_id


func get_length() -> float:
	return centerline.get_baked_length()
