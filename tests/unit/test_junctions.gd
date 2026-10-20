extends GutTest
## Lane trimming at junctions and the connector lanes that link roads through them.

const G := 64.0  # One grid cell.

var roads: RoadNetwork
var road_type: RoadType


func before_each() -> void:
	roads = RoadNetwork.new()
	road_type = RoadType.load_by_id(&"local_2lane")


func _add(from: Vector2, to: Vector2, type: RoadType = road_type) -> int:
	return roads.add_road(from * G, to * G, type)


func _node(at: Vector2) -> RoadNode:
	return roads.nodes[roads.find_node_at(at * G)]


func _connectors(at: Vector2) -> Array[Lane]:
	var result: Array[Lane] = []
	for lane_id in _node(at).connector_lane_ids:
		result.append(roads.lanes[lane_id])
	return result


func _turns(at: Vector2) -> Array:
	return _connectors(at).map(func(lane: Lane) -> Lane.Turn: return lane.turn)


func _road_lane(segment_id: int, from_at: Vector2) -> Lane:
	var from_node := roads.find_node_at(from_at * G)
	for lane_id in roads.segments[segment_id].lane_ids:
		if roads.lanes[lane_id].from_node_id == from_node:
			return roads.lanes[lane_id]
	return null


func _start(lane: Lane) -> Vector2:
	return lane.curve.get_point_position(0)


func _end(lane: Lane) -> Vector2:
	return lane.curve.get_point_position(lane.curve.point_count - 1)


func test_dead_end_gets_a_u_turn_and_no_trim() -> void:
	var id := _add(Vector2(0, 0), Vector2(2, 0))
	assert_eq(_turns(Vector2(2, 0)), [Lane.Turn.U_TURN])
	var eastbound := _road_lane(id, Vector2(0, 0))
	assert_eq(_end(eastbound), Vector2(2 * G, 7))


func test_u_turn_links_the_two_directions() -> void:
	var id := _add(Vector2(0, 0), Vector2(2, 0))
	var eastbound := _road_lane(id, Vector2(0, 0))
	var westbound := _road_lane(id, Vector2(2, 0))
	var u_turn: Lane = _connectors(Vector2(2, 0))[0]
	assert_eq(eastbound.outgoing_lane_ids, [u_turn.id])
	assert_eq(u_turn.outgoing_lane_ids, [westbound.id])
	assert_almost_eq(_start(u_turn), _end(eastbound), Vector2.ONE * 0.01)
	assert_almost_eq(_end(u_turn), _start(westbound), Vector2.ONE * 0.01)


func test_corner_trims_lanes_by_half_the_road_width() -> void:
	var east := _add(Vector2(0, 0), Vector2(2, 0))
	_add(Vector2(2, 0), Vector2(2, 2))
	var half_width := road_type.get_total_width() * 0.5
	var eastbound := _road_lane(east, Vector2(0, 0))
	var westbound := _road_lane(east, Vector2(2, 0))
	assert_almost_eq(_end(eastbound), Vector2(2 * G - half_width, 7), Vector2.ONE * 0.01)
	assert_almost_eq(_start(westbound), Vector2(2 * G - half_width, -7), Vector2.ONE * 0.01)
	# The far end is still a dead end and stays untrimmed.
	assert_almost_eq(_start(eastbound), Vector2(0, 7), Vector2.ONE * 0.01)


func test_corner_has_one_turn_each_way_and_no_u_turns() -> void:
	_add(Vector2(0, 0), Vector2(2, 0))
	_add(Vector2(2, 0), Vector2(2, 2))
	var turns := _turns(Vector2(2, 0))
	assert_eq(turns.size(), 2)
	assert_has(turns, Lane.Turn.RIGHT)  # Eastbound turning south.
	assert_has(turns, Lane.Turn.LEFT)  # Northbound turning west.


