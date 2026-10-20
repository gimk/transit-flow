extends GutTest

var roads: RoadNetwork
var road_type: RoadType


func before_each() -> void:
	roads = RoadNetwork.new()
	road_type = RoadType.load_by_id(&"local_2lane")


func _add(from: Vector2, to: Vector2, layer := 0) -> int:
	return roads.add_road(from, to, road_type, layer)


func test_road_type_resource_loads() -> void:
	assert_not_null(road_type)
	assert_eq(road_type.id, &"local_2lane")


func test_add_road_creates_two_nodes_one_segment_two_lanes() -> void:
	var id := _add(Vector2(0, 0), Vector2(128, 0))
	assert_ne(id, -1)
	assert_eq(roads.nodes.size(), 2)
	assert_eq(roads.segments.size(), 1)
	assert_eq(roads.lanes.size(), 2)


func test_lanes_connect_the_segment_nodes_in_both_directions() -> void:
	var segment := roads.segments[_add(Vector2(0, 0), Vector2(128, 0))]
	var directions: Array = []
	for lane_id in segment.lane_ids:
		var lane := roads.lanes[lane_id]
		directions.append([lane.from_node_id, lane.to_node_id])
		assert_eq(lane.speed_limit, road_type.speed_limit)
	assert_has(directions, [segment.start_node_id, segment.end_node_id])
	assert_has(directions, [segment.end_node_id, segment.start_node_id])


func test_roads_sharing_an_endpoint_share_a_node() -> void:
	_add(Vector2(0, 0), Vector2(128, 0))
	_add(Vector2(128, 0), Vector2(128, 128))
	assert_eq(roads.nodes.size(), 3)
	var corner := roads.find_node_at(Vector2(128, 0))
	assert_eq(roads.nodes[corner].get_degree(), 2)


func test_rejects_too_short_road() -> void:
	assert_eq(_add(Vector2(64, 64), Vector2(64, 64)), -1)
	assert_eq(roads.segments.size(), 0)


func test_rejects_duplicate_road_in_either_direction() -> void:
	_add(Vector2(0, 0), Vector2(128, 0))
	assert_eq(_add(Vector2(0, 0), Vector2(128, 0)), -1)
	assert_eq(_add(Vector2(128, 0), Vector2(0, 0)), -1)


func test_rejects_crossing_on_same_layer() -> void:
	_add(Vector2(0, 64), Vector2(128, 64))
	assert_eq(_add(Vector2(64, 0), Vector2(64, 128)), -1)


func test_allows_crossing_on_different_layers() -> void:
	_add(Vector2(0, 64), Vector2(128, 64), 0)
	assert_ne(_add(Vector2(64, 0), Vector2(64, 128), 1), -1)
	assert_eq(roads.nodes.size(), 4)


func test_rejects_ending_in_the_middle_of_a_road() -> void:
	_add(Vector2(0, 64), Vector2(128, 64))
	assert_eq(_add(Vector2(64, 0), Vector2(64, 64)), -1)


func test_rejects_passing_through_an_existing_node() -> void:
	_add(Vector2(64, 0), Vector2(64, 64))
	assert_eq(_add(Vector2(0, 64), Vector2(128, 64)), -1)


func test_rejects_collinear_overlap() -> void:
	_add(Vector2(0, 0), Vector2(128, 0))
	assert_eq(_add(Vector2(64, 0), Vector2(256, 0)), -1)


func test_allows_extending_a_road_in_a_straight_line() -> void:
	_add(Vector2(0, 0), Vector2(128, 0))
	assert_ne(_add(Vector2(128, 0), Vector2(256, 0)), -1)


func test_remove_segment_removes_lanes_and_orphan_nodes_only() -> void:
	var first := _add(Vector2(0, 0), Vector2(128, 0))
	_add(Vector2(128, 0), Vector2(128, 128))
	assert_true(roads.remove_segment(first))
	assert_eq(roads.segments.size(), 1)
	assert_eq(roads.lanes.size(), 2)
	assert_eq(roads.nodes.size(), 2, "the shared corner node is kept")
	assert_eq(roads.find_node_at(Vector2(0, 0)), -1, "the dead-end node is removed")


func test_remove_unknown_segment_returns_false() -> void:
	assert_false(roads.remove_segment(42))


func test_changed_signal_reports_touched_ids() -> void:
	watch_signals(roads)
	var id := _add(Vector2(0, 0), Vector2(128, 0))
	assert_signal_emitted(roads, "changed")
	var params: Array = get_signal_parameters(roads, "changed")
	assert_eq(params[1], [id])
	assert_eq(params[0].size(), 2)
	roads.remove_segment(id)
	assert_signal_emit_count(roads, "changed", 2)


func test_find_segment_at() -> void:
	var id := _add(Vector2(0, 0), Vector2(128, 0))
	assert_eq(roads.find_segment_at(Vector2(60, 10), 18.0), id)
	assert_eq(roads.find_segment_at(Vector2(60, 40), 18.0), -1)


func test_save_round_trip_through_json() -> void:
	_add(Vector2(0, 0), Vector2(128, 0))
	_add(Vector2(128, 0), Vector2(128, 192))
	var removed := _add(Vector2(128, 192), Vector2(320, 192))
	roads.remove_segment(removed)

	var saved := roads.to_dict()
	var parsed: Dictionary = JSON.parse_string(JSON.stringify(saved, "", true, true))
	var loaded := RoadNetwork.from_dict(parsed)

	assert_eq_deep(loaded.to_dict(), saved)
	assert_eq(loaded.lanes.size(), 4)
	assert_eq(loaded.nodes[loaded.find_node_at(Vector2(128, 0))].get_degree(), 2)
	# Ids keep counting from where the saved network left off.
	var next_id := loaded.add_road(Vector2(0, 0), Vector2(0, 128), road_type)
	assert_eq(next_id, removed + 1)
