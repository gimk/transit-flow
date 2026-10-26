class_name RoadRenderer
extends Node2D
## Draws each road segment as Line2D strips built from its centerline, in layers so
## segments meeting at a node blend into one clean junction: sidewalks (where people
## walk, WalkNetwork), the kerb, the gutter (the road type's edge colour), the asphalt,
## then bus lanes (a RoadMarkings each, BusLanes: their own layer in the dot display) and
## lane markings (RoadMarkings). Where roads meet, the inside corners are rounded
## (corner_fillets): the kerb curves round, and the back of the sidewalk with it. Paving
## gets a soft grain (surface_grain.gdshader).

## Sidewalk width beyond the kerb (people walk WalkNetwork.SIDEWALK_OFFSET out; lots start
## at LotSystem.ROAD_CLEARANCE).
const SIDEWALK_WIDTH := LotSystem.ROAD_CLEARANCE
const KERB_WIDTH := 1.5
const GUTTER_WIDTH := 0.75
const SIDEWALK_COLOR := Color(0.56, 0.56, 0.53)
const KERB_COLOR := Color(0.72, 0.72, 0.69)
## Radius of the kerb round an inside corner where roads meet.
const CORNER_RADIUS := 10.0
const CORNER_POINTS := 8

var _sidewalks := Node2D.new()
var _sidewalk_corners := _CornerLayer.new()
var _edges := Node2D.new()
var _surfaces := Node2D.new()
var _corners := _CornerLayer.new()
var _bus_lanes := Node2D.new()
var _markings := Node2D.new()
## Segment id -> [sidewalk, kerb, gutter, surface Line2D, RoadMarkings, and the bus lane
## RoadMarkings on roads with bus lanes].
var _lines: Dictionary[int, Array] = {}


## Draws one part of every junction corner (sidewalk or asphalt), from RoadRenderer.
class _CornerLayer:
	extends Node2D
	var corners: Array[Dictionary] = []
	var sidewalk := false

	func _draw() -> void:
		for corner in corners:
			if sidewalk:
				draw_colored_polygon(corner.sidewalk, SIDEWALK_COLOR)
			else:
				# Solid bands, not an antialiased line: its feathering is in world units, so
				# it blurs when zoomed in.
				draw_colored_polygon(corner.asphalt, corner.color)
				draw_colored_polygon(corner.kerb_band, KERB_COLOR)
				draw_colored_polygon(corner.gutter_band, corner.edge_color)


func _ready() -> void:
	var grain := ShaderMaterial.new()
	grain.shader = preload("res://view/surface_grain.gdshader")
	var sidewalk_grain := grain.duplicate() as ShaderMaterial
	sidewalk_grain.set_shader_parameter("scale", 0.6)
	sidewalk_grain.set_shader_parameter("strength", 0.06)
	for layer: Array in [[_sidewalks, "Sidewalks", sidewalk_grain], [_sidewalk_corners, "SidewalkCorners", sidewalk_grain],
			[_edges, "Edges", null], [_surfaces, "Surfaces", grain], [_corners, "Corners", grain], [_bus_lanes, "BusLanes", null],
			[_markings, "Markings", null]]:
		var node: Node2D = layer[0]
		node.name = layer[1]
		node.material = layer[2]
		add_child(node)
	_sidewalk_corners.sidewalk = true
	Events.simulation_reset.connect(_rebuild_all)
	Events.road_network_changed.connect(_on_road_network_changed)
	_rebuild_all()


func _rebuild_all() -> void:
	for segment_id in _lines.keys():
		_remove_segment(segment_id)
	for segment_id in Game.simulation.roads.segments:
		_add_segment(segment_id)
	_update_corners()


func _on_road_network_changed(_node_ids: Array, segment_ids: Array) -> void:
	for segment_id: int in segment_ids:
		_remove_segment(segment_id)
		if Game.simulation.roads.segments.has(segment_id):
			_add_segment(segment_id)
	_update_corners()


func _add_segment(segment_id: int) -> void:
	var segment := Game.simulation.roads.segments[segment_id]
	var points := LaneBuilder.get_polyline(segment.centerline)
	var road_type := segment.road_type
	var width := road_type.get_total_width()
	var sidewalk := _make_line(points, width + SIDEWALK_WIDTH * 2.0, SIDEWALK_COLOR)
	var kerb := _make_line(points, width + KERB_WIDTH * 2.0, KERB_COLOR)
	var gutter := _make_line(points, width + GUTTER_WIDTH * 2.0, road_type.edge_color)
	var surface := _make_line(points, width, road_type.surface_color)
	_sidewalks.add_child(sidewalk)
	_edges.add_child(kerb)
	_edges.add_child(gutter)
	_surfaces.add_child(surface)
	var markings := RoadMarkings.new()
	var roads := Game.simulation.roads
	markings.setup(points, road_type, roads.get_junction_radius(segment.start_node_id), roads.get_junction_radius(segment.end_node_id),
		roads.nodes[segment.start_node_id].get_degree() >= 3, roads.nodes[segment.end_node_id].get_degree() >= 3)
	_markings.add_child(markings)
	_lines[segment_id] = [sidewalk, kerb, gutter, surface, markings]
	if road_type.bus_lanes and not road_type.one_way:
		var bus_lanes := RoadMarkings.new()
		bus_lanes.bus_lanes_only = true
		bus_lanes.setup(points, road_type, roads.get_junction_radius(segment.start_node_id), roads.get_junction_radius(segment.end_node_id))
		_bus_lanes.add_child(bus_lanes)
		_lines[segment_id].append(bus_lanes)


