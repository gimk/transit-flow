class_name RoadTool
extends BuildTool
## Click-drag between grid points to build a straight, axis-aligned road.
## Crossing or touching existing roads creates junctions there (white dots in the
## preview). The preview turns red and the hint explains why when the road can't be built.

const VALID_COLOR := Color(0.45, 0.85, 1.0, 0.6)
const INVALID_COLOR := Color(1.0, 0.35, 0.3, 0.6)
const JUNCTION_COLOR := Color(1, 1, 1, 0.9)
const IDLE_HINT := "Drag to build a road · Right-click / Esc to cancel"

var road_type: RoadType

var _dragging := false
var _start := Vector2.ZERO
var _end := Vector2.ZERO
var _hover := Vector2.INF
var _error := ""
## Where the dragged road meets existing roads (junctions it will create or join).
var _junctions: Array[Vector2] = []


func _ready() -> void:
	road_type = RoadType.load_by_id(&"local_2lane")
	super()


func activate() -> void:
	super()
	_hover = Vector2.INF
	Events.tool_hint_changed.emit(IDLE_HINT)


func _process(_delta: float) -> void:
	# Polled rather than driven by mouse motion so the preview follows keyboard panning too.
	var hover := get_snapped_pointer_position()
	if hover != _hover:
		_hover = hover
		_update_preview()


func handle_input(event: InputEvent) -> bool:
	var mouse_button := event as InputEventMouseButton
	if mouse_button == null:
		return false
	if mouse_button.button_index == MOUSE_BUTTON_LEFT:
		if mouse_button.pressed:
			_dragging = true
			_start = get_snapped_pointer_position()
			_update_preview()
		elif _dragging:
			# Re-evaluate at the release point: the last preview may be a frame old.
			_hover = get_snapped_pointer_position()
			_update_preview()
			_dragging = false
			if _start != _end and _error == "":
				Game.simulation.roads.add_road(_start, _end, road_type)
			_update_preview()
		return true
	if mouse_button.button_index == MOUSE_BUTTON_RIGHT and mouse_button.pressed:
		return cancel()
	return false


func cancel() -> bool:
	if not _dragging:
		return false
	_dragging = false
	_update_preview()
	return true


func _update_preview() -> void:
	_error = ""
	_junctions.clear()
	if _dragging:
		_end = _axis_locked(_start, _hover)
		if _start != _end:
			var roads := Game.simulation.roads
			var plan := roads.plan_road(_start, _end)
			_error = plan.error
			for point: Vector2 in plan.points:
				if roads.find_node_at(point) != -1 or roads.find_segment_at(point, RoadNetwork.EPSILON) != -1:
					_junctions.append(point)
		var cells := roundi(_start.distance_to(_end) / Grid.CELL_SIZE)
		Events.tool_hint_changed.emit(_error if _error != "" else "%d cell%s" % [cells, "" if cells == 1 else "s"])
	else:
		Events.tool_hint_changed.emit(IDLE_HINT)
	queue_redraw()


## Keeps the road horizontal or vertical, whichever is closer to the cursor.
static func _axis_locked(from: Vector2, to: Vector2) -> Vector2:
	var delta := to - from
	if absf(delta.x) >= absf(delta.y):
		return Vector2(to.x, from.y)
	return Vector2(from.x, to.y)


func _draw() -> void:
	var width := road_type.get_total_width()
	if not _dragging:
		if _hover.is_finite():
			draw_circle(_hover, width * 0.5, VALID_COLOR)
		return
	var color := INVALID_COLOR if _error != "" else VALID_COLOR
	draw_line(_start, _end, color, width)
	draw_circle(_start, width * 0.5, color)
	draw_circle(_end, width * 0.5, color)
	if _error == "":
		for point in _junctions:
			draw_circle(point, 4.0, JUNCTION_COLOR)
