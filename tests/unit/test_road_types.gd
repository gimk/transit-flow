extends GutTest
## Phase 5a: avenues, one-way roads, upgrading roads in place.

const G := 64.0
const Checks := preload("res://tests/traffic_checks.gd")

var sim: Simulation
var roads: RoadNetwork


func before_each() -> void:
	sim = Simulation.new()
	roads = sim.roads


func after_each() -> void:
	sim.dispose()


func _type(id: StringName) -> RoadType:
	return RoadType.load_by_id(id)


func _road(from: Vector2, to: Vector2, type_id := &"local_2lane") -> Array[int]:
	return roads.add_road_pieces(from * G, to * G, _type(type_id))


func _lanes_of(segment_id: int) -> Array[Lane]:
	var result: Array[Lane] = []
	for lane_id in roads.segments[segment_id].lane_ids:
		result.append(roads.lanes[lane_id])
	return result


func test_palette_order_starts_with_the_local_street() -> void:
	var ids := RoadType.list_all().map(func(t: RoadType) -> StringName: return t.id)
	assert_eq(ids, [&"local_2lane", &"avenue_4lane", &"one_way_1lane", &"one_way_2lane", &"one_way_3lane"])


func test_avenue_has_two_lanes_each_way() -> void:
	var id := _road(Vector2(1, 2), Vector2(6, 2), &"avenue_4lane")[0]
	var lanes := _lanes_of(id)
	assert_eq(lanes.size(), 4)
	var eastbound := lanes.filter(func(l: Lane) -> bool: return LaneBuilder.end_direction(l.curve).x > 0.0)
	assert_eq(eastbound.size(), 2)
	assert_eq(_type(&"avenue_4lane").get_total_width(), 56.0)


func test_one_way_lanes_all_run_the_way_it_was_dragged() -> void:
	var east := _road(Vector2(1, 2), Vector2(6, 2), &"one_way_3lane")[0]
	var west := _road(Vector2(6, 5), Vector2(1, 5), &"one_way_3lane")[0]
	for lane in _lanes_of(east):
		assert_gt(LaneBuilder.end_direction(lane.curve).x, 0.9, "eastbound")
	for lane in _lanes_of(west):
		assert_lt(LaneBuilder.end_direction(lane.curve).x, -0.9, "westbound")
	# Centered on the road, index 0 on the left of the direction of travel (north here).
	var ys := _lanes_of(east).map(func(l: Lane) -> float: return l.curve.get_point_position(0).y - 2 * G)
	assert_eq(ys, [-14.0, 0.0, 14.0])


func test_one_way_roads_are_never_driven_the_wrong_way() -> void:
	# A one-way loop of three sides, closed by a two-way street.
	_road(Vector2(1, 1), Vector2(6, 1), &"one_way_1lane")
	_road(Vector2(6, 1), Vector2(6, 5), &"one_way_1lane")
	_road(Vector2(6, 5), Vector2(1, 5), &"one_way_1lane")
	_road(Vector2(1, 5), Vector2(1, 1))
	# From the middle of the top (eastbound) road back to its start: all the way around.
	var from := roads.find_lane_position(Vector2(4, 1) * G, 10.0)
	var to := roads.find_lane_position(Vector2(2, 1) * G, 10.0)
	var route := sim.router.find_route(from.lane_id, from.distance, to.lane_id, to.distance)
	assert_not_null(route)
	var turns := 0
	for lane_id in route.lane_ids:
		if roads.lanes[lane_id].is_connector():
			turns += 1
	assert_eq(turns, 4, "around the block, not backwards")


