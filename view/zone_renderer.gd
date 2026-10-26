class_name ZoneRenderer
extends Node2D
## Draws zoned cells that have no building yet as a light tint of their zone colour, on
## what the roads leave of the cell (LotSystem.compute_plot), under roads and buildings;
## the dot display marks them as hollow rings (DotDisplay). Cells still waiting for a
## road, or for room (a road built too close leaves none), get a dashed red outline so
## the player sees why nothing grows there (under the dot display, AlertRenderer marks
## them instead).

const FILL_ALPHA := 0.16
const NO_ROAD_COLOR := Color(1.0, 0.35, 0.3, 0.6)

## The dot display is on.
var _dot_display := true


func _ready() -> void:
	Events.simulation_reset.connect(queue_redraw)
	Events.zones_changed.connect(queue_redraw)
	Events.lots_changed.connect(queue_redraw)
	Events.debug_overlay_changed.connect(_on_overlay_changed)
	Events.road_network_changed.connect(func(_nodes: Array, _segments: Array) -> void: queue_redraw())


func _draw() -> void:
	var simulation := Game.simulation
	var zones := simulation.zones
	var size := Vector2.ONE * Grid.CELL_SIZE
	for cell: Vector2i in zones.zones:
		var rect := Rect2(Vector2(cell) * Grid.CELL_SIZE, size)
		if simulation.lots.get_lot_at_cell(cell) == -1:
			var plot := LotSystem.compute_plot(simulation.roads, cell)
			var color: Color = LotRenderer.COLORS[zones.zones[cell]]
			color.a = FILL_ALPHA
			draw_rect(plot if plot.has_area() else rect, color)
		if zones.is_waiting_for_road(cell) and not _dot_display:
			var inner := rect.grow(-6.0)
			var corners := [inner.position, Vector2(inner.end.x, inner.position.y), inner.end,
				Vector2(inner.position.x, inner.end.y), inner.position]
			for i in 4:
				draw_dashed_line(corners[i], corners[i + 1], NO_ROAD_COLOR, 1.5, 4.0)


func _on_overlay_changed(overlay_id: StringName, shown: bool) -> void:
	if overlay_id == DotDisplay.OVERLAY_ID:
		_dot_display = shown
		queue_redraw()
