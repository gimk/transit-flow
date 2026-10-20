class_name Lot
extends RefCounted
## One grid cell of zoned land. Placed one by one for now; Phase 3 replaces this with
## painted zones that spawn lots.

enum Type { RESIDENTIAL, COMMERCIAL, INDUSTRIAL }

var id: int
var cell: Vector2i
var type: Type


func _init(p_id: int, p_cell: Vector2i, p_type: Type) -> void:
	id = p_id
	cell = p_cell
	type = p_type


func get_center() -> Vector2:
	return Grid.cell_center(cell)
