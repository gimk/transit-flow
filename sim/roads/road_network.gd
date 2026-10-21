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
## Lanes between junctions must at least fit the longest vehicle (a truck) and a gap.
const MIN_LANE_LENGTH := 21.0
const TOO_CLOSE_ERROR := "Junctions too close together for these roads (wider roads need more room)"
## Connectors whose paths come closer than this (center to center) conflict: two
## vehicles (up to Vehicle.TRUCK_WIDTH wide) on them at once could touch.
const CONFLICT_DISTANCE := 8.0
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


## Like add_road, but returns the id of every segment built or rebuilt (in order from
## `from` to `to`). Pieces running along an existing road of another type are converted
## to `road_type` in place (an upgrade), and along a one-way road pointing the other
## way, reversed; pieces already of that type and direction are left alone.
func add_road_pieces(from: Vector2, to: Vector2, road_type: RoadType, elevation_layer := 0) -> Array[int]:
	var plan := plan_road(from, to, elevation_layer, road_type)
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
		var start_id: int = plan.node_ids[i]
		var end_id: int = plan.node_ids[i + 1]
		if plan.overlaps[i]:
			if plan.upgrades[i]:
				var existing := segments[get_segment_between(start_id, end_id)]
				_retype(existing, road_type, road_type.one_way and existing.start_node_id != start_id)
				touched_segments.append(existing.id)
				new_ids.append(existing.id)
			continue
		var centerline := straight_curve(nodes[start_id].position, nodes[end_id].position)
		var segment := _insert_segment(_next_segment_id, start_id, end_id, road_type, elevation_layer, centerline)
		_next_segment_id += 1
		new_ids.append(segment.id)

	for segment_id in _refresh_junctions(junction_ids):
		if not touched_segments.has(segment_id):
			touched_segments.append(segment_id)
	changed.emit(junction_ids, touched_segments)
	return new_ids


## Changes a segment's road type in place (and, if `reverse`, swaps its direction, which
## matters for one-way roads). Its nodes, junctions and the lots along it stay; its lanes
## are rebuilt with new ids, so vehicles on it re-route.
func set_segment_type(segment_id: int, road_type: RoadType, reverse := false) -> bool:
	var segment: RoadSegment = segments.get(segment_id)
	if segment == null or (segment.road_type == road_type and not reverse):
		return false
	_retype(segment, road_type, reverse)
	var touched_nodes := [segment.start_node_id, segment.end_node_id]
	var touched_segments := _refresh_junctions(touched_nodes)
	if not touched_segments.has(segment_id):
		touched_segments.append(segment_id)
	changed.emit(touched_nodes, touched_segments)
	return true


## Sets how vehicles take turns at a junction. Ignored (returns false) for nodes where
## fewer than 3 roads meet.
func set_node_control(node_id: int, control: RoadNode.JunctionControl) -> bool:
	var node: RoadNode = nodes.get(node_id)
	if node == null or (control != RoadNode.JunctionControl.NONE and node.get_degree() < 3):
		return false
	node.control = control
	changed.emit([node_id], _refresh_junctions([node_id]))
	return true


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


## Breaks the reference cycles between lanes (lanes link to each other through
## outgoing_lanes, and to the vehicles in their queues). Call before discarding a
## network, or it will never be freed.
func dispose() -> void:
	for lane: Lane in lanes.values():
		lane.outgoing_lanes.clear()
		lane.queue.clear()


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
## - "upgrades": per piece, true if it runs along an existing road that will be
##   converted to `road_type` (another type, or a one-way road to reverse); only when a
##   `road_type` is given,
## - "node_ids": empty; filled in by add_road_pieces.
func plan_road(from: Vector2, to: Vector2, elevation_layer := 0, road_type: RoadType = null) -> Dictionary:
	var plan := { "error": "", "points": [], "overlaps": [], "upgrades": [], "node_ids": [] }
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
		var existing_id := find_segment_at((a + b) * 0.5, EPSILON, elevation_layer)
		var overlaps := existing_id != -1
		var upgrade := overlaps and road_type != null and _needs_retype(segments[existing_id], road_type, b - a)
		plan.overlaps.append(overlaps)
		plan.upgrades.append(upgrade)
		builds_something = builds_something or not overlaps or upgrade
	if not builds_something:
		plan.error = "Road already exists"
	elif road_type != null:
		plan.error = _check_lane_room(plan, road_type, elevation_layer)
	return plan


