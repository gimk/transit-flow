class_name RoadNetwork
extends RefCounted
## Authoritative road graph: nodes, segments, and the lanes derived from them.
## Every edit goes through a command method (add_road, remove_segment …), and every
## command emits `changed` with the node and segment ids it touched, so views and
## routing can update incrementally.
##
## Lanes: each segment owns road lanes (stable ids for the segment's lifetime), trimmed
## back from junction nodes. Each node owns connector lanes linking its incoming lanes
## to its outgoing lanes (rebuilt, with new ids, whenever the junction changes).
## Dead ends get a U-turn connector so no lane is a trap.
##
## Building a road that crosses, touches or passes through existing roads on the same
## elevation layer splits everything at those points and creates junction nodes there.

signal changed(node_ids: Array, segment_ids: Array)

## Positions closer than this are treated as the same point.
const EPSILON := 0.5
## No segment may be shorter than this (it would leave no room between junctions).
const MIN_SEGMENT_LENGTH := 16.0
## A connector counts as going straight if it turns by less than this.
const STRAIGHT_MAX_ANGLE := PI / 6.0  # 30°
## Connector speed limit relative to the slower of the two lanes it joins.
const TURN_SPEED_FACTORS := {
	Lane.Turn.STRAIGHT: 1.0,
	Lane.Turn.RIGHT: 0.5,
	Lane.Turn.LEFT: 0.6,
	Lane.Turn.U_TURN: 0.3,
}

var nodes: Dictionary[int, RoadNode] = {}
var segments: Dictionary[int, RoadSegment] = {}
var lanes: Dictionary[int, Lane] = {}

var _next_node_id := 1
var _next_segment_id := 1
var _next_lane_id := 1


# --- Commands ---------------------------------------------------------------

## Adds a straight road, splitting it and any existing road it meets into separate
## segments joined at junctions. Returns the first new segment id, or -1 if invalid.
func add_road(from: Vector2, to: Vector2, road_type: RoadType, elevation_layer := 0) -> int:
	var ids := add_road_pieces(from, to, road_type, elevation_layer)
	return ids[0] if not ids.is_empty() else -1


## Like add_road, but returns every new segment id (in order from `from` to `to`).
func add_road_pieces(from: Vector2, to: Vector2, road_type: RoadType, elevation_layer := 0) -> Array[int]:
	var plan := plan_road(from, to, elevation_layer)
	var new_ids: Array[int] = []
	if plan.error != "":
		return new_ids
	var touched_segments := []
	var junction_ids := []
	for point: Vector2 in plan.points:
		var node_id := find_node_at(point, elevation_layer)
		if node_id == -1:
			var split_id := _find_segment_with_interior_point(point, elevation_layer)
			if split_id != -1:
				var old := segments[split_id]
				junction_ids.append_array([old.start_node_id, old.end_node_id])
				touched_segments.append(split_id)
				node_id = _split_segment(split_id, point)
				touched_segments.append_array(nodes[node_id].segment_ids)
			else:
				node_id = _create_node(point, elevation_layer)
		junction_ids.append(node_id)
		plan.node_ids.append(node_id)

	for i in plan.overlaps.size():
		if plan.overlaps[i]:
			continue
		var start_id: int = plan.node_ids[i]
		var end_id: int = plan.node_ids[i + 1]
		var centerline := straight_curve(nodes[start_id].position, nodes[end_id].position)
		var segment := _insert_segment(_next_segment_id, start_id, end_id, road_type, elevation_layer, centerline)
		_next_segment_id += 1
		new_ids.append(segment.id)

	for segment_id in _refresh_junctions(junction_ids):
		if not touched_segments.has(segment_id):
			touched_segments.append(segment_id)
	changed.emit(junction_ids, touched_segments)
	return new_ids


## Removes a segment and its lanes. Nodes left without any segment are removed too.
func remove_segment(segment_id: int) -> bool:
	var segment: RoadSegment = segments.get(segment_id)
	if segment == null:
		return false
	_detach_segment(segment)
	var touched_nodes := [segment.start_node_id, segment.end_node_id]
	for node_id: int in touched_nodes:
		var node := nodes[node_id]
		if node.segment_ids.is_empty():
			_erase_connectors(node)
			nodes.erase(node_id)
	var touched_segments := _refresh_junctions(touched_nodes)
	touched_segments.append(segment_id)
	changed.emit(touched_nodes, touched_segments)
	return true