func test_connector_endpoints_meet_the_lanes_they_join() -> void:
	_add(Vector2(0, 0), Vector2(2, 0))
	_add(Vector2(2, 0), Vector2(2, 2))
	for connector in _connectors(Vector2(2, 0)):
		var target := roads.lanes[connector.outgoing_lane_ids[0]]
		assert_almost_eq(_end(connector), _start(target), Vector2.ONE * 0.01)
		var feeders := roads.lanes.values().filter(func(lane: Lane) -> bool: return lane.outgoing_lane_ids.has(connector.id))
		assert_eq(feeders.size(), 1)
		assert_almost_eq(_start(connector), _end(feeders[0]), Vector2.ONE * 0.01)


func test_four_way_junction_has_straight_left_right_from_every_approach() -> void:
	_add(Vector2(0, 2), Vector2(2, 2))
	_add(Vector2(4, 2), Vector2(2, 2))
	_add(Vector2(2, 0), Vector2(2, 2))
	_add(Vector2(2, 4), Vector2(2, 2))
	var turns := _turns(Vector2(2, 2))
	assert_eq(turns.size(), 12)
	assert_eq(turns.count(Lane.Turn.STRAIGHT), 4)
	assert_eq(turns.count(Lane.Turn.RIGHT), 4)
	assert_eq(turns.count(Lane.Turn.LEFT), 4)


func test_straight_continuation_connector_is_straight() -> void:
	_add(Vector2(0, 0), Vector2(2, 0))
	_add(Vector2(2, 0), Vector2(4, 0))
	var straight: Array = _connectors(Vector2(2, 0)).filter(func(lane: Lane) -> bool: return lane.turn == Lane.Turn.STRAIGHT)
	assert_eq(straight.size(), 2)
	for connector: Lane in straight:
		assert_almost_eq(connector.length, road_type.get_total_width(), 0.01)


func test_turn_connectors_are_slower_than_straight() -> void:
	_add(Vector2(0, 0), Vector2(2, 0))
	_add(Vector2(2, 0), Vector2(2, 2))
	for connector in _connectors(Vector2(2, 0)):
		assert_lt(connector.speed_limit, road_type.speed_limit)


func test_removing_a_road_turns_the_junction_back_into_a_dead_end() -> void:
	var east := _add(Vector2(0, 0), Vector2(2, 0))
	var south := _add(Vector2(2, 0), Vector2(2, 2))
	roads.remove_segment(south)
	assert_eq(_turns(Vector2(2, 0)), [Lane.Turn.U_TURN])
	assert_eq(_end(_road_lane(east, Vector2(0, 0))), Vector2(2 * G, 7), "trim removed")
	for lane: Lane in roads.lanes.values():
		for next_id in lane.outgoing_lane_ids:
			assert_true(roads.lanes.has(next_id), "no dangling lane references")


func test_road_lane_ids_survive_junction_changes() -> void:
	var east := _add(Vector2(0, 0), Vector2(2, 0))
	var before := roads.segments[east].lane_ids.duplicate()
	_add(Vector2(2, 0), Vector2(2, 2))
	assert_eq(roads.segments[east].lane_ids, before)


func test_changed_signal_lists_segments_whose_lanes_were_retrimmed() -> void:
	var east := _add(Vector2(0, 0), Vector2(2, 0))
	watch_signals(roads)
	var south := _add(Vector2(2, 0), Vector2(2, 2))
	var params: Array = get_signal_parameters(roads, "changed")
	assert_has(params[1], east)
	assert_has(params[1], south)


func test_multi_lane_roads_turn_from_the_matching_lanes() -> void:
	var avenue := RoadType.new()
	avenue.lanes_per_direction = 2
	avenue.lane_width = 14.0
	_add(Vector2(0, 0), Vector2(3, 0), avenue)
	_add(Vector2(3, 0), Vector2(3, 3), avenue)
	for connector in _connectors(Vector2(3, 0)):
		var source: Lane = roads.lanes.values().filter(func(lane: Lane) -> bool: return lane.outgoing_lane_ids.has(connector.id))[0]
		var target := roads.lanes[connector.outgoing_lane_ids[0]]
		if connector.turn == Lane.Turn.RIGHT:
			assert_eq([source.index_from_center, target.index_from_center], [1, 1], "right turns use curb lanes")
		else:
			assert_eq([source.index_from_center, target.index_from_center], [0, 0], "left turns use center lanes")
	assert_eq(_connectors(Vector2(3, 0)).size(), 2)
