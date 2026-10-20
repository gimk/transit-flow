class_name ExternalRenderer
extends Node2D
## Marks the city's connections to the outside world with a highway sign on the map edge
## (greyed out while no road reaches it).

const SIGN_SIZE := Vector2(46, 22)
const ACTIVE_COLOR := Color("1e88e5")
const INACTIVE_COLOR := Color(0.5, 0.5, 0.5, 0.7)


func _ready() -> void:
	Events.simulation_reset.connect(queue_redraw)
	Events.externals_changed.connect(queue_redraw)
	Events.road_network_changed.connect(func(_nodes: Array, _segments: Array) -> void: queue_redraw())


func _draw() -> void:
	var externals := Game.simulation.externals
	var active := externals.get_active_points()
	var font := ThemeDB.fallback_font
	for point in externals.points:
		var color := ACTIVE_COLOR if active.has(point) else INACTIVE_COLOR
		var rect := Rect2(point + Vector2(4, -SIGN_SIZE.y - 18), SIGN_SIZE)
		draw_rect(rect, color)
		draw_rect(rect, Color.WHITE, false, 1.5)
		draw_string(font, rect.position + Vector2(0, 16), "HWY", HORIZONTAL_ALIGNMENT_CENTER, SIGN_SIZE.x, 14, Color.WHITE)