# --- Queries ----------------------------------------------------------------

## Returns "" if the road can be built, otherwise a player-facing reason.
func validate_road(from: Vector2, to: Vector2, elevation_layer := 0) -> String:
	return plan_road(from, to, elevation_layer).error


## Works out, without changing anything, how a straight road from `from` to `to` would
## be built. Returns:
## - "error": "" or a player-facing reason it can't be built,
## - "points": where the road will have nodes, in order (its ends plus every point
##   where it meets an existing road or node on the same layer),
## - "overlaps": per consecutive pair of points, true if that piece runs along an
##   existing road and won't be built again,
## - "node_ids": empty; filled in by add_road_pieces.
func plan_road(from: Vector2, to: Vector2, elevation_layer := 0) -> Dictionary:
	var plan := { "error": "", "points": [], "overlaps": [], "node_ids": [] }
	if from.distance_to(to) < MIN_SEGMENT_LENGTH:
		plan.error = "Road is too short"
		return plan

	var points: Array[Vector2] = [from, to]
	for node: RoadNode in nodes.values():
		if node.elevation_layer == elevation_layer and _distance_to_line(node.position, from, to) <= EPSILON:
			points.append(node.position)
	for segment: RoadSegment in segments.values():
		if segment.elevation_layer != elevation_layer:
			continue
		var polyline := LaneBuilder.get_polyline(segment.centerline)
		var meets := false
		for endpoint in [from, to]:
			if _is_on_interior(endpoint, polyline):
				meets = true
		for i in polyline.size() - 1:
			var hit: Variant = Geometry2D.segment_intersects_segment(from, to, polyline[i], polyline[i + 1])
			if hit != null:
				points.append(hit)
				if _is_on_interior(hit, polyline):
					meets = true
		if meets and _is_curved(segment):
			plan.error = "Can't join a curved road in the middle yet"
			return plan

	points.sort_custom(func(a: Vector2, b: Vector2) -> bool: return from.distance_squared_to(a) < from.distance_squared_to(b))
	for point in points:
		if plan.points.is_empty() or plan.points[-1].distance_to(point) > EPSILON:
			plan.points.append(point)

	var builds_something := false
	for i in plan.points.size() - 1:
		var a: Vector2 = plan.points[i]
		var b: Vector2 = plan.points[i + 1]
		if a.distance_to(b) < MIN_SEGMENT_LENGTH:
			plan.error = "Too close to an existing junction"
			return plan
		# Pieces only meet other roads at their ends, so a piece whose middle lies on a
		# road runs along that road.
		var overlaps := find_segment_at((a + b) * 0.5, EPSILON, elevation_layer) != -1
		plan.overlaps.append(overlaps)
		builds_something = builds_something or not overlaps
	if not builds_something:
		plan.error = "Road already exists"
	return plan


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


## Nearest road lane (not connector) to `position` within `max_distance`, as
## { "lane_id": int, "distance": float along the lane }, or {} if none.
func find_lane_position(position: Vector2, max_distance: float, elevation_layer := 0) -> Dictionary:
	var best := {}
	var best_distance := max_distance
	for lane: Lane in lanes.values():
		if lane.is_connector() or segments[lane.segment_id].elevation_layer != elevation_layer:
			continue
		var distance := lane.curve.get_closest_point(position).distance_to(position)
		if distance <= best_distance:
			best_distance = distance
			best = { "lane_id": lane.id, "distance": lane.curve.get_closest_offset(position) }
	return best