## "" if, once the planned road is built, every lane it creates, converts or shortens
## still fits the longest vehicle between its junctions (lanes are cut back from each
## junction by half its widest road: two avenue junctions one cell apart would leave 8 px,
## which no vehicle fits, and traffic there would crawl through one at a time).
func _check_lane_room(plan: Dictionary, road_type: RoadType, elevation_layer: int) -> String:
	var points: Array = plan.points
	var count := points.size()
	var half_width := road_type.get_total_width() * 0.5
	# How far lanes will be cut back at each planned point (0 at a dead end).
	var trims: Array[float] = []
	for i in count:
		var node_id := find_node_at(points[i], elevation_layer)
		var degree := 0
		var widest := 0.0
		if node_id != -1:
			degree = nodes[node_id].get_degree()
			widest = get_junction_radius(node_id) if degree >= 2 else segments[nodes[node_id].segment_ids[0]].road_type.get_total_width() * 0.5
		else:
			var split_id := _find_segment_with_interior_point(points[i], elevation_layer)
			if split_id != -1:
				degree = 2
				widest = segments[split_id].road_type.get_total_width() * 0.5
		for piece in [i - 1, i]:
			if piece < 0 or piece >= count - 1:
				continue
			if not plan.overlaps[piece]:
				degree += 1
			if not plan.overlaps[piece] or plan.upgrades[piece]:
				widest = maxf(widest, half_width)
		trims.append(widest if degree >= 2 else 0.0)
	# The new road's own pieces (and those it converts).
	for i in count - 1:
		if plan.overlaps[i] and not plan.upgrades[i]:
			continue
		if points[i].distance_to(points[i + 1]) - trims[i] - trims[i + 1] < MIN_LANE_LENGTH:
			return TOO_CLOSE_ERROR
	# Existing roads it meets or splits: their stretches between (old and new) junctions.
	for segment: RoadSegment in segments.values():
		if segment.elevation_layer != elevation_layer:
			continue
		var polyline := LaneBuilder.get_polyline(segment.centerline)
		var start := nodes[segment.start_node_id].position
		var end := nodes[segment.end_node_id].position
		# [distance along the segment, trim there]
		var stops: Array = [[0.0, get_junction_radius(segment.start_node_id)], [segment.get_length(), get_junction_radius(segment.end_node_id)]]
		var touched := false
		for i in count:
			if points[i].distance_to(start) <= EPSILON:
				stops[0][1] = trims[i]
				touched = true
			elif points[i].distance_to(end) <= EPSILON:
				stops[1][1] = trims[i]
				touched = true
			elif _is_on_interior(points[i], polyline):
				stops.append([LaneBuilder.distance_along_polyline(polyline, points[i]), trims[i]])
				touched = true
		if not touched:
			continue
		stops.sort_custom(func(a: Array, b: Array) -> bool: return a[0] < b[0])
		for k in stops.size() - 1:
			if stops[k + 1][0] - stops[k][0] - stops[k][1] - stops[k + 1][1] < MIN_LANE_LENGTH:
				return TOO_CLOSE_ERROR
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
			"control": RoadNode.JunctionControl.find_key(node.control).to_lower(),
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
		node.control = _control_from_save(entry.get("control", "none"))
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
		lanes[lane_id].outgoing_lanes.clear()
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


## Would building `road_type` along `segment`, dragged in `direction`, change it?
func _needs_retype(segment: RoadSegment, road_type: RoadType, direction: Vector2) -> bool:
	if segment.road_type != road_type:
		return true
	var segment_direction := nodes[segment.end_node_id].position - nodes[segment.start_node_id].position
	return road_type.one_way and segment_direction.dot(direction) < 0.0


