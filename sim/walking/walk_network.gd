class_name WalkNetwork
extends RefCounted
## Where pedestrians walk, derived from the roads: a sidewalk along each side of every
## road, corners joining the sidewalks of neighbouring roads around each node, and, at
## junctions of 3+ roads, a crosswalk across the mouth of each road; plus mid-block
## crosswalks (at bus stops, Crosswalks.mid_block). (A dead end is walked round; a bend or
## straight continuation just joins its sidewalks.) Rebuilt lazily after any road change;
## pedestrians cross roads only at crosswalks.
##
## Sides: +1 is the right of a road's start -> end direction. Around a node, roads are
## sorted by the angle of their direction away from the node; the sidewalk to the right of
## that direction faces the next road, so corners join it to the next road's left one.

## How far beyond the road's edge sidewalks run (px).
const SIDEWALK_OFFSET := 5.0

## Sidewalk graph; point positions are sidewalk ends.
var graph := AStar2D.new()
## Edge (Vector2i(lower point id, higher point id)) -> the crosswalk it is (Crosswalks key).
var crosswalk_edges: Dictionary[Vector2i, Vector2i] = {}
## Vector2i(segment id, side) -> Vector2i(point at the start end, point at the end end).
var sidewalks: Dictionary[Vector2i, Vector2i] = {}

var _roads: RoadNetwork
var _dirty := true
var _next_point_id := 1
## Every stretch of sidewalk between two graph points (sidewalks are split where a
## mid-block crosswalk meets them).
var _edges: Array[Vector2i] = []
## Mid-block crosswalks (Crosswalks.mid_block), and the version the graph was built with.
var _crosswalks: Crosswalks
var _built_version := -1


func _init(roads: RoadNetwork, crosswalks: Crosswalks = null) -> void:
	_crosswalks = crosswalks
	_roads = roads
	_roads.changed.connect(_on_roads_changed)


## The walking route between two places on the map (e.g. lot centers): from the nearest
## sidewalk to each, through the sidewalk graph. Returns { "points": PackedVector2Array,
## "crossings": Array[Vector2i] (per leg between consecutive points: the crosswalk it
## crosses, or Crosswalks.NONE), "length": float }, or {} if they aren't connected.
func find_path(from: Vector2, to: Vector2) -> Dictionary:
	_ensure_built()
	var start := _nearest_sidewalk(from)
	var goal := _nearest_sidewalk(to)
	if start.is_empty() or goal.is_empty():
		return {}
	if start.edge == goal.edge:
		return _make_path([], start.point, goal.point)
	var best: PackedInt64Array
	var best_length := INF
	for a: int in [start.edge.x, start.edge.y]:
		for b: int in [goal.edge.x, goal.edge.y]:
			var ids := graph.get_id_path(a, b)
			if ids.is_empty():
				continue
			var length: float = start.point.distance_to(graph.get_point_position(a)) + goal.point.distance_to(graph.get_point_position(b))
			for i in range(1, ids.size()):
				length += graph.get_point_position(ids[i - 1]).distance_to(graph.get_point_position(ids[i]))
			if length < best_length:
				best_length = length
				best = ids
	if best.is_empty():
		return {}
	return _make_path(best, start.point, goal.point)


## Walking distance between two places, or INF if they aren't connected.
func get_distance(from: Vector2, to: Vector2) -> float:
	var path := find_path(from, to)
	return path.length if not path.is_empty() else INF


func _make_path(ids: PackedInt64Array, from: Vector2, to: Vector2) -> Dictionary:
	var points := PackedVector2Array([from])
	var crossings: Array[Vector2i] = [Crosswalks.NONE]
	for i in ids.size():
		points.append(graph.get_point_position(ids[i]))
		var crosses := Crosswalks.NONE
		if i + 1 < ids.size():
			crosses = crosswalk_edges.get(Vector2i(mini(ids[i], ids[i + 1]), maxi(ids[i], ids[i + 1])), Crosswalks.NONE)
		crossings.append(crosses)
	points.append(to)
	crossings.resize(points.size() - 1)
	var length := 0.0
	for i in range(1, points.size()):
		length += points[i - 1].distance_to(points[i])
	return { "points": points, "crossings": crossings, "length": length }


## The closest point on any sidewalk: { "edge": Vector2i(point a, point b), "point" }.
func _nearest_sidewalk(position: Vector2) -> Dictionary:
	var best := {}
	var best_distance := INF
	for ends: Vector2i in _edges:
		var a := graph.get_point_position(ends.x)
		var b := graph.get_point_position(ends.y)
		var point := Geometry2D.get_closest_point_to_segment(position, a, b)
		var distance := point.distance_to(position)
		if distance < best_distance:
			best_distance = distance
			best = { "edge": ends, "point": point }
	return best


func _on_roads_changed(_node_ids: Array, _segment_ids: Array) -> void:
	_dirty = true


