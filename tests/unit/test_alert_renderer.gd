extends GutTest
## Stuck vehicles are marked one ring per group (AlertRenderer._stuck_clusters), and the
## dot display's level of detail follows the zoom steps (DotDisplay.band_for).


func test_stuck_vehicles_close_together_make_one_ring() -> void:
	var spots := PackedVector2Array([Vector2(10, 10), Vector2(20, 12), Vector2(14, 30)])
	var clusters := AlertRenderer._stuck_clusters(spots, 50.0)
	assert_eq(clusters.size(), 1)
	assert_almost_eq(clusters[0], Vector2(44, 52) / 3.0, Vector2.ONE * 0.01, "at their middle")


func test_stuck_vehicles_far_apart_make_a_ring_each() -> void:
	var spots := PackedVector2Array([Vector2(10, 10), Vector2(500, 10), Vector2(10, 500)])
	assert_eq(AlertRenderer._stuck_clusters(spots, 50.0).size(), 3)


func test_bands_follow_zoom_steps() -> void:
	var expected := {
		4.0: DotDisplay.Band.DETAIL, 3.0: DotDisplay.Band.DETAIL, 2.0: DotDisplay.Band.DETAIL,
		1.5: DotDisplay.Band.STREET, 1.0: DotDisplay.Band.STREET,
		0.75: DotDisplay.Band.DISTRICT, 0.5: DotDisplay.Band.DISTRICT,
		0.375: DotDisplay.Band.CITY, 0.25: DotDisplay.Band.CITY,
	}
	for zoom: float in MapCamera.ZOOM_LEVELS:
		assert_eq(DotDisplay.band_for(zoom), expected[zoom], "zoom %s" % zoom)
