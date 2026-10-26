extends GutTest
## The dot display's grid follows the camera's zoom steps: at every zoom level the cell
## is a power of two in world px and dots land 3 or 4 screen px apart
## (DotDisplay.cell_size, MapCamera.ZOOM_LEVELS).


func test_every_zoom_level_gives_3_or_4_px_dots() -> void:
	for zoom in MapCamera.ZOOM_LEVELS:
		var cell := DotDisplay.cell_size(zoom)
		var pitch := cell * zoom
		assert_true(is_equal_approx(pitch, 3.0) or is_equal_approx(pitch, 4.0), "zoom %s: %s px" % [zoom, pitch])
		assert_eq(cell, pow(2.0, roundf(log(cell) / log(2.0))), "zoom %s: a power of two" % zoom)


func test_finest_cell_is_one_world_px() -> void:
	assert_eq(DotDisplay.cell_size(MapCamera.ZOOM_LEVELS[-1]), 1.0)
	assert_eq(DotDisplay.cell_size(MapCamera.ZOOM_LEVELS[0]), 16.0)


func test_nearest_level() -> void:
	assert_eq(MapCamera.ZOOM_LEVELS[MapCamera.nearest_level(1.0)], 1.0)
	assert_eq(MapCamera.ZOOM_LEVELS[MapCamera.nearest_level(1.1)], 1.0)
	assert_eq(MapCamera.ZOOM_LEVELS[MapCamera.nearest_level(10.0)], 4.0)
	assert_eq(MapCamera.ZOOM_LEVELS[MapCamera.nearest_level(0.01)], 0.25)
