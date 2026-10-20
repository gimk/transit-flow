extends GutTest

var road_type: RoadType


func before_each() -> void:
	road_type = RoadType.new()
	road_type.lanes_per_direction = 1
	road_type.lane_width = 14.0


func _points(curve: Curve2D) -> PackedVector2Array:
	return LaneBuilder.get_polyline(curve)


func test_eastbound_road_drives_on_the_right() -> void:
	var specs := LaneBuilder.build_lane_specs(RoadNetwork.straight_curve(Vector2(0, 0), Vector2(128, 0)), road_type)
	assert_eq(specs.size(), 2)
	var forward: Dictionary = specs.filter(func(s: Dictionary) -> bool: return s.forward)[0]
	var backward: Dictionary = specs.filter(func(s: Dictionary) -> bool: return not s.forward)[0]
	# Heading east (+x) in y-down space, the right-hand side is south (+y).
	assert_eq(_points(forward.curve), PackedVector2Array([Vector2(0, 7), Vector2(128, 7)]))
	# The backward lane heads west, so its right-hand side is north (-y).
	assert_eq(_points(backward.curve), PackedVector2Array([Vector2(128, -7), Vector2(0, -7)]))


func test_straight_lane_length_matches_road() -> void:
	var specs := LaneBuilder.build_lane_specs(RoadNetwork.straight_curve(Vector2(0, 0), Vector2(0, 192)), road_type)
	for spec: Dictionary in specs:
		assert_almost_eq(spec.curve.get_baked_length(), 192.0, 0.01)


func test_multiple_lanes_per_direction_stack_outward() -> void:
	road_type.lanes_per_direction = 2
	var specs := LaneBuilder.build_lane_specs(RoadNetwork.straight_curve(Vector2(0, 0), Vector2(128, 0)), road_type)
	assert_eq(specs.size(), 4)
	for spec: Dictionary in specs:
		var expected_offset: float = (spec.index + 0.5) * 14.0
		assert_almost_eq(absf(_points(spec.curve)[0].y), expected_offset, 0.001)


func test_curved_centerline_lanes_stay_at_constant_offset() -> void:
	# Quarter circle-ish arc from (0,0) to (128,128).
	var centerline := Curve2D.new()
	centerline.add_point(Vector2(0, 0), Vector2.ZERO, Vector2(70, 0))
	centerline.add_point(Vector2(128, 128), Vector2(0, -70), Vector2.ZERO)
	var centerline_points := LaneBuilder.get_polyline(centerline)
	var specs := LaneBuilder.build_lane_specs(centerline, road_type)
	for spec: Dictionary in specs:
		for point in _points(spec.curve):
			assert_almost_eq(LaneBuilder.distance_to_polyline(point, centerline_points), 7.0, 0.5)