## Replaces a segment's lanes for a new road type (and direction). The caller refreshes
## the junctions at both ends and emits `changed`.
func _retype(segment: RoadSegment, road_type: RoadType, reverse: bool) -> void:
	for lane_id in segment.lane_ids:
		# Vehicles still on a removed lane re-snap or re-route from the lane object.
		lanes[lane_id].outgoing_lanes.clear()
		lanes.erase(lane_id)
	segment.lane_ids.clear()
	if reverse:
		var start_id := segment.start_node_id
		segment.start_node_id = segment.end_node_id
		segment.end_node_id = start_id
		segment.centerline = _reversed_curve(segment.centerline)
	segment.road_type = road_type
	_create_lanes(segment)


static func _reversed_curve(curve: Curve2D) -> Curve2D:
	var reversed := Curve2D.new()
	for i in range(curve.point_count - 1, -1, -1):
		reversed.add_point(curve.get_point_position(i), curve.get_point_out(i), curve.get_point_in(i))
	return reversed


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
		lane.bus_only = segment.road_type.is_bus_lane(spec.index)
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
		# Vehicles still on a removed connector only need its outgoing_lane_ids.
		lanes[lane_id].outgoing_lanes.clear()
		lanes.erase(lane_id)
	node.connector_lane_ids.clear()


func _rebuild_connectors(node: RoadNode) -> void:
	_erase_connectors(node)
	if node.get_degree() < 3:
		node.control = RoadNode.JunctionControl.NONE
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
		in_lane.outgoing_lanes.clear()
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
			connector.outgoing_lanes.append(out_lane)
			connector.feeder_lane_id = in_lane.id
			connector.bus_only = in_lane.bus_only or out_lane.bus_only
			in_lane.outgoing_lane_ids.append(connector.id)
			in_lane.outgoing_lanes.append(connector)
			lanes[connector.id] = connector
			node.connector_lane_ids.append(connector.id)
	_compute_conflicts(node)
	_assign_signal_phases(node, incoming)


## Traffic lights alternate between the two road axes: lanes arriving east-west belong
## to axis 0, lanes arriving north-south to axis 1 (by their direction at the stop line).
## A connector's phase is its feeder lane's axis; with protected left turns, left turns
## and U-turns get a phase of their own after their axis's through phase:
## 0 axis-0 through, 1 axis-0 left, 2 axis-1 through, 3 axis-1 left. Phases nobody uses
## are simply never asked for (TrafficLights skips phases without demand).
func _assign_signal_phases(node: RoadNode, incoming: Array[Lane]) -> void:
	var axes := {}
	for lane in incoming:
		var direction := LaneBuilder.end_direction(lane.curve)
		lane.signal_phase = 0 if absf(direction.x) >= absf(direction.y) else 1
		axes[lane.signal_phase] = true
	if axes.size() == 1:
		for lane in incoming:
			lane.signal_phase = 0
	var left_phases := node.control == RoadNode.JunctionControl.TRAFFIC_LIGHT_PROTECTED_LEFT and axes.size() > 1
	node.signal_phase_count = 4 if left_phases else axes.size()
	for lane_id in node.connector_lane_ids:
		var connector := lanes[lane_id]
		var axis := lanes[connector.feeder_lane_id].signal_phase
		if left_phases:
			var turning_across := connector.turn == Lane.Turn.LEFT or connector.turn == Lane.Turn.U_TURN
			connector.signal_phase = axis * 2 + (1 if turning_across else 0)
		else:
			connector.signal_phase = axis


