class_name LotTool
extends BuildTool
## Click, or hold and sweep, to place lots of `lot_type` on empty cells.
## One scene node per lot type (ResidentialTool, IndustrialTool).

const INVALID_COLOR := Color(1.0, 0.35, 0.3, 0.5)

@export var lot_type := Lot.Type.RESIDENTIAL
@export var hint := "Click or drag to place lots"

var _hover_cell := Vector2i.MAX
var _painting := false


func activate() -> void:
	super()
	_hover_cell = Vector2i.MAX
	Events.tool_hint_changed.emit(hint)


func _process(_delta: float) -> void:
	var cell := Grid.world_to_cell(get_pointer_position())
	if cell != _hover_cell:
		_hover_cell = cell
		if _painting:
			Game.simulation.lots.add_lot(cell, lot_type)
		queue_redraw()


func handle_input(event: InputEvent) -> bool:
	var mouse_button := event as InputEventMouseButton
	if mouse_button == null:
		return false
	if mouse_button.button_index == MOUSE_BUTTON_LEFT:
		_painting = mouse_button.pressed
		if _painting:
			_hover_cell = Grid.world_to_cell(get_pointer_position())
			Game.simulation.lots.add_lot(_hover_cell, lot_type)
			queue_redraw()
		return true
	if mouse_button.button_index == MOUSE_BUTTON_RIGHT and mouse_button.pressed:
		return cancel()
	return false


func cancel() -> bool:
	var was_painting := _painting
	_painting = false
	return was_painting


func _draw() -> void:
	if _hover_cell == Vector2i.MAX:
		return
	var color: Color = LotRenderer.COLORS[lot_type]
	color.a = 0.5
	if not Game.simulation.lots.can_place(_hover_cell):
		color = INVALID_COLOR
	draw_rect(LotRenderer.get_lot_rect(_hover_cell), color)