## How far lanes are cut back from a node to leave room for its junction:
## half the width of the widest road meeting there. Dead ends are not trimmed.
func get_junction_radius(node_id: int) -> float:
	var node := nodes[node_id]
	if node.get_degree() < 2:
		return 0.0
	var radius := 0.0
	for segment_id in node.segment_ids:
		radius = maxf(radius, segments[segment_id].road_type.get_total_width() * 0.5)
	return radius


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
	network._refresh_junctions(network.nodes.keys())
	network._next_node_id = int(data.get("next_node_id", 1))
	network._next_segment_id = int(data.get("next_segment_id", 1))
	return network


# --- Internals --------------------------------------------------------------

func _create_node(position: Vector2, elevation_layer: int) -> int:
	var node := RoadNode.new(_next_node_id, position, elevation_layer)
	_next_node_id += 1
	nodes[node.id] = node
	return node.id


## Splits a segment at `point` (on its interior) into two segments joined by a new
## node, keeping the original direction. Returns the new node id. The caller must
## refresh the junctions at both original ends and at the new node.
func _split_segment(segment_id: int, point: Vector2) -> int:
	var old := segments[segment_id]
	var polyline := LaneBuilder.get_polyline(old.centerline)
	var at := LaneBuilder.distance_along_polyline(polyline, point)
	var first := LaneBuilder.sub_polyline(polyline, 0.0, at)
	var second := LaneBuilder.sub_polyline(polyline, at, LaneBuilder.polyline_length(polyline))
	first[first.size() - 1] = point
	second[0] = point
	_detach_segment(old)
	var node_id := _create_node(point, old.elevation_layer)
	for piece: Array in [[old.start_node_id, node_id, first], [node_id, old.end_node_id, second]]:
		_insert_segment(_next_segment_id, piece[0], piece[1], old.road_type, old.elevation_layer,
			LaneBuilder.polyline_to_curve(piece[2]))
		_next_segment_id += 1
	return node_id


## Removes a segment and its lanes from the graph, leaving its nodes in place.
func _detach_segment(segment: RoadSegment) -> void:
	for lane_id in segment.lane_ids:
		lanes.erase(lane_id)
	segments.erase(segment.id)
	nodes[segment.start_node_id].segment_ids.erase(segment.id)
	nodes[segment.end_node_id].segment_ids.erase(segment.id)


func _find_segment_with_interior_point(point: Vector2, elevation_layer: int) -> int:
	for segment: RoadSegment in segments.values():
		if segment.elevation_layer == elevation_layer and _is_on_interior(point, LaneBuilder.get_polyline(segment.centerline)):
			return segment.id
	return -1


static func _is_curved(segment: RoadSegment) -> bool:
	return LaneBuilder.has_handles(segment.centerline)


static func _distance_to_line(point: Vector2, from: Vector2, to: Vector2) -> float:
	return Geometry2D.get_closest_point_to_segment(point, from, to).distance_to(point)


func _insert_segment(id: int, start_id: int, end_id: int, road_type: RoadType,
		elevation_layer: int, centerline: Curve2D) -> RoadSegment:
	var segment := RoadSegment.new(id, start_id, end_id, road_type, elevation_layer, centerline)
	segments[id] = segment
	nodes[start_id].segment_ids.append(id)
	nodes[end_id].segment_ids.append(id)
	_create_lanes(segment)
	return segment


## Re-derives everything that depends on the given nodes' sets of segments: the
## trimmed geometry of every lane touching them, and their connectors.
## Returns the ids of the segments whose lanes were updated.
## (Lanes only change at the trimmed end, so connectors at the far nodes stay valid.)
func _refresh_junctions(node_ids: Array) -> Array:
	node_ids = _unique(node_ids)
	var segment_ids := []
	for node_id: int in node_ids:
		if not nodes.has(node_id):
			continue
		for segment_id in nodes[node_id].segment_ids:
			if not segment_ids.has(segment_id):
				segment_ids.append(segment_id)
	for segment_id: int in segment_ids:
		_update_lane_geometry(segments[segment_id])
	for node_id: int in node_ids:
		if nodes.has(node_id):
			_rebuild_connectors(nodes[node_id])
	return segment_ids


## Creates the segment's road lanes once; their ids stay stable while it exists.
## Geometry is untrimmed until _update_lane_geometry runs.
func _create_lanes(segment: RoadSegment) -> void:
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


