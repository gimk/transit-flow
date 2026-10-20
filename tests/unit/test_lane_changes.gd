extends GutTest
## Phase 5b: lane choice and lane changing on multi-lane roads.

const G := 64.0
const DT := Simulation.TICK_DT
const Checks := preload("res://tests/traffic_checks.gd")

var sim: Simulation
var roads: RoadNetwork


func before_each() -> void:
	sim = Simulation.new()
	roads = sim.roads


func after_each() -> void:
	sim.dispose()


func _road(from: Vector2, to: Vector2, type_id := &"avenue_4lane") -> void:
	roads.add_road(from * G, to * G, RoadType.load_by_id(type_id))


## The eastbound lane of the avenue along y = 4 with the given index (0 = next to the center).
func _eastbound(index: int, x: float) -> Lane:
	for lane: Lane in roads.lanes.values():
		if not lane.is_connector() and lane.index_from_center == index and LaneBuilder.end_direction(lane.curve).x > 0.9:
			var points := LaneBuilder.get_polyline(lane.curve)
			if points[0].x <= x * G and points[-1].x >= x * G and absf(points[0].y - 4 * G) < 30.0:
				return lane
	return null


func _southbound_end() -> Dictionary:
	return roads.find_lane_position((Vector2(8, 8) + Vector2(-7, 0) / G) * G, 4.0)


## A car in the inner (left) lane of an avenue that must turn right at the next junction.
func _car_needing_to_change(from_x := 2.0) -> Vehicle:
	var lane := _eastbound(0, from_x)
	var end := _southbound_end()
	var route := sim.router.find_route(lane.id, from_x * G - LaneBuilder.get_polyline(lane.curve)[0].x, end.lane_id, end.distance)
	assert_not_null(route, "test setup: route exists")
	return sim.vehicles.spawn(route, -1, 1)


func _build_avenue_with_side_street() -> void:
	_road(Vector2(0, 4), Vector2(12, 4))
	_road(Vector2(8, 4), Vector2(8, 9), &"local_2lane")


func test_route_changes_to_the_lane_its_turn_needs() -> void:
	_build_avenue_with_side_street()
	var vehicle := _car_needing_to_change()
	var lanes := vehicle.next_lanes
	assert_true(lanes[0].is_beside(vehicle.lane), "first, over to the curb lane")
	assert_eq(lanes[0].index_from_center, 1)
	assert_eq(lanes[1].turn, Lane.Turn.RIGHT, "then the right turn")


func test_a_lane_change_costs_a_little_not_a_whole_lane() -> void:
	_build_avenue_with_side_street()
	var inner := _eastbound(0, 2.0)
	var curb := _eastbound(1, 2.0)
	var straight := sim.router.find_route(curb.id, 10.0, curb.id, curb.length - 10.0)
	var changing := sim.router.find_route(inner.id, 10.0, curb.id, curb.length - 10.0)
	assert_eq(changing.lane_ids.size(), 2)
	assert_almost_eq(changing.travel_time - straight.travel_time, LaneGraph.LANE_CHANGE_COST, 0.01)
	var free_flow_gap := Route.new(changing.lane_ids, 10.0, curb.length - 10.0, 0.0).get_free_flow_time(roads) \
		- Route.new(straight.lane_ids, 10.0, curb.length - 10.0, 0.0).get_free_flow_time(roads)
	assert_almost_eq(free_flow_gap, 0.0, 0.01, "the distance driven is the same")


func test_car_changes_lanes_then_turns_right() -> void:
	_build_avenue_with_side_street()
	var vehicle := _car_needing_to_change()
	var start_lane := vehicle.lane
	var start_distance := vehicle.distance
	var changed_at := -1.0
	var turned := false
	for i in 400:
		sim.vehicles.step(DT)
		if not sim.vehicles.vehicles.has(vehicle.id):
			break
		if changed_at < 0.0 and vehicle.lane != start_lane:
			changed_at = vehicle.distance
			assert_eq(vehicle.lane.index_from_center, 1, "now in the curb lane")
			assert_ne(vehicle.lateral_offset, 0.0, "drawn sliding across")
		turned = turned or vehicle.lane.turn == Lane.Turn.RIGHT
	assert_gte(changed_at, 0.0, "changed lanes")
	assert_lt(changed_at - start_distance, 10.0, "right away, on an empty road")
	assert_true(turned)
	assert_false(sim.vehicles.vehicles.has(vehicle.id), "arrived")


func test_merging_car_finds_a_gap_in_a_stream() -> void:
	_build_avenue_with_side_street()
	# A steady stream along the curb lane, a car starting late in the inner lane that
	# must get over to turn right.
	var curb := _eastbound(1, 1.0)
	var end := roads.find_lane_position((Vector2(11.5, 4) + Vector2(0, 21) / G) * G, 4.0)
	var vehicle: Vehicle = null
	var worst_gap := INF
	for i in 600:
		if i % 20 == 0 and sim.vehicles.can_spawn_at(curb.id, 5.0):
			var route := sim.router.find_route(curb.id, 5.0, end.lane_id, end.distance)
			if route != null:
				sim.vehicles.spawn(route, -1, 2)
		if i == 60:
			vehicle = _car_needing_to_change(6.0)
		sim.vehicles.step(DT)
		worst_gap = minf(worst_gap, Checks.min_queue_gap(sim))
	assert_not_null(vehicle)
	assert_false(sim.vehicles.vehicles.has(vehicle.id), "got in, turned and arrived")
	assert_false(vehicle.gave_up)
	assert_gte(worst_gap, VehicleSystem.MIN_GAP - 0.01, "never too close, merging included")


func test_lane_changes_and_offsets_survive_rerouting() -> void:
	_build_avenue_with_side_street()
	var vehicle := _car_needing_to_change()
	sim.vehicles.reroute_delayed_vehicle(vehicle)
	assert_true(vehicle.next_lanes[0].is_beside(vehicle.lane), "still plans the lane change")
	for i in 400:
		sim.vehicles.step(DT)
	assert_false(sim.vehicles.vehicles.has(vehicle.id))
