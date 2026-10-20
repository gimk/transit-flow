extends GutTest

const G := 64.0  # One grid cell.

var roads: RoadNetwork
var router: Router
var local: RoadType


func before_each() -> void:
	roads = RoadNetwork.new()
	router = Router.new(roads)
	local = RoadType.load_by_id(&"local_2lane")


func after_each() -> void:
	roads.dispose()


func _add(from: Vector2, to: Vector2, type: RoadType = local) -> int:
	return roads.add_road(from * G, to * G, type)


## The lane of `segment_id` that drives away from the node at `from_at`.
func _lane(segment_id: int, from_at: Vector2) -> Lane:
	var from_node := roads.find_node_at(from_at * G)
	for lane_id in roads.segments[segment_id].lane_ids:
		if roads.lanes[lane_id].from_node_id == from_node:
			return roads.lanes[lane_id]
	return null


func _assert_connected(route: Route) -> void:
	for i in route.lane_ids.size() - 1:
		assert_has(roads.lanes[route.lane_ids[i]].outgoing_lane_ids, int(route.lane_ids[i + 1]),
			"each lane must lead into the next")


## The road segments a route drives on, in order, ignoring connectors.
func _segments_of(route: Route) -> Array:
	var result := []
	for lane_id in route.lane_ids:
		var lane := roads.lanes[lane_id]
		if not lane.is_connector() and (result.is_empty() or result[-1] != lane.segment_id):
			result.append(lane.segment_id)
	return result


## Approach road into (0,0), then either a direct 4-cell road to (4,0) or a 10-cell
## detour south via y=3, then an exit road. Returns the key segment ids.
func _build_detour_map(direct_type: RoadType = local, detour_type: RoadType = local) -> Dictionary:
	var ids := {}
	ids.start = _add(Vector2(-2, 0), Vector2(0, 0))
	ids.direct = _add(Vector2(0, 0), Vector2(4, 0), direct_type)
	_add(Vector2(0, 0), Vector2(0, 3), detour_type)
	_add(Vector2(0, 3), Vector2(4, 3), detour_type)
	_add(Vector2(4, 3), Vector2(4, 0), detour_type)
	ids.dest = _add(Vector2(4, 0), Vector2(6, 0))
	return ids


func test_same_lane_ahead_is_a_single_lane_route() -> void:
	var id := _add(Vector2(0, 0), Vector2(4, 0))
	var lane := _lane(id, Vector2(0, 0))
	var route := router.find_route(lane.id, 10.0, lane.id, 100.0)
	assert_eq(Array(route.lane_ids), [lane.id])
	assert_almost_eq(route.travel_time, 90.0 / lane.speed_limit, 0.001)


func test_around_a_corner() -> void:
	var a := _add(Vector2(0, 0), Vector2(3, 0))
	var b := _add(Vector2(3, 0), Vector2(3, 3))
	var route := router.find_route(_lane(a, Vector2(0, 0)).id, 0.0, _lane(b, Vector2(3, 0)).id, 50.0)
	assert_not_null(route)
	assert_eq(route.lane_ids.size(), 3, "lane, right-turn connector, lane")
	assert_eq(roads.lanes[route.lane_ids[1]].turn, Lane.Turn.RIGHT)
	_assert_connected(route)


func test_destination_behind_on_same_lane_loops_via_dead_end() -> void:
	var id := _add(Vector2(0, 0), Vector2(4, 0))
	var eastbound := _lane(id, Vector2(0, 0))
	var route := router.find_route(eastbound.id, 100.0, eastbound.id, 20.0)
	assert_not_null(route)
	_assert_connected(route)
	assert_eq(route.lane_ids[0], eastbound.id)
	assert_eq(route.lane_ids[-1], eastbound.id)
	var turns := Array(route.lane_ids).map(func(lane_id: int) -> Lane.Turn: return roads.lanes[lane_id].turn)
	assert_eq(turns.count(Lane.Turn.U_TURN), 2, "turn around at each end")


func test_opposite_direction_uses_a_u_turn() -> void:
	var id := _add(Vector2(0, 0), Vector2(4, 0))
	var route := router.find_route(_lane(id, Vector2(0, 0)).id, 10.0, _lane(id, Vector2(4, 0)).id, 10.0)
	assert_not_null(route)
	assert_eq(route.lane_ids.size(), 3)
	assert_eq(roads.lanes[route.lane_ids[1]].turn, Lane.Turn.U_TURN)