func _remove_segment(segment_id: int) -> void:
	if not _lines.has(segment_id):
		return
	for line: Node in _lines[segment_id]:
		line.queue_free()
	_lines.erase(segment_id)


## Recomputes every junction corner (cheap: a few per node).
func _update_corners() -> void:
	var roads := Game.simulation.roads
	var corners: Array[Dictionary] = []
	for node: RoadNode in roads.nodes.values():
		corners.append_array(corner_fillets(roads, node))
	_corners.corners = corners
	_sidewalk_corners.corners = corners
	_corners.queue_redraw()
	_sidewalk_corners.queue_redraw()


## The rounded inside corners between neighbouring roads at a node (none on the outside
## of a bend, or between roads running nearly straight on): per corner, "asphalt" (the
## polygon added to the road to round its kerb), "kerb" (the curve where asphalt meets
## kerb), "kerb_band" and "gutter_band" (polygons along it, as wide as the straight kerb
## and gutter), "sidewalk" (the polygon added to round the back of the sidewalk), "color"
## and "edge_color" (the asphalt's and gutter's).
static func corner_fillets(roads: RoadNetwork, node: RoadNode) -> Array[Dictionary]:
	var arms: Array[Dictionary] = []
	for segment_id in node.segment_ids:
		var segment: RoadSegment = roads.segments.get(segment_id)
		if segment == null or segment.start_node_id == segment.end_node_id:
			continue
		var points := LaneBuilder.get_polyline(segment.centerline)
		var direction := points[1] - points[0] if segment.start_node_id == node.id else points[-2] - points[-1]
		arms.append({ "direction": direction.normalized(), "half": segment.road_type.get_total_width() * 0.5,
			"color": segment.road_type.surface_color, "edge_color": segment.road_type.edge_color })
	var corners: Array[Dictionary] = []
	if arms.size() < 2:
		return corners
	arms.sort_custom(func(a: Dictionary, b: Dictionary) -> bool: return a.direction.angle() < b.direction.angle())
	for i in arms.size():
		var a: Dictionary = arms[i]
		var b: Dictionary = arms[(i + 1) % arms.size()]
		var wedge := wrapf((b.direction as Vector2).angle() - (a.direction as Vector2).angle(), 0.0, TAU)
		if wedge < 0.3 or wedge > PI - 0.15:
			continue
		# Normals pointing into the corner, from each road toward the other.
		var na := (a.direction as Vector2).rotated(PI * 0.5)
		var nb := (b.direction as Vector2).rotated(-PI * 0.5)
		var half_a: float = a.half
		var half_b: float = b.half
		var center := node.position + _solve(na, nb, half_a + CORNER_RADIUS, half_b + CORNER_RADIUS)
		var kerb := _arc(center, center - na * CORNER_RADIUS, center - nb * CORNER_RADIUS, CORNER_RADIUS)
		var asphalt := PackedVector2Array([node.position + _solve(na, nb, half_a, half_b)])
		asphalt.append_array(kerb)
		var back := CORNER_RADIUS - SIDEWALK_WIDTH
		var sidewalk := PackedVector2Array([node.position + _solve(na, nb, half_a + SIDEWALK_WIDTH, half_b + SIDEWALK_WIDTH)])
		sidewalk.append_array(_arc(center, center - na * back, center - nb * back, back))
		# Wider roads first: their asphalt colour wins where corners meet.
		var wider: Dictionary = a if half_a >= half_b else b
		corners.append({ "asphalt": asphalt, "kerb": kerb, "sidewalk": sidewalk,
			"kerb_band": _band(kerb, center, CORNER_RADIUS - KERB_WIDTH),
			"gutter_band": _band(kerb, center, CORNER_RADIUS - GUTTER_WIDTH),
			"color": wider.color, "edge_color": wider.edge_color })
	return corners


## The point p with p·na = va and p·nb = vb.
static func _solve(na: Vector2, nb: Vector2, va: float, vb: float) -> Vector2:
	var det := na.x * nb.y - na.y * nb.x
	return Vector2(va * nb.y - vb * na.y, na.x * vb - nb.x * va) / det


## Points along the short arc round `center` from `from` to `to`.
static func _arc(center: Vector2, from: Vector2, to: Vector2, radius: float) -> PackedVector2Array:
	var start := (from - center).angle()
	var sweep := wrapf((to - center).angle() - start, -PI, PI)
	var points := PackedVector2Array()
	for k in CORNER_POINTS + 1:
		points.append(center + Vector2.from_angle(start + sweep * k / CORNER_POINTS) * radius)
	return points


## A band along an arc round `center`, from the arc in to `inner_radius`.
static func _band(arc: PackedVector2Array, center: Vector2, inner_radius: float) -> PackedVector2Array:
	var band := arc.duplicate()
	for k in range(arc.size() - 1, -1, -1):
		band.append(center + (arc[k] - center).normalized() * inner_radius)
	return band


static func _make_line(points: PackedVector2Array, width: float, color: Color) -> Line2D:
	var line := Line2D.new()
	line.points = points
	line.width = width
	line.default_color = color
	line.begin_cap_mode = Line2D.LINE_CAP_ROUND
	line.end_cap_mode = Line2D.LINE_CAP_ROUND
	line.joint_mode = Line2D.LINE_JOINT_ROUND
	line.antialiased = true
	line.use_parent_material = true
	return line
