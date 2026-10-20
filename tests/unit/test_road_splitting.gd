extends GutTest
## Phase 2a: roads that cross, touch or overlap existing roads are split into
## segments joined by junction nodes.

const G := 64.0

var roads: RoadNetwork
var road_type: RoadType


func before_each() -> void:
	roads = RoadNetwork.new()
	road_type = RoadType.load_by_id(&"local_2lane")


func after_each() -> void:
	roads.dispose()


func _add(from: Vector2, to: Vector2, layer := 0) -> Array[int]:
	return roads.add_road_pieces(from * G, to * G, road_type, layer)


func _node_at(at: Vector2, layer := 0) -> RoadNode:
	var id := roads.find_node_at(at * G, layer)
	return roads.nodes[id] if id != -1 else null


func _degree(at: Vector2) -> int:
	var node := _node_at(at)
	return node.get_degree() if node != null else 0


## Every invariant the rest of the game relies on.
func _assert_graph_valid() -> void:
	for segment: RoadSegment in roads.segments.values():
		var start := roads.nodes[segment.start_node_id]
		var end := roads.nodes[segment.end_node_id]
		assert_has(start.segment_ids, segment.id)
		assert_has(end.segment_ids, segment.id)
		var points := LaneBuilder.get_polyline(segment.centerline)
		assert_almost_eq(points[0], start.position, Vector2.ONE * 0.01)
		assert_almost_eq(points[-1], end.position, Vector2.ONE * 0.01)
		assert_gte(segment.get_length(), RoadNetwork.MIN_SEGMENT_LENGTH)
		for lane_id in segment.lane_ids:
			assert_true(roads.lanes.has(lane_id))
		# Segments on one layer only meet at their ends.
		for other: RoadSegment in roads.segments.values():
			if other.id != segment.id and other.elevation_layer == segment.elevation_layer:
				var mid := (points[0] + points[-1]) * 0.5
				assert_gt(LaneBuilder.distance_to_polyline(mid, LaneBuilder.get_polyline(other.centerline)), RoadNetwork.EPSILON,
					"segments %d and %d overlap" % [segment.id, other.id])
	for node: RoadNode in roads.nodes.values():
		assert_gt(node.get_degree(), 0, "no orphan nodes")
		for lane_id in node.connector_lane_ids:
			assert_true(roads.lanes.has(lane_id))
	for lane: Lane in roads.lanes.values():
		for next_id in lane.outgoing_lane_ids:
			assert_true(roads.lanes.has(next_id), "no dangling lane links")


func test_crossing_creates_a_four_way_junction() -> void:
	_add(Vector2(0, 2), Vector2(4, 2))
	var pieces := _add(Vector2(2, 0), Vector2(2, 4))
	assert_eq(pieces.size(), 2)
	assert_eq(roads.segments.size(), 4)
	assert_eq(_degree(Vector2(2, 2)), 4)
	assert_eq(_node_at(Vector2(2, 2)).connector_lane_ids.size(), 12)
	_assert_graph_valid()


func test_ending_on_a_road_creates_a_t_junction() -> void:
	_add(Vector2(0, 2), Vector2(4, 2))
	assert_eq(_add(Vector2(2, 0), Vector2(2, 2)).size(), 1)
	assert_eq(roads.segments.size(), 3)
	assert_eq(_degree(Vector2(2, 2)), 3)
	_assert_graph_valid()


func test_starting_on_a_road_creates_a_t_junction() -> void:
	_add(Vector2(0, 2), Vector2(4, 2))
	_add(Vector2(2, 2), Vector2(2, 5))
	assert_eq(_degree(Vector2(2, 2)), 3)
	_assert_graph_valid()


func test_passing_through_an_existing_node_splits_the_new_road() -> void:
	_add(Vector2(2, 0), Vector2(2, 2))
	var pieces := _add(Vector2(0, 2), Vector2(4, 2))
	assert_eq(pieces.size(), 2)
	assert_eq(_degree(Vector2(2, 2)), 3)
	_assert_graph_valid()


func test_crossing_several_roads_at_once() -> void:
	for y in [1, 3, 5]:
		_add(Vector2(0, y), Vector2(4, y))
	var pieces := _add(Vector2(2, 0), Vector2(2, 6))
	assert_eq(pieces.size(), 4)
	assert_eq(roads.segments.size(), 4 + 6)
	for y in [1, 3, 5]:
		assert_eq(_degree(Vector2(2, y)), 4)
	_assert_graph_valid()


func test_new_pieces_are_returned_in_order_from_start_to_end() -> void:
	_add(Vector2(0, 2), Vector2(4, 2))
	var pieces := _add(Vector2(2, 4), Vector2(2, 0))
	assert_eq(roads.nodes[roads.segments[pieces[0]].start_node_id].position, Vector2(2, 4) * G)
	assert_eq(roads.nodes[roads.segments[pieces[1]].end_node_id].position, Vector2(2, 0) * G)


