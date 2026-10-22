class_name RoadTool
extends BuildTool
## Builds roads between grid points. Straight (default): click-drag, kept horizontal or
## vertical. Curved (V toggles): drag from the start to the bend point (the road leaves
## the start heading there), release, then click where the road ends; the road curves
## from the start to the end, pulled toward the bend point (RoadNetwork.curve_path).
## Crossing or touching existing roads creates junctions there (white dots in the
## preview). The preview turns red and the hint explains why when the road can't be built.

const VALID_COLOR := Color(0.45, 0.85, 1.0, 0.6)
const INVALID_COLOR := Color(1.0, 0.35, 0.3, 0.6)
const JUNCTION_COLOR := Color(1, 1, 1, 0.9)
const GUIDE_COLOR := Color(1, 1, 1, 0.4)
const IDLE_HINT := "%s: drag to build (over a road: convert it; one-way: drag the way traffic goes) · R: next type · V: curved roads · Esc: cancel"
const CURVE_IDLE_HINT := "%s, curved: drag from the start to the bend point, then click the end · V: straight roads · Esc: cancel"
const CURVE_END_HINT := "%s, curved: click where the road ends · Esc: cancel"

var road_type: RoadType
## Building curves instead of straight roads.
var curved := false

var _dragging := false
## Curved mode: the start and bend point are set, the end follows the pointer.
var _placing_end := false
var _start := Vector2.ZERO
var _control := Vector2.ZERO
var _end := Vector2.ZERO
var _hover := Vector2.INF
var _error := ""
## The road being previewed (empty if none).
var _path := PackedVector2Array()
## Where the previewed road meets existing roads (junctions it will create or join).
var _junctions: Array[Vector2] = []


func _ready() -> void:
	road_type = RoadType.list_all()[0]
	super()


func shows_grid() -> bool:
	return true


func get_variants() -> Array[StringName]:
	var ids: Array[StringName] = []
	for type in RoadType.list_all():
		ids.append(type.id)
	return ids


func get_variant() -> StringName:
	return road_type.id


func set_variant(variant: StringName) -> void:
	var type := RoadType.load_by_id(variant)
	if type == null or type == road_type:
		return
	road_type = type
	Events.tool_variant_changed.emit(name, variant)
	if is_active:
		_update_preview()


func activate() -> void:
	super()
	_hover = Vector2.INF
	_update_preview()


func set_curved(value: bool) -> void:
	cancel()
	curved = value
	_update_preview()


func _process(_delta: float) -> void:
	# Polled rather than driven by mouse motion so the preview follows keyboard panning too.
	var hover := get_snapped_pointer_position()
	if hover != _hover:
		_hover = hover
		_update_preview()


func handle_input(event: InputEvent) -> bool:
	if event.is_action_pressed("road_curve_mode"):
		set_curved(not curved)
		return true
	var mouse_button := event as InputEventMouseButton
	if mouse_button == null:
		return false
	if mouse_button.button_index == MOUSE_BUTTON_RIGHT and mouse_button.pressed:
		return cancel()
	if mouse_button.button_index != MOUSE_BUTTON_LEFT:
		return false
	# Re-evaluate at the click: the last preview may be a frame old.
	_hover = get_snapped_pointer_position()
	if mouse_button.pressed:
		if _placing_end:
			_update_preview()
			if _error == "" and not _path.is_empty():
				Game.simulation.roads.add_path_pieces(_path, road_type)
			_placing_end = false
		else:
			_dragging = true
			_start = _hover
		_update_preview()
	elif _dragging:
		_update_preview()
		_dragging = false
		if curved:
			# The bend point is set: now the end.
			_placing_end = _start != _control
		elif _error == "" and not _path.is_empty():
			Game.simulation.roads.add_path_pieces(_path, road_type)
		_update_preview()
	return true


func cancel() -> bool:
	if not _dragging and not _placing_end:
		return false
	_dragging = false
	_placing_end = false
	_update_preview()
	return true


func _update_preview() -> void:
	_error = ""
	_junctions.clear()
	_path = PackedVector2Array()
	var hint := (CURVE_IDLE_HINT if curved else IDLE_HINT) % road_type.display_name
	if _dragging and curved:
		_control = _hover
	elif _dragging:
		_end = _axis_locked(_start, _hover)
		if _start != _end:
			_path = PackedVector2Array([_start, _end])
	elif _placing_end:
		_end = _hover
		hint = CURVE_END_HINT % road_type.display_name
		if _end != _start:
			_path = RoadNetwork.curve_path(_start, _control, _end) if _end != _control else PackedVector2Array([_start, _end])
	if not _path.is_empty():
		var roads := Game.roads
		var plan := roads.plan_path(_path, 0, road_type)
		_error = plan.error
		for point: Vector2 in plan.points:
			if roads.find_node_at(point) != -1 or roads.find_segment_at(point, RoadNetwork.EPSILON) != -1:
				_junctions.append(point)
		var cells := roundi(LaneBuilder.polyline_length(_path) / Grid.CELL_SIZE)
		hint = _error if _error != "" else "%s · %d cell%s" % [road_type.display_name, cells, "" if cells == 1 else "s"]
	Events.tool_hint_changed.emit(hint)
	queue_redraw()


## Keeps the road horizontal or vertical, whichever is closer to the cursor.
static func _axis_locked(from: Vector2, to: Vector2) -> Vector2:
	var delta := to - from
	if absf(delta.x) >= absf(delta.y):
		return Vector2(to.x, from.y)
	return Vector2(from.x, to.y)


func _draw() -> void:
	var width := road_type.get_total_width()
	if _dragging and curved:
		# Setting the bend point: the way the road will leave the start.
		draw_line(_start, _control, GUIDE_COLOR, 2.0)
		draw_circle(_start, width * 0.5, VALID_COLOR)
		draw_circle(_control, 4.0, GUIDE_COLOR)
		return
	if _path.is_empty():
		if _hover.is_finite():
			draw_circle(_hover, width * 0.5, VALID_COLOR)
		if _placing_end:
			draw_line(_start, _control, GUIDE_COLOR, 2.0)
		return
	if _placing_end:
		draw_polyline(PackedVector2Array([_start, _control, _end]), GUIDE_COLOR, 1.0)
	var color := INVALID_COLOR if _error != "" else VALID_COLOR
	draw_polyline(_path, color, width)
	draw_circle(_path[0], width * 0.5, color)
	draw_circle(_path[-1], width * 0.5, color)
	if _error == "":
		for point in _junctions:
			draw_circle(point, 4.0, JUNCTION_COLOR)
		if road_type.one_way:
			# Which way traffic will flow.
			RoadMarkings.draw_arrows(self, _path, JUNCTION_COLOR)
