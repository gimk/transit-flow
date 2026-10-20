class_name RoadNetwork
extends RefCounted
## Authoritative road graph: nodes, segments, and the lanes derived from them.
## Every edit goes through a command method (add_road, remove_segment …), and every
## command emits `changed` with the node and segment ids it touched, so views and
## (later) routing can update incrementally.
##
## Phase 1a rule: roads may only meet at their endpoints. Crossings and T-junctions
## onto the middle of a road are rejected until Phase 2a splits segments.

signal changed(node_ids: Array, segment_ids: Array)

## Positions closer than this are treated as the same point.
const EPSILON := 0.5

var nodes: Dictionary[int, RoadNode] = {}
var segments: Dictionary[int, RoadSegment] = {}
var lanes: Dictionary[int, Lane] = {}

var _next_node_id := 1
var _next_segment_id := 1
var _next_lane_id := 1


# --- Commands ---------------------------------------------------------------

## Adds a straight road. Returns the new segment id, or -1 if the road is invalid.
func add_road(from: Vector2, to: Vector2, road_type: RoadType, elevation_layer := 0) -> int:
	return add_road_with_centerline(straight_curve(from, to), road_type, elevation_layer)


func add_road_with_centerline(centerline: Curve2D, road_type: RoadType, elevation_layer := 0) -> int:
	if validate_centerline(centerline, elevation_layer) != "":
		return -1
	var last := centerline.point_count - 1
	var start_id := _get_or_create_node(centerline.get_point_position(0), elevation_layer)
	var end_id := _get_or_create_node(centerline.get_point_position(last), elevation_layer)
	# Make the curve end exactly on the (possibly pre-existing) nodes.
	centerline.set_point_position(0, nodes[start_id].position)
	centerline.set_point_position(last, nodes[end_id].position)

	var segment := _insert_segment(_next_segment_id, start_id, end_id, road_type, elevation_layer, centerline)
	_next_segment_id += 1
	changed.emit([start_id, end_id], [segment.id])
	return segment.id


## Removes a segment and its lanes. Nodes left without any segment are removed too.
func remove_segment(segment_id: int) -> bool:
	var segment: RoadSegment = segments.get(segment_id)
	if segment == null:
		return false
	for lane_id in segment.lane_ids:
		lanes.erase(lane_id)
	segments.erase(segment_id)
	var touched_nodes: Array[int] = [segment.start_node_id, segment.end_node_id]
	for node_id in touched_nodes:
		var node := nodes[node_id]
		node.segment_ids.erase(segment_id)
		if node.segment_ids.is_empty():
			nodes.erase(node_id)
	changed.emit(touched_nodes, [segment_id])
	return true


# --- Queries ----------------------------------------------------------------

## Returns "" if the road can be built, otherwise a player-facing reason.
func validate_road(from: Vector2, to: Vector2, elevation_layer := 0) -> String:
	return validate_centerline(straight_curve(from, to), elevation_layer)


func validate_centerline(centerline: Curve2D, elevation_layer := 0) -> String:
	var points := LaneBuilder.get_polyline(centerline)
	if points.size() < 2 or points[0].distance_to(points[-1]) <= EPSILON:
		return "Road is too short"
	var start := points[0]
	var end := points[-1]

	var start_node := find_node_at(start, elevation_layer)
	var end_node := find_node_at(end, elevation_layer)
	if start_node != -1 and end_node != -1 and get_segment_between(start_node, end_node) != -1:
		return "Road already exists"

	for node: RoadNode in nodes.values():
		if node.elevation_layer != elevation_layer:
			continue
		if node.position.distance_to(start) <= EPSILON or node.position.distance_to(end) <= EPSILON:
			continue
		if LaneBuilder.distance_to_polyline(node.position, points) <= EPSILON:
			return "Road can't pass through a junction yet"

	for segment: RoadSegment in segments.values():
		if segment.elevation_layer != elevation_layer:
			continue
		var other := LaneBuilder.get_polyline(segment.centerline)
		if _is_on_interior(start, other) or _is_on_interior(end, other):
			return "Roads can only connect at their ends for now"
		if _polylines_cross(points, other):
			return "Roads can't cross yet"
	return ""


## Id of the node at `position` on the given layer, or -1.
func find_node_at(position: Vector2, elevation_layer := 0) -> int:
	for node: RoadNode in nodes.values():
		if node.elevation_layer == elevation_layer and node.position.distance_to(position) <= EPSILON:
			return node.id
	return -1


## Id of the segment whose centerline is closest to `position` within `max_distance`, or -1.
func find_segment_at(position: Vector2, max_distance: float, elevation_layer := 0) -> int:
	var best_id := -1
	var best_distance := max_distance
	for segment: RoadSegment in segments.values():
		if segment.elevation_layer != elevation_layer:
			continue
		var distance := LaneBuilder.distance_to_polyline(position, LaneBuilder.get_polyline(segment.centerline))
		if distance <= best_distance:
			best_id = segment.id
			best_distance = distance
	return best_id


