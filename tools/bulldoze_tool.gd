class_name BulldozeTool
extends BuildTool
## Click, or hold and sweep, to remove road segments or lots under the cursor.
## Roads win when the cursor is close to one; otherwise the lot in the cell is picked.

const HIGHLIGHT_COLOR := Color(1.0, 0.3, 0.25, 0.55)
const PICK_DISTANCE := 18.0
const HINT := "Click or drag over roads and lots to remove them"

var _hovered_segment_id := -1
var _hovered_lot_id := -1
var _held := false


func activate() -> void:
	super()
	Events.tool_hint_changed.emit(HINT)


func _process(_delta: float) -> void:
	var sim := Game.simulation
	var pointer := get_pointer_position()
	var segment_id := sim.roads.find_segment_at(pointer, PICK_DISTANCE)
	var lot_id := -1 if segment_id != -1 else sim.lots.get_lot_at_cell(Grid.world_to_cell(pointer))
	if _held:
		if segment_id != -1:
			sim.roads.remove_segment(segment_id)
			segment_id = -1
		elif lot_id != -1:
			var cell: Vector2i = sim.lots.lots[lot_id].cell
			if sim.zones.zones.has(cell):
				# Otherwise the zone would just grow the building back.
				var cells: Array[Vector2i] = [cell]
				sim.zones.erase(cells)
			else:
				sim.lots.remove_lot(lot_id)
			lot_id = -1
	if segment_id != _hovered_segment_id or lot_id != _hovered_lot_id:
		_hovered_segment_id = segment_id
		_hovered_lot_id = lot_id
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
	var sim := Game.simulation
	var lot: Lot = sim.lots.lots.get(_hovered_lot_id)
	if lot != null:
		draw_rect(lot.plot.grow(3.0), HIGHLIGHT_COLOR)
	var segment: RoadSegment = sim.roads.segments.get(_hovered_segment_id)
	if segment == null:
		return
	var points := LaneBuilder.get_polyline(segment.centerline)
	var width := segment.road_type.get_total_width() + 6.0
	draw_polyline(points, HIGHLIGHT_COLOR, width)
	draw_circle(points[0], width * 0.5, HIGHLIGHT_COLOR)
	draw_circle(points[-1], width * 0.5, HIGHLIGHT_COLOR)
