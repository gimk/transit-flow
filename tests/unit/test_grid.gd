extends GutTest


func test_snap_to_point_rounds_to_nearest_grid_intersection() -> void:
	assert_eq(Grid.snap_to_point(Vector2(95, 33)), Vector2(64, 64))
	assert_eq(Grid.snap_to_point(Vector2(31, 97)), Vector2(0, 128))


func test_world_to_cell_floors_negative_coordinates() -> void:
	assert_eq(Grid.world_to_cell(Vector2(-1, 0)), Vector2i(-1, 0))
	assert_eq(Grid.world_to_cell(Vector2(64, 127)), Vector2i(1, 1))


func test_cell_center() -> void:
	assert_eq(Grid.cell_center(Vector2i(1, 2)), Vector2(96, 160))


func test_map_bounds() -> void:
	var size := Grid.MAP_SIZE_CELLS * Grid.CELL_SIZE
	assert_true(Grid.is_inside_map(Vector2.ZERO))
	assert_true(Grid.is_inside_map(Vector2(size)))
	assert_false(Grid.is_inside_map(Vector2(-1, 10)))
