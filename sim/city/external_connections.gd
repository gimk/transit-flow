class_name ExternalConnections
extends RefCounted
## Fixed points on the map edge where the city meets the outside world (highways for
## now). Trucks bring raw materials and imports in, and take exports out, through them.
## A connection works while a road ends exactly on its point.

signal changed

var points: Array[Vector2] = []

var _roads: RoadNetwork


func _init(roads: RoadNetwork) -> void:
	_roads = roads


func add(point: Vector2) -> void:
	if not points.has(point):
		points.append(point)
		changed.emit()


## Points that currently have a road ending on them.
func get_active_points() -> Array[Vector2]:
	var active: Array[Vector2] = []
	for point in points:
		if _roads.find_node_at(point) != -1:
			active.append(point)
	return active


## Where vehicles arriving from outside appear: just inside the lane leaving the point.
## { "lane_id", "distance" }, or {} if the connection has no road.
func get_inbound(point: Vector2) -> Dictionary:
	var lane := _edge_lane(point, true)
	if lane == null:
		return {}
	return { "lane_id": lane.id, "distance": minf(Vehicle.MAX_LENGTH, lane.length * 0.5) }


## Where vehicles leaving the city disappear: the end of the lane arriving at the point.
func get_outbound(point: Vector2) -> Dictionary:
	var lane := _edge_lane(point, false)
	if lane == null:
		return {}
	return { "lane_id": lane.id, "distance": maxf(lane.length - 1.0, 0.0) }


func to_dict() -> Dictionary:
	var list: Array = []
	for point in points:
		list.append([point.x, point.y])
	return { "points": list }


func restore(data: Dictionary) -> void:
	points.clear()
	for entry: Array in data.get("points", []):
		points.append(Vector2(entry[0], entry[1]))
	changed.emit()


func _edge_lane(point: Vector2, leaving: bool) -> Lane:
	var node_id := _roads.find_node_at(point)
	if node_id == -1:
		return null
	for segment_id in _roads.nodes[node_id].segment_ids:
		for lane_id in _roads.segments[segment_id].lane_ids:
			var lane := _roads.lanes[lane_id]
			if (lane.from_node_id if leaving else lane.to_node_id) == node_id:
				return lane
	return null