func test_unreachable_returns_null() -> void:
	var a := _add(Vector2(0, 0), Vector2(2, 0))
	var b := _add(Vector2(5, 5), Vector2(7, 5))
	assert_null(router.find_route(_lane(a, Vector2(0, 0)).id, 0.0, _lane(b, Vector2(5, 5)).id, 10.0))


func test_unknown_lane_returns_null() -> void:
	assert_null(router.find_route(123, 0.0, 456, 0.0))


func test_prefers_shorter_route_when_speeds_are_equal() -> void:
	var start := _build_detour_map()
	var route := router.find_route(_lane(start.start, Vector2(-2, 0)).id, 0.0, _lane(start.dest, Vector2(4, 0)).id, 20.0)
	assert_eq(_segments_of(route), [start.start, start.direct, start.dest])
	_assert_connected(route)


func test_prefers_longer_but_faster_route() -> void:
	var slow := RoadType.new()
	slow.speed_limit = 20.0  # Direct: 256 px at 20 = 12.8 s.
	var fast := RoadType.new()
	fast.speed_limit = 200.0  # Detour: 640 px at 200 = 3.2 s, plus turns.
	var map := _build_detour_map(slow, fast)
	var route := router.find_route(_lane(map.start, Vector2(-2, 0)).id, 0.0, _lane(map.dest, Vector2(4, 0)).id, 20.0)
	assert_not_null(route)
	_assert_connected(route)
	assert_does_not_have(_segments_of(route), map.direct)
	assert_eq(_segments_of(route).size(), 5)


func test_route_updates_after_network_change() -> void:
	var map := _build_detour_map()
	var from_lane := _lane(map.start, Vector2(-2, 0)).id
	var to_lane := _lane(map.dest, Vector2(4, 0)).id
	assert_has(_segments_of(router.find_route(from_lane, 0.0, to_lane, 20.0)), map.direct)
	roads.remove_segment(map.direct)
	var route := router.find_route(from_lane, 0.0, to_lane, 20.0)
	assert_not_null(route, "detour still exists")
	assert_eq(_segments_of(route).size(), 5)
	_assert_connected(route)


func test_travel_time_accounts_for_partial_first_and_last_lanes() -> void:
	var a := _add(Vector2(0, 0), Vector2(3, 0))
	var b := _add(Vector2(3, 0), Vector2(3, 3))
	var first := _lane(a, Vector2(0, 0))
	var last := _lane(b, Vector2(3, 0))
	var route := router.find_route(first.id, 30.0, last.id, 40.0)
	var expected := (first.length - 30.0) / first.speed_limit \
		+ roads.lanes[route.lane_ids[1]].get_travel_time() \
		+ 40.0 / last.speed_limit
	assert_almost_eq(route.travel_time, expected, 0.001)


func test_congestion_makes_a_lane_less_attractive() -> void:
	var map := _build_detour_map()
	var from_lane := _lane(map.start, Vector2(-2, 0)).id
	var to_lane := _lane(map.dest, Vector2(4, 0)).id
	router.find_route(from_lane, 0.0, to_lane, 20.0)  # Builds the graph.
	router.graph.congestion[_lane(map.direct, Vector2(0, 0)).id] = 50.0
	router.invalidate_cache()
	assert_does_not_have(_segments_of(router.find_route(from_lane, 0.0, to_lane, 20.0)), map.direct)


func test_route_polyline_starts_and_ends_at_the_requested_points() -> void:
	var a := _add(Vector2(0, 0), Vector2(3, 0))
	var b := _add(Vector2(3, 0), Vector2(3, 3))
	var first := _lane(a, Vector2(0, 0))
	var last := _lane(b, Vector2(3, 0))
	var route := router.find_route(first.id, 30.0, last.id, 40.0)
	var points := route.get_polyline(roads)
	assert_almost_eq(points[0], first.curve.sample_baked(30.0), Vector2.ONE * 0.5)
	assert_almost_eq(points[-1], last.curve.sample_baked(40.0), Vector2.ONE * 0.5)
