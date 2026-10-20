class_name BulldozeTool
extends BuildTool
## Click, or hold and sweep, to remove road segments under the cursor.

const HIGHLIGHT_COLOR := Color(1.0, 0.3, 0.25, 0.55)
const PICK_DISTANCE := 18.0
const HINT := "Click or drag over roads to remove them"

var _hovered_segment_id := -1
var _held := false


func activate() -> void:
	super()
	Events.tool_hint_changed.emit(HINT)


func _process(_delta: float) -> void:
	var roads := Game.simulation.roads
	var hovered := roads.find_segment_at(get_pointer_position(), PICK_DISTANCE)
	if _held and hovered != -1:
		roads.remove_segment(hovered)
		hovered = -1
	if hovered != _hovered_segment_id:
		_hovered_segment_id = hovered
		queue_redraw()


func handle_input(event: InputEvent) -> bool:
	var mouse_button := event as InputEventMouseButton
	if mouse_button != null and mouse_button.button_index == MOUSE_BUTTON_LEFT:
		_held = mouse_button.pressed
		return true
	return false


func cancel() -> bool:
	var was_held := _held
	_held = false
	return was_held


func _draw() -> void:
	var segment: RoadSegment = Game.simulation.roads.segments.get(_hovered_segment_id)
	if segment == null:
		return
	var points := LaneBuilder.get_polyline(segment.centerline)
	var width := segment.road_type.get_total_width() + 6.0
	draw_polyline(points, HIGHLIGHT_COLOR, width)
	draw_circle(points[0], width * 0.5, HIGHLIGHT_COLOR)
	draw_circle(points[-1], width * 0.5, HIGHLIGHT_COLOR)
