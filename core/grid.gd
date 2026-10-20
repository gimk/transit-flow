class_name Grid
extends RefCounted
## Grid constants and snapping helpers.
## The grid is a building aid only: the simulation itself works in world pixels.

const CELL_SIZE := 64
const MAP_SIZE_CELLS := Vector2i(128, 128)


static func map_rect() -> Rect2:
	return Rect2(Vector2.ZERO, Vector2(MAP_SIZE_CELLS * CELL_SIZE))


## Cell containing a world position (floor division, so negatives behave).
static func world_to_cell(world_pos: Vector2) -> Vector2i:
	return Vector2i((world_pos / CELL_SIZE).floor())


static func cell_center(cell: Vector2i) -> Vector2:
	return (Vector2(cell) + Vector2(0.5, 0.5)) * CELL_SIZE


## Nearest grid line intersection, where road nodes snap to.
static func snap_to_point(world_pos: Vector2) -> Vector2:
	return (world_pos / CELL_SIZE).round() * CELL_SIZE


static func is_inside_map(world_pos: Vector2) -> bool:
	var rect := map_rect()
	return world_pos.x >= rect.position.x and world_pos.y >= rect.position.y \
		and world_pos.x <= rect.end.x and world_pos.y <= rect.end.y