func test_dragging_another_type_over_a_road_upgrades_it_in_place() -> void:
	_road(Vector2(1, 2), Vector2(6, 2))
	_road(Vector2(3, 0), Vector2(3, 2))  # A side street meeting it.
	var lot := sim.lots.add_lot(Vector2i(2, 2), Lot.Type.RESIDENTIAL)
	var nodes_before := roads.nodes.size()
	var segments_before := roads.segments.size()
	var pieces := _road(Vector2(1, 2), Vector2(6, 2), &"avenue_4lane")
	assert_eq(pieces.size(), 2, "both pieces along it (it was split by the side street)")
	for segment_id in pieces:
		assert_eq(roads.segments[segment_id].road_type.id, &"avenue_4lane")
		assert_eq(_lanes_of(segment_id).size(), 4)
	assert_eq(roads.segments.size(), segments_before, "no new segments: the same ones, retyped")
	assert_eq(roads.nodes.size(), nodes_before, "no new nodes")
	assert_false(sim.lots.get_access(lot).is_empty(), "lots along it keep their access")
	assert_eq(roads.validate_road(Vector2(1, 2) * G, Vector2(6, 2) * G), "Road already exists")
	assert_eq(roads.plan_road(Vector2(1, 2) * G, Vector2(6, 2) * G, 0, _type(&"avenue_4lane")).error, "Road already exists")


func test_upgrading_part_of_a_road_splits_it() -> void:
	_road(Vector2(1, 2), Vector2(9, 2))
	_road(Vector2(5, 2), Vector2(9, 2), &"avenue_4lane")
	assert_eq(roads.segments.size(), 2)
	var types := roads.segments.values().map(func(s: RoadSegment) -> String: return String(s.road_type.id))
	types.sort()
	assert_eq(types, ["avenue_4lane", "local_2lane"])


func test_dragging_a_one_way_road_backwards_reverses_it() -> void:
	var id := _road(Vector2(1, 2), Vector2(6, 2), &"one_way_2lane")[0]
	assert_eq(roads.validate_road(Vector2(1, 2) * G, Vector2(6, 2) * G), "Road already exists")
	assert_eq(roads.plan_road(Vector2(1, 2) * G, Vector2(6, 2) * G, 0, _type(&"one_way_2lane")).error, "Road already exists")
	_road(Vector2(6, 2), Vector2(1, 2), &"one_way_2lane")
	for lane in _lanes_of(id):
		assert_lt(LaneBuilder.end_direction(lane.curve).x, -0.9, "now westbound")
	assert_eq(roads.nodes[roads.segments[id].start_node_id].position, Vector2(6, 2) * G)


func test_types_and_directions_survive_a_save() -> void:
	_road(Vector2(1, 2), Vector2(6, 2), &"avenue_4lane")
	_road(Vector2(6, 5), Vector2(1, 5), &"one_way_2lane")
	var loaded := RoadNetwork.from_dict(JSON.parse_string(JSON.stringify(roads.to_dict())))
	var one_way: RoadSegment = loaded.segments.values().filter(func(s: RoadSegment) -> bool: return s.road_type.one_way)[0]
	assert_eq(loaded.nodes[one_way.start_node_id].position, Vector2(6, 5) * G)
	for lane_id in one_way.lane_ids:
		assert_lt(LaneBuilder.end_direction(loaded.lanes[lane_id].curve).x, -0.9)
	assert_eq(loaded.lanes.size(), roads.lanes.size())
	loaded.dispose()


func test_turns_from_a_three_lane_one_way_use_the_matching_lanes() -> void:
	_road(Vector2(1, 3), Vector2(8, 3), &"one_way_3lane")  # Eastbound.
	_road(Vector2(5, 0), Vector2(5, 6))
	var junction := roads.nodes[roads.find_node_at(Vector2(5, 3) * G)]
	for lane_id in junction.connector_lane_ids:
		var connector := roads.lanes[lane_id]
		var feeder := roads.lanes[connector.feeder_lane_id]
		if not roads.segments[feeder.segment_id].road_type.one_way:
			continue
		match connector.turn:
			Lane.Turn.LEFT:
				assert_eq(feeder.index_from_center, 0, "left turns from the left lane")
			Lane.Turn.RIGHT:
				assert_eq(feeder.index_from_center, 2, "right turns from the curb lane")


