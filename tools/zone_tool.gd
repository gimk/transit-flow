class_name ZoneTool
extends BuildTool
## Drag a rectangle of cells to zone them as `zone_type`, or to clear their zoning when
## `erase` is set. Buildings appear on zoned cells next to a road after a short while.
## One scene node per zone type, plus one for erasing.

const ERASE_COLOR := Color(1.0, 0.35, 0.3, 0.35)

@export var zone_type := Lot.Type.RESIDENTIAL
@export var erase := false
@export var hint := "Drag to zone an area"

var _dragging := false
var _start_cell := Vector2i.ZERO
var _hover_cell := Vector2i.MAX


func activate() -> void:
	super()
	_hover_cell = Vector2i.MAX
	Events.tool_hint_changed.emit(hint)


func _process(_delta: float) -> void:
	var cell := Grid.world_to_cell(get_pointer_position())
	if cell != _hover_cell:
		_hover_cell = cell
		if _dragging:
			Events.tool_hint_changed.emit("%d cells" % get_selected_cells().size())
		queue_redraw()


func shows_grid() -> bool:
	return true


func handle_input(event: InputEvent) -> bool:
	var mouse_button := event as InputEventMouseButton
	if mouse_button == null:
		return false
	if mouse_button.button_index == MOUSE_BUTTON_LEFT:
		if mouse_button.pressed:
			_dragging = true
			_hover_cell = Grid.world_to_cell(get_pointer_position())
			_start_cell = _hover_cell
		elif _dragging:
			_hover_cell = Grid.world_to_cell(get_pointer_position())
			var cells := get_selected_cells()
			_dragging = false
			if erase:
				Game.simulation.zones.erase(cells)
			else:
				Game.simulation.zones.paint(cells, zone_type)
			Events.tool_hint_changed.emit(hint)
		queue_redraw()
		return true
	if mouse_button.button_index == MOUSE_BUTTON_RIGHT and mouse_button.pressed:
		return cancel()
	return false


func cancel() -> bool:
	if not _dragging:
		return false
	_dragging = false
	Events.tool_hint_changed.emit(hint)
	queue_redraw()
	return true


## The cells covered by the current drag (just the hovered cell when not dragging).
func get_selected_cells() -> Array[Vector2i]:
	var from := _start_cell if _dragging else _hover_cell
	var to := _hover_cell
	var cells: Array[Vector2i] = []
	for x in range(mini(from.x, to.x), maxi(from.x, to.x) + 1):
		for y in range(mini(from.y, to.y), maxi(from.y, to.y) + 1):
			cells.append(Vector2i(x, y))
	return cells


func _draw() -> void:
	if _hover_cell == Vector2i.MAX:
		return
	var color := ERASE_COLOR
	if not erase:
		color = LotRenderer.COLORS[zone_type]
		color.a = 0.35
	var from := _start_cell if _dragging else _hover_cell
	var top_left := Vector2i(mini(from.x, _hover_cell.x), mini(from.y, _hover_cell.y))
	var bottom_right := Vector2i(maxi(from.x, _hover_cell.x), maxi(from.y, _hover_cell.y)) + Vector2i.ONE
	var rect := Rect2(Vector2(top_left) * Grid.CELL_SIZE, Vector2(bottom_right - top_left) * Grid.CELL_SIZE)
	draw_rect(rect, color)
	draw_rect(rect, Color(color, 0.9), false, 2.0)