func get_segment_between(node_a: int, node_b: int) -> int:
	if not nodes.has(node_a):
		return -1
	for segment_id in nodes[node_a].segment_ids:
		if segments[segment_id].get_other_node_id(node_a) == node_b:
			return segment_id
	return -1


static func straight_curve(from: Vector2, to: Vector2) -> Curve2D:
	var curve := Curve2D.new()
	curve.add_point(from)
	curve.add_point(to)
	return curve


# --- Serialization ----------------------------------------------------------
# Lanes are derived data: they are rebuilt on load, never saved.

func to_dict() -> Dictionary:
	var node_list: Array = []
	for node: RoadNode in nodes.values():
		node_list.append({
			"id": node.id,
			"position": [node.position.x, node.position.y],
			"layer": node.elevation_layer,
		})
	var segment_list: Array = []
	for segment: RoadSegment in segments.values():
		var points: Array = []
		for i in segment.centerline.point_count:
			var p := segment.centerline.get_point_position(i)
			var p_in := segment.centerline.get_point_in(i)
			var p_out := segment.centerline.get_point_out(i)
			points.append([p.x, p.y, p_in.x, p_in.y, p_out.x, p_out.y])
		segment_list.append({
			"id": segment.id,
			"start": segment.start_node_id,
			"end": segment.end_node_id,
			"type": String(segment.road_type.id),
			"layer": segment.elevation_layer,
			"centerline": points,
		})
	return {
		"nodes": node_list,
		"segments": segment_list,
		"next_node_id": _next_node_id,
		"next_segment_id": _next_segment_id,
	}


static func from_dict(data: Dictionary) -> RoadNetwork:
	var network := RoadNetwork.new()
	for entry: Dictionary in data.get("nodes", []):
		var node := RoadNode.new(int(entry.id), Vector2(entry.position[0], entry.position[1]), int(entry.layer))
		network.nodes[node.id] = node
	for entry: Dictionary in data.get("segments", []):
		var road_type := RoadType.load_by_id(StringName(entry.type))
		if road_type == null:
			push_warning("Skipping segment %d: unknown road type '%s'" % [entry.id, entry.type])
			continue
		var centerline := Curve2D.new()
		for p: Array in entry.centerline:
			centerline.add_point(Vector2(p[0], p[1]), Vector2(p[2], p[3]), Vector2(p[4], p[5]))
		network._insert_segment(int(entry.id), int(entry.start), int(entry.end), road_type, int(entry.layer), centerline)
	network._next_node_id = int(data.get("next_node_id", 1))
	network._next_segment_id = int(data.get("next_segment_id", 1))
	return network


# --- Internals --------------------------------------------------------------

func _get_or_create_node(position: Vector2, elevation_layer: int) -> int:
	var existing := find_node_at(position, elevation_layer)
	if existing != -1:
		return existing
	var node := RoadNode.new(_next_node_id, position, elevation_layer)
	_next_node_id += 1
	nodes[node.id] = node
	return node.id


func _insert_segment(id: int, start_id: int, end_id: int, road_type: RoadType,
		elevation_layer: int, centerline: Curve2D) -> RoadSegment:
	var segment := RoadSegment.new(id, start_id, end_id, road_type, elevation_layer, centerline)
	segments[id] = segment
	nodes[start_id].segment_ids.append(id)
	nodes[end_id].segment_ids.append(id)
	_build_lanes(segment)
	return segment


func _build_lanes(segment: RoadSegment) -> void:
	for lane_id in segment.lane_ids:
		lanes.erase(lane_id)
	segment.lane_ids.clear()
	for spec in LaneBuilder.build_lane_specs(segment.centerline, segment.road_type):
		var forward: bool = spec.forward
		var lane := Lane.new(
			_next_lane_id,
			segment.id,
			segment.start_node_id if forward else segment.end_node_id,
			segment.end_node_id if forward else segment.start_node_id,
			spec.index,
			spec.curve,
			segment.road_type.speed_limit)
		_next_lane_id += 1
		lanes[lane.id] = lane
		segment.lane_ids.append(lane.id)


## True if `point` lies on the polyline but not at either of its ends.
func _is_on_interior(point: Vector2, polyline: PackedVector2Array) -> bool:
	if point.distance_to(polyline[0]) <= EPSILON or point.distance_to(polyline[-1]) <= EPSILON:
		return false
	return LaneBuilder.distance_to_polyline(point, polyline) <= EPSILON


## True if the polylines touch anywhere other than at an end they share.
## Collinear overlaps are caught earlier by the node/endpoint checks.
func _polylines_cross(a: PackedVector2Array, b: PackedVector2Array) -> bool:
	for i in a.size() - 1:
		for j in b.size() - 1:
			var hit: Variant = Geometry2D.segment_intersects_segment(a[i], a[i + 1], b[j], b[j + 1])
			if hit == null:
				continue
			var at_end_of_a: bool = hit.distance_to(a[0]) <= EPSILON or hit.distance_to(a[-1]) <= EPSILON
			var at_end_of_b: bool = hit.distance_to(b[0]) <= EPSILON or hit.distance_to(b[-1]) <= EPSILON
			if not (at_end_of_a and at_end_of_b):
				return true
	return false