## build_lane_specs returns lanes in a fixed order, matching segment.lane_ids.
func _update_lane_geometry(segment: RoadSegment) -> void:
	var specs := LaneBuilder.build_lane_specs(segment.centerline, segment.road_type,
		get_junction_radius(segment.start_node_id), get_junction_radius(segment.end_node_id))
	for i in specs.size():
		lanes[segment.lane_ids[i]].set_curve(specs[i].curve)


func _erase_connectors(node: RoadNode) -> void:
	for lane_id in node.connector_lane_ids:
		lanes.erase(lane_id)
	node.connector_lane_ids.clear()


func _rebuild_connectors(node: RoadNode) -> void:
	_erase_connectors(node)
	var incoming: Array[Lane] = []
	var outgoing: Array[Lane] = []
	for segment_id in node.segment_ids:
		for lane_id in segments[segment_id].lane_ids:
			var lane := lanes[lane_id]
			if lane.to_node_id == node.id:
				incoming.append(lane)
			else:
				outgoing.append(lane)

	for in_lane in incoming:
		in_lane.outgoing_lane_ids.clear()
		for out_lane in outgoing:
			var turn := _classify_movement(node, in_lane, out_lane)
			if turn == Lane.Turn.NONE:
				continue
			var connector := Lane.new(
				_next_lane_id,
				-1,
				node.id,
				node.id,
				out_lane.index_from_center,
				LaneBuilder.build_connector_curve(in_lane.curve, out_lane.curve),
				minf(in_lane.speed_limit, out_lane.speed_limit) * TURN_SPEED_FACTORS[turn])
			_next_lane_id += 1
			connector.turn = turn
			connector.outgoing_lane_ids.append(out_lane.id)
			in_lane.outgoing_lane_ids.append(connector.id)
			lanes[connector.id] = connector
			node.connector_lane_ids.append(connector.id)


## Which movement (if any) is allowed from `in_lane` into `out_lane` through `node`.
## Multi-lane roads: straight keeps its lane, right turns use the curb lanes,
## left turns the center lanes. U-turns only at dead ends.
func _classify_movement(node: RoadNode, in_lane: Lane, out_lane: Lane) -> Lane.Turn:
	var in_lane_count := segments[in_lane.segment_id].road_type.lanes_per_direction
	var out_lane_count := segments[out_lane.segment_id].road_type.lanes_per_direction
	if in_lane.segment_id == out_lane.segment_id:
		var is_dead_end := node.get_degree() == 1
		if is_dead_end and in_lane.index_from_center == 0 and out_lane.index_from_center == 0:
			return Lane.Turn.U_TURN
		return Lane.Turn.NONE

	var d0 := LaneBuilder.end_direction(in_lane.curve)
	var d1 := LaneBuilder.start_direction(out_lane.curve)
	if absf(d0.angle_to(d1)) < STRAIGHT_MAX_ANGLE:
		var target := mini(in_lane.index_from_center, out_lane_count - 1)
		return Lane.Turn.STRAIGHT if out_lane.index_from_center == target else Lane.Turn.NONE
	# y-down screen space: a positive cross product means turning clockwise, i.e. right.
	if d0.cross(d1) > 0.0:
		var from_curb := in_lane.index_from_center == in_lane_count - 1
		var to_curb := out_lane.index_from_center == out_lane_count - 1
		return Lane.Turn.RIGHT if from_curb and to_curb else Lane.Turn.NONE
	var from_center := in_lane.index_from_center == 0
	var to_center := out_lane.index_from_center == 0
	return Lane.Turn.LEFT if from_center and to_center else Lane.Turn.NONE


## True if `point` lies on the polyline but not at either of its ends.
func _is_on_interior(point: Vector2, polyline: PackedVector2Array) -> bool:
	if point.distance_to(polyline[0]) <= EPSILON or point.distance_to(polyline[-1]) <= EPSILON:
		return false
	return LaneBuilder.distance_to_polyline(point, polyline) <= EPSILON


static func _unique(values: Array) -> Array:
	var result := []
	for value in values:
		if not result.has(value):
			result.append(value)
	return result
