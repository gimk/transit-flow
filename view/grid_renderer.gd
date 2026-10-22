class_name GridRenderer
extends Node2D
## Draws the map's ground (ground.gdshader: varied grass in `map_color`) and, over it on
## a child layer, the building grid, limited to the visible area. The grid only shows
## (fading in and out) while a tool that snaps to it is active (BuildTool.shows_grid:
## roads, zoning), and faintly: the rest of the time it would only clutter the map.
## Minor lines are hidden when zoomed out far enough that they would turn into noise.
## The map border always shows.

const MAJOR_EVERY := 8
const MIN_MINOR_SPACING_PX := 12.0
## Seconds for the grid to fade in or out.
const FADE_TIME := 0.15

@export var map_color := Color("1f2933")
@export var minor_color := Color(1, 1, 1, 0.14)
@export var major_color := Color(1, 1, 1, 0.24)
@export var border_color := Color(1, 1, 1, 0.35)

var _visible_rect := Rect2()
## The grid lines, drawn without the ground shader.
var _lines := Node2D.new()
## How much of the grid shows (0..1), and toward what.
var _grid_alpha := 0.0
var _grid_target := 0.0


func _ready() -> void:
	var ground := ShaderMaterial.new()
	ground.shader = preload("res://view/ground.gdshader")
	material = ground
	_lines.name = "Lines"
	add_child(_lines)
	_lines.draw.connect(_draw_lines)
	Events.tool_changed.connect(_on_tool_changed)


func _process(delta: float) -> void:
	var rect := _compute_visible_rect()
	if rect != _visible_rect:
		_visible_rect = rect
		_lines.queue_redraw()
	if _grid_alpha != _grid_target:
		_grid_alpha = move_toward(_grid_alpha, _grid_target, delta / FADE_TIME)
		_lines.queue_redraw()


func _on_tool_changed(tool_id: StringName) -> void:
	# Tools live under the ToolManager node, named by their id.
	var tool: BuildTool = null
	if tool_id != &"":
		tool = get_node_or_null(NodePath("../Tools/" + String(tool_id))) as BuildTool
	_grid_target = 1.0 if tool != null and tool.shows_grid() else 0.0


func _draw() -> void:
	draw_rect(Grid.map_rect(), map_color)


func _draw_lines() -> void:
	var map := Grid.map_rect()
	var view := _visible_rect.intersection(map)
	if view.has_area() and _grid_alpha > 0.0:
		var cell := Grid.CELL_SIZE
		var screen_scale := get_viewport().get_canvas_transform().get_scale().x
		var draw_minor := cell * screen_scale >= MIN_MINOR_SPACING_PX
		var minor := PackedVector2Array()
		var major := PackedVector2Array()

		for i in range(ceili(view.position.x / cell), floori(view.end.x / cell) + 1):
			var from := Vector2(i * cell, view.position.y)
			var to := Vector2(i * cell, view.end.y)
			if i % MAJOR_EVERY == 0:
				major.append(from)
				major.append(to)
			elif draw_minor:
				minor.append(from)
				minor.append(to)
		for j in range(ceili(view.position.y / cell), floori(view.end.y / cell) + 1):
			var from := Vector2(view.position.x, j * cell)
			var to := Vector2(view.end.x, j * cell)
			if j % MAJOR_EVERY == 0:
				major.append(from)
				major.append(to)
			elif draw_minor:
				minor.append(from)
				minor.append(to)

		if not minor.is_empty():
			_lines.draw_multiline(minor, Color(minor_color, minor_color.a * _grid_alpha))
		if not major.is_empty():
			_lines.draw_multiline(major, Color(major_color, major_color.a * _grid_alpha))

	_lines.draw_rect(map, border_color, false)


func _compute_visible_rect() -> Rect2:
	var to_world := get_viewport().get_canvas_transform().affine_inverse()
	var screen := get_viewport_rect()
	return Rect2(to_world * screen.position, Vector2.ZERO).expand(to_world * screen.end)
