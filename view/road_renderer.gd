class_name RoadRenderer
extends Node2D
## Draws each road segment as two Line2D strips (dark edge + asphalt surface) built
## from its centerline. All edges sit in one layer under all surfaces, so segments
## meeting at a node blend into one clean junction. Lane markings (RoadMarkings) go on
## top of all surfaces.

const EDGE_WIDTH := 3.0

var _edges := Node2D.new()
var _surfaces := Node2D.new()
var _markings := Node2D.new()
## Segment id -> [edge Line2D, surface Line2D, RoadMarkings].
var _lines: Dictionary[int, Array] = {}


func _ready() -> void:
	_edges.name = "Edges"
	_surfaces.name = "Surfaces"
	add_child(_edges)
	add_child(_surfaces)
	_markings.name = "Markings"
	add_child(_markings)
	Events.simulation_reset.connect(_rebuild_all)
	Events.road_network_changed.connect(_on_road_network_changed)
	_rebuild_all()


func _rebuild_all() -> void:
	for segment_id in _lines.keys():
		_remove_segment(segment_id)
	for segment_id in Game.simulation.roads.segments:
		_add_segment(segment_id)


func _on_road_network_changed(_node_ids: Array, segment_ids: Array) -> void:
	for segment_id: int in segment_ids:
		_remove_segment(segment_id)
		if Game.simulation.roads.segments.has(segment_id):
			_add_segment(segment_id)


func _add_segment(segment_id: int) -> void:
	var segment := Game.simulation.roads.segments[segment_id]
	var points := LaneBuilder.get_polyline(segment.centerline)
	var width := segment.road_type.get_total_width()
	var edge := _make_line(points, width + EDGE_WIDTH * 2.0, segment.road_type.edge_color)
	var surface := _make_line(points, width, segment.road_type.surface_color)
	_edges.add_child(edge)
	_surfaces.add_child(surface)
	var markings := RoadMarkings.new()
	var roads := Game.simulation.roads
	markings.setup(points, segment.road_type, roads.get_junction_radius(segment.start_node_id), roads.get_junction_radius(segment.end_node_id),
		roads.nodes[segment.start_node_id].get_degree() >= 3, roads.nodes[segment.end_node_id].get_degree() >= 3)
	_markings.add_child(markings)
	_lines[segment_id] = [edge, surface, markings]


func _remove_segment(segment_id: int) -> void:
	if not _lines.has(segment_id):
		return
	for line: Node in _lines[segment_id]:
		line.queue_free()
	_lines.erase(segment_id)


static func _make_line(points: PackedVector2Array, width: float, color: Color) -> Line2D:
	var line := Line2D.new()
	line.points = points
	line.width = width
	line.default_color = color
	line.begin_cap_mode = Line2D.LINE_CAP_ROUND
	line.end_cap_mode = Line2D.LINE_CAP_ROUND
	line.joint_mode = Line2D.LINE_JOINT_ROUND
	line.antialiased = true
	return line
