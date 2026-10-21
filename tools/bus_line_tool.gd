class_name BusLineTool
extends BuildTool
## Draws bus lines: click roads to place stops (at the curb on the side clicked, or an
## existing stop there) in the order buses call at them; click the first stop again, or
## right-click, to close the loop and open the line. Right-clicking a stop while no line
## is being drawn removes it (lines through it skip it). Buses start running at once.

const STOP_COLOR := Color(1, 1, 1, 0.9)
const PREVIEW_COLOR := Color(1, 1, 1, 0.5)
const LINE_COLOR := Color(1, 1, 1, 0.6)
const STOP_RADIUS := 6.0
const IDLE_HINT := "Click roads to place stops in order (each side of a street is its own stop) · right-click a stop to remove it"
const DRAWING_HINT := "Line: %d stops · click the first stop again or right-click to open the line · Esc cancels"

## Stops of the line being drawn: ids, and their positions (for drawing without waiting
## for the simulation).
var _stop_ids: Array[int] = []
var _stop_positions := PackedVector2Array()
## Where a click would put a stop now ({} if not on a road).
var _hover := {}


func activate() -> void:
	super()
	_update_hint()


func cancel() -> bool:
	if _stop_ids.is_empty():
		return false
	_stop_ids.clear()
	_stop_positions.clear()
	_update_hint()
	queue_redraw()
	return true


func _process(_delta: float) -> void:
	_hover = TransitSystem.find_curb_spot(Game.roads, get_pointer_position())
	queue_redraw()


func handle_input(event: InputEvent) -> bool:
	var mouse_button := event as InputEventMouseButton
	if mouse_button == null or not mouse_button.pressed:
		return false
	var transit := Game.simulation.transit
	var point := get_pointer_position()
	if mouse_button.button_index == MOUSE_BUTTON_RIGHT:
		if not _stop_ids.is_empty():
			_open_line()
		else:
			var stop_id := transit.find_stop(point)
			if stop_id != -1:
				transit.remove_stop(stop_id)
				Events.notification_posted.emit("Bus stop removed")
		return true
	if mouse_button.button_index != MOUSE_BUTTON_LEFT:
		return false
	var stop_id := transit.add_stop(point)
	if stop_id == -1:
		Events.notification_posted.emit("Bus stops go on a road, with room for a bus")
		return true
	if not _stop_ids.is_empty() and stop_id == _stop_ids[0]:
		_open_line()
	elif not _stop_ids.has(stop_id):
		_stop_ids.append(stop_id)
		_stop_positions.append(transit.stops[stop_id].position)
	_update_hint()
	return true


func _open_line() -> void:
	var transit := Game.simulation.transit
	var line_id := transit.add_line(_stop_ids)
	if line_id == -1:
		Events.notification_posted.emit("A bus line needs at least two stops")
	else:
		var line := transit.lines[line_id]
		Events.notification_posted.emit("Bus line %d: %d stops, %d buses" % [line_id, line.stop_ids.size(), line.bus_count])
	_stop_ids.clear()
	_stop_positions.clear()
	_update_hint()


func _update_hint() -> void:
	Events.tool_hint_changed.emit(IDLE_HINT if _stop_ids.is_empty() else DRAWING_HINT % _stop_ids.size())


func _draw() -> void:
	if _stop_positions.size() >= 2:
		draw_polyline(_stop_positions, LINE_COLOR, 2.0)
	for position in _stop_positions:
		draw_circle(position, STOP_RADIUS * 0.6, STOP_COLOR)
	if not _hover.is_empty():
		var at: Vector2 = _hover.position
		if not _stop_positions.is_empty():
			draw_dashed_line(_stop_positions[-1], at, PREVIEW_COLOR, 1.5, 6.0)
		draw_arc(at, STOP_RADIUS, 0.0, TAU, 24, PREVIEW_COLOR, 2.0)
