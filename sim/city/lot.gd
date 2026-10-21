class_name Lot
extends RefCounted
## One grid cell of built-up land: a building and its parking, on the part of the cell
## the roads around it leave free (`plot`, see LotSystem.compute_plot).

enum Type { RESIDENTIAL, COMMERCIAL, INDUSTRIAL }

var id: int
var cell: Vector2i
var type: Type
## The land the lot stands on (world px): its cell, less the roads and sidewalks.
var plot := Rect2()


func _init(p_id: int, p_cell: Vector2i, p_type: Type) -> void:
	id = p_id
	cell = p_cell
	type = p_type


func get_center() -> Vector2:
	return Grid.cell_center(cell)
