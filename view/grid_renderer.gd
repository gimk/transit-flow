class_name GridRenderer
extends Node2D
## Draws the map background and the building grid, limited to the visible area.
## Minor lines are hidden when zoomed out far enough that they would turn into noise.

const MAJOR_EVERY := 8
const MIN_MINOR_SPACING_PX := 12.0

@export var map_color := Color("1f2933")
@export var minor_color := Color(1, 1, 1, 0.05)
@export var major_color := Color(1, 1, 1, 0.12)
@export var border_color := Color(1, 1, 1, 0.35)

var _visible_rect := Rect2()


func _process(_delta: float) -> void:
	var rect := _compute_visible_rect()
	if rect != _visible_rect:
		_visible_rect = rect
		queue_redraw()


func _draw() -> void:
	var map := Grid.map_rect()
	draw_rect(map, map_color)

	var view := _visible_rect.intersection(map)
	if view.has_area():
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
			draw_multiline(minor, minor_color)
		if not major.is_empty():
			draw_multiline(major, major_color)

	draw_rect(map, border_color, false)


func _compute_visible_rect() -> Rect2:
	var to_world := get_viewport().get_canvas_transform().affine_inverse()
	var screen := get_viewport_rect()
	return Rect2(to_world * screen.position, Vector2.ZERO).expand(to_world * screen.end)