func _ensure_built() -> void:
	if _dirty or (_crosswalks != null and _crosswalks.mid_block_version != _built_version):
		_dirty = false
		_built_version = _crosswalks.mid_block_version if _crosswalks != null else 0
		_build()


func _build() -> void:
	graph.clear()
	crosswalk_edges.clear()
	sidewalks.clear()
	_edges.clear()
	_next_point_id = 1
	# Segment id -> its mid-block crosswalks, as [distance along the road, key].
	var mid_block: Dictionary[int, Array] = {}
	if _crosswalks != null:
		for key: Vector2i in _crosswalks.mid_block:
			var crossing: Dictionary = _crosswalks.mid_block[key]
			if not mid_block.has(crossing.segment_id):
				mid_block[crossing.segment_id] = []
			mid_block[crossing.segment_id].append([crossing.distance, key])
	# Vector3i(segment id, side, node id) -> the sidewalk end at that node.
	var ends: Dictionary[Vector3i, int] = {}
	for segment: RoadSegment in _roads.segments.values():
		if segment.elevation_layer != 0:
			continue
		var centerline := LaneBuilder.get_polyline(segment.centerline)
		var offset := segment.road_type.get_total_width() * 0.5 + SIDEWALK_OFFSET
		var start_trim := _roads.get_junction_radius(segment.start_node_id)
		var end_trim := _roads.get_junction_radius(segment.end_node_id)
		var length := LaneBuilder.polyline_length(centerline)
		var crossings: Array = mid_block.get(segment.id, []).filter(
			func(c: Array) -> bool: return c[0] > start_trim and c[0] < length - end_trim)
		crossings.sort_custom(func(x: Array, y: Array) -> bool: return x[0] < y[0])
		# Per crossing: the point on each side where it meets the sidewalk.
		var crossing_points: Array[Vector2i] = []
		crossing_points.resize(crossings.size())
		for side in [-1, 1]:
			var full_line := LaneBuilder.offset_polyline(centerline, side * offset)
			var line := LaneBuilder.trim_polyline(full_line, start_trim, end_trim)
			var a := _add_point(line[0])
			var b := _add_point(line[-1])
			var previous := a
			for i in crossings.size():
				var point := _add_point(LaneBuilder.sub_polyline(full_line, crossings[i][0], crossings[i][0])[0])
				_connect(previous, point)
				previous = point
				if side == -1:
					crossing_points[i].x = point
				else:
					crossing_points[i].y = point
			_connect(previous, b)
			sidewalks[Vector2i(segment.id, side)] = Vector2i(a, b)
			ends[Vector3i(segment.id, side, segment.start_node_id)] = a
			ends[Vector3i(segment.id, side, segment.end_node_id)] = b
		for i in crossings.size():
			var across := crossing_points[i]
			graph.connect_points(across.x, across.y)
			crosswalk_edges[Vector2i(mini(across.x, across.y), maxi(across.x, across.y))] = crossings[i][1]
	for node: RoadNode in _roads.nodes.values():
		if node.elevation_layer != 0 or node.segment_ids.is_empty():
			continue
		var arms: Array = []
		for segment_id in node.segment_ids:
			arms.append([_away_from(node, _roads.segments[segment_id]).angle(), segment_id])
		arms.sort_custom(func(a: Array, b: Array) -> bool: return a[0] < b[0])
		for i in arms.size():
			var here: int = arms[i][1]
			var next: int = arms[(i + 1) % arms.size()][1]
			var right: int = ends.get(Vector3i(here, _side(node, here, 1), node.id), -1)
			var left: int = ends.get(Vector3i(next, _side(node, next, -1), node.id), -1)
			if right != -1 and left != -1 and right != left:
				graph.connect_points(right, left)
		if node.get_degree() >= 3:
			for arm: Array in arms:
				var segment_id: int = arm[1]
				var a: int = ends.get(Vector3i(segment_id, 1, node.id), -1)
				var b: int = ends.get(Vector3i(segment_id, -1, node.id), -1)
				if a != -1 and b != -1:
					graph.connect_points(a, b)
					crosswalk_edges[Vector2i(mini(a, b), maxi(a, b))] = Vector2i(node.id, segment_id)


func _add_point(position: Vector2) -> int:
	var id := _next_point_id
	_next_point_id += 1
	graph.add_point(id, position)
	return id


## Direction of a road leaving `node`.
func _away_from(node: RoadNode, segment: RoadSegment) -> Vector2:
	var points := LaneBuilder.get_polyline(segment.centerline)
	if segment.start_node_id == node.id:
		return (points[1] - points[0]).normalized()
	return (points[-2] - points[-1]).normalized()


## The sidewalk side (relative to the road's start -> end) that is `away_side` of its
## direction away from `node`.
func _side(node: RoadNode, segment_id: int, away_side: int) -> int:
	return away_side if _roads.segments[segment_id].start_node_id == node.id else -away_side


## Joins two points with a stretch of sidewalk.
func _connect(a: int, b: int) -> void:
	graph.connect_points(a, b)
	_edges.append(Vector2i(a, b))