func test_extending_along_an_existing_road_only_builds_the_new_part() -> void:
	_add(Vector2(0, 2), Vector2(4, 2))
	var pieces := _add(Vector2(2, 2), Vector2(6, 2))
	assert_eq(pieces.size(), 1, "only 4..6 is new")
	assert_eq(roads.segments.size(), 3, "0..2, 2..4, 4..6")
	assert_eq(_degree(Vector2(2, 2)), 2)
	assert_eq(_degree(Vector2(4, 2)), 2)
	_assert_graph_valid()


func test_road_entirely_on_an_existing_road_is_rejected() -> void:
	_add(Vector2(0, 2), Vector2(6, 2))
	assert_eq(roads.validate_road(Vector2(1, 2) * G, Vector2(4, 2) * G), "Road already exists")
	assert_eq(_add(Vector2(1, 2), Vector2(4, 2)).size(), 0)
	assert_eq(roads.segments.size(), 1, "nothing was split by the rejected road")


func test_rejects_pieces_shorter_than_the_minimum() -> void:
	roads.add_road(Vector2(100, 0), Vector2(100, 200), road_type)
	assert_eq(roads.validate_road(Vector2(90, 100), Vector2(300, 100)), "Too close to an existing junction")
	assert_eq(roads.add_road(Vector2(90, 100), Vector2(300, 100), road_type), -1)
	assert_eq(roads.segments.size(), 1)


func test_different_layers_do_not_split() -> void:
	_add(Vector2(0, 2), Vector2(4, 2), 0)
	_add(Vector2(2, 0), Vector2(2, 4), 1)
	assert_eq(roads.segments.size(), 2)
	assert_null(_node_at(Vector2(2, 2), 0))
	assert_null(_node_at(Vector2(2, 2), 1))


func test_planning_changes_nothing() -> void:
	_add(Vector2(0, 2), Vector2(4, 2))
	var before := roads.to_dict()
	watch_signals(roads)
	var plan := roads.plan_road(Vector2(2, 0) * G, Vector2(2, 4) * G)
	assert_eq(plan.error, "")
	assert_eq(plan.points, [Vector2(2, 0) * G, Vector2(2, 2) * G, Vector2(2, 4) * G])
	assert_eq(plan.overlaps, [false, false])
	assert_eq_deep(roads.to_dict(), before)
	assert_signal_not_emitted(roads, "changed")


func test_changed_signal_reports_split_and_new_segments() -> void:
	var original: int = _add(Vector2(0, 2), Vector2(4, 2))[0]
	watch_signals(roads)
	var pieces := _add(Vector2(2, 0), Vector2(2, 4))
	var params: Array = get_signal_parameters(roads, "changed")
	assert_has(params[1], original, "the removed segment, so views drop it")
	for id in roads.segments:
		assert_has(params[1], id, "every segment now at the junction")
	assert_false(roads.segments.has(original))
	assert_eq(pieces.size(), 2)


func test_split_keeps_direction_and_type() -> void:
	_add(Vector2(0, 2), Vector2(4, 2))
	_add(Vector2(2, 0), Vector2(2, 4))
	var west_half := roads.get_segment_between(roads.find_node_at(Vector2(0, 2) * G), roads.find_node_at(Vector2(2, 2) * G))
	assert_eq(roads.nodes[roads.segments[west_half].start_node_id].position, Vector2(0, 2) * G)
	assert_eq(roads.segments[west_half].road_type, road_type)


func test_routing_goes_straight_through_a_created_crossing() -> void:
	var router := Router.new(roads)
	_add(Vector2(0, 2), Vector2(4, 2))
	_add(Vector2(2, 0), Vector2(2, 4))
	var west := roads.find_lane_position(Vector2(0.5, 2) * G + Vector2(0, 7), 4.0)
	var east := roads.find_lane_position(Vector2(3.5, 2) * G + Vector2(0, 7), 4.0)
	var route := router.find_route(west.lane_id, west.distance, east.lane_id, east.distance)
	assert_not_null(route)
	assert_eq(route.lane_ids.size(), 3)
	assert_eq(roads.lanes[route.lane_ids[1]].turn, Lane.Turn.STRAIGHT)


func test_random_grid_roads_keep_the_graph_valid() -> void:
	var rng := RandomNumberGenerator.new()
	rng.seed = 12345
	for i in 60:
		var a := Vector2(rng.randi_range(0, 8), rng.randi_range(0, 8))
		var b := a
		if rng.randi() % 2 == 0:
			b.x = rng.randi_range(0, 8)
		else:
			b.y = rng.randi_range(0, 8)
		_add(a, b)
		if rng.randi() % 5 == 0 and not roads.segments.is_empty():
			roads.remove_segment(roads.segments.keys()[rng.randi() % roads.segments.size()])
	assert_gt(roads.segments.size(), 10, "the fuzz actually built something")
	_assert_graph_valid()
	var reloaded := RoadNetwork.from_dict(JSON.parse_string(JSON.stringify(roads.to_dict(), "", true, true)))
	assert_eq(reloaded.segments.size(), roads.segments.size())
	assert_eq(reloaded.lanes.size(), roads.lanes.size())
	reloaded.dispose()