func test_vehicles_on_a_road_being_upgraded_carry_on() -> void:
	_road(Vector2(0, 2), Vector2(12, 2))
	var from := roads.find_lane_position((Vector2(1, 2) + Vector2(0, 7) / G) * G, 4.0)
	var to := roads.find_lane_position((Vector2(11, 2) + Vector2(0, 7) / G) * G, 4.0)
	var vehicle := sim.vehicles.spawn(sim.router.find_route(from.lane_id, from.distance, to.lane_id, to.distance), -1, 1)
	for i in 20:
		sim.step()
	_road(Vector2(0, 2), Vector2(12, 2), &"avenue_4lane")
	var arrived := false
	for i in 600:
		sim.step()
		if not sim.vehicles.vehicles.has(vehicle.id):
			arrived = not vehicle.gave_up
			break
	assert_true(arrived, "re-snapped onto the new lanes and arrived")


## Random traffic on grids mixing road types: multi-lane and one-way junctions and lane
## changes keep the safety rules and nothing deadlocks. 200 vehicles: at 250 a grid with
## one-way streets is at the edge of real gridlock (it locks up or not depending on the
## random trips).
func test_stress_grid_with_avenues() -> void:
	_stress_grid(&"avenue_4lane", &"avenue_4lane", &"local_2lane", &"local_2lane", 300)


func test_stress_grid_with_one_way_streets() -> void:
	_stress_grid(&"local_2lane", &"local_2lane", &"one_way_2lane", &"one_way_1lane", 200)


## Before lane changing (5b) this gridlocked: a car in the wrong lane of an avenue had to
## circle the block to reach its turn.
func test_stress_grid_with_avenues_and_one_way_streets() -> void:
	_stress_grid(&"avenue_4lane", &"avenue_4lane", &"one_way_2lane", &"one_way_1lane", 300)


## 200 random trips for 90 s on a 5 x 5 grid, 3 cells apart: even rows/columns of
## `even_*` roads, odd ones of `odd_*` roads (one-way ones alternate direction).
func _stress_grid(even_h: StringName, even_v: StringName, odd_h: StringName, odd_v: StringName, min_arrivals: int) -> void:
	for i in 5:
		var at := 1 + i * 3
		var horizontal := even_h if i % 2 == 0 else odd_h
		var vertical := even_v if i % 2 == 0 else odd_v
		if i % 4 == 1:
			_road(Vector2(13, at), Vector2(1, at), horizontal)  # Westbound.
			_road(Vector2(at, 13), Vector2(at, 1), vertical)    # Northbound.
		else:
			_road(Vector2(1, at), Vector2(13, at), horizontal)
			_road(Vector2(at, 1), Vector2(at, 13), vertical)
	sim.stress.target_count = 200
	var arrived := { "count": 0 }
	sim.vehicles.vehicle_arrived.connect(func(_v: Vehicle) -> void: arrived.count += 1)
	var violations := 0
	var worst_gap := INF
	for tick in Simulation.TICK_RATE * 90:
		sim.step()
		violations += Checks.connector_conflict_violations(sim)
		worst_gap = minf(worst_gap, Checks.min_queue_gap(sim))
	gut.p("grid %s/%s: vehicles %d, arrivals %d, stuck %d, smallest gap %.2f" % [
		even_h, odd_h, sim.vehicles.vehicles.size(), arrived.count, sim.vehicles.get_stuck_count(), worst_gap])
	assert_eq(violations, 0, "never two vehicles where conflicting paths meet")
	assert_gte(worst_gap, VehicleSystem.MIN_GAP - 0.01, "no overlaps, lane changes included")
	assert_gt(arrived.count, min_arrivals, "traffic flows")
	assert_eq(sim.vehicles.get_stuck_count(30.0), 0, "no deadlock")