## Finds which of the node's connectors conflict and where: two connectors conflict
## where their paths come closer than CONFLICT_DISTANCE (crossing or merging). Each
## records the stretch of its own path that is shared (its conflict zone), so vehicles
## only have to keep out of each other's way there, not for the whole junction.
## Connectors leaving the same lane don't conflict: vehicles on them already follow
## each other.
func _compute_conflicts(node: RoadNode) -> void:
	var connectors: Array[Lane] = []
	var polylines: Array[PackedVector2Array] = []
	for lane_id in node.connector_lane_ids:
		connectors.append(lanes[lane_id])
		polylines.append(LaneBuilder.get_polyline(lanes[lane_id].curve))
	for i in connectors.size():
		for j in range(i + 1, connectors.size()):
			var a := connectors[i]
			var b := connectors[j]
			if a.feeder_lane_id == b.feeder_lane_id:
				continue
			var merge := a.outgoing_lane_ids[0] == b.outgoing_lane_ids[0]
			if not merge and LaneBuilder.polyline_distance(polylines[i], polylines[j]) >= CONFLICT_DISTANCE:
				continue
			var zone_a := LaneBuilder.conflict_zone(polylines[i], polylines[j], CONFLICT_DISTANCE, 2.0)
			var zone_b := LaneBuilder.conflict_zone(polylines[j], polylines[i], CONFLICT_DISTANCE, 2.0)
			if merge:
				# Merging into one lane: at the very least they meet where they end.
				zone_a = _with_end_zone(zone_a, a.length)
				zone_b = _with_end_zone(zone_b, b.length)
			if zone_a.x < 0.0 or zone_b.x < 0.0:
				continue
			a.conflicting_lane_ids.append(b.id)
			b.conflicting_lane_ids.append(a.id)
			a.conflict_zones[b.id] = zone_a
			b.conflict_zones[a.id] = zone_b


## Junction control is saved by name. Older saves stored the enum number, back when it
## also had stop signs (0 none, 1 stop sign, 2 lights): stop signs load as no control.
static func _control_from_save(value: Variant) -> RoadNode.JunctionControl:
	if value is String:
		var key: String = value.to_upper()
		return RoadNode.JunctionControl.get(key, RoadNode.JunctionControl.NONE)
	return RoadNode.JunctionControl.TRAFFIC_LIGHT if int(value) == 2 else RoadNode.JunctionControl.NONE


static func _with_end_zone(zone: Vector2, length: float) -> Vector2:
	var end_zone := Vector2(maxf(length - Vehicle.MAX_LENGTH, 0.0), length)
	if zone.x < 0.0:
		return end_zone
	return Vector2(minf(zone.x, end_zone.x), length)


## Which movement (if any) is allowed from `in_lane` into `out_lane` through `node`.
## Multi-lane roads: straight keeps its lane, right turns use the curb lanes,
## left turns the center lanes. U-turns only at dead ends. Bus lanes (RoadType.bus_lanes)
## are left out of the lanes everyone uses: the curb lane for cars is then the one next
## to the bus lane (buses may also turn right from it into a bus lane); a bus lane goes on
## straight into the next road's curb lane (its bus lane, if it has one) or turns right
## into it.
func _classify_movement(node: RoadNode, in_lane: Lane, out_lane: Lane) -> Lane.Turn:
	var out_type := segments[out_lane.segment_id].road_type
	var in_general := segments[in_lane.segment_id].road_type.get_general_lane_count()
	var out_count := out_type.lanes_per_direction
	var out_general := out_type.get_general_lane_count()
	if in_lane.segment_id == out_lane.segment_id:
		var is_dead_end := node.get_degree() == 1
		if is_dead_end and in_lane.index_from_center == 0 and out_lane.index_from_center == 0:
			return Lane.Turn.U_TURN
		return Lane.Turn.NONE

	var d0 := LaneBuilder.end_direction(in_lane.curve)
	var d1 := LaneBuilder.start_direction(out_lane.curve)
	if absf(d0.angle_to(d1)) < STRAIGHT_MAX_ANGLE:
		var target := out_count - 1 if in_lane.bus_only else mini(in_lane.index_from_center, out_general - 1)
		return Lane.Turn.STRAIGHT if out_lane.index_from_center == target else Lane.Turn.NONE
	# y-down screen space: a positive cross product means turning clockwise, i.e. right.
	if d0.cross(d1) > 0.0:
		if in_lane.bus_only:
			return Lane.Turn.RIGHT if out_lane.index_from_center == out_count - 1 else Lane.Turn.NONE
		var from_curb := in_lane.index_from_center == in_general - 1
		var to_curb := out_lane.index_from_center == out_general - 1
		# Buses may also turn right straight into a bus lane (a bus-only turn: see bus_only).
		to_curb = to_curb or out_lane.bus_only
		return Lane.Turn.RIGHT if from_curb and to_curb else Lane.Turn.NONE
	if in_lane.bus_only:
		return Lane.Turn.NONE
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
