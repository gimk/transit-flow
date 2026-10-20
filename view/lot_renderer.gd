class_name LotRenderer
extends Node2D
## Draws lots as colored squares inset in their cell, with a short "driveway" to their
## road access point, or dimmed with a red outline when they have no road access.

const INSET := 14.0
const COLORS := {
	Lot.Type.RESIDENTIAL: Color("4caf50"),
	Lot.Type.COMMERCIAL: Color("42a5f5"),
	Lot.Type.INDUSTRIAL: Color("f9a825"),
}
const NO_ACCESS_COLOR := Color(1.0, 0.3, 0.25, 0.9)


static func get_lot_rect(cell: Vector2i) -> Rect2:
	return Rect2(Vector2(cell) * Grid.CELL_SIZE, Vector2.ONE * Grid.CELL_SIZE).grow(-INSET)


func _ready() -> void:
	Events.simulation_reset.connect(queue_redraw)
	Events.lots_changed.connect(queue_redraw)
	Events.road_network_changed.connect(func(_nodes: Array, _segments: Array) -> void: queue_redraw())


func _draw() -> void:
	var lots := Game.simulation.lots
	var roads := Game.simulation.roads
	for lot: Lot in lots.lots.values():
		var rect := get_lot_rect(lot.cell)
		var color: Color = COLORS[lot.type]
		var access := lots.get_access(lot.id)
		if access.is_empty():
			draw_rect(rect, color.darkened(0.55))
			draw_rect(rect, NO_ACCESS_COLOR, false, 2.0)
			continue
		var door: Vector2 = roads.lanes[access.lane_id].curve.sample_baked(access.distance)
		draw_line(rect.get_center(), door, color.darkened(0.35), 4.0)
		draw_rect(rect, color)
