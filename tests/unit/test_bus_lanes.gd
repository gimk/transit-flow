extends GutTest
## Phase 8c: bus lanes (the curb lane each way of an "Avenue with Bus Lanes").

const G := 64.0
const Checks := preload("res://tests/traffic_checks.gd")

var sim: Simulation
var roads: RoadNetwork


func before_each() -> void:
	sim = Simulation.new()
	roads = sim.roads


func after_each() -> void:
	sim.dispose()


func _road(from: Vector2, to: Vector2, type_id := &"local_2lane") -> void:
	roads.add_road(from * G, to * G, RoadType.load_by_id(type_id))


func _lane_at(point: Vector2) -> Lane:
	return roads.lanes[roads.find_lane_position(point, 4.0).lane_id]


func test_the_curb_lanes_are_bus_lanes() -> void:
	_road(Vector2(0, 4), Vector2(12, 4), &"avenue_4lane_bus")
	var segment := roads.segments.values()[0] as RoadSegment
	var bus_lanes := 0
	for lane_id in segment.lane_ids:
		var lane := roads.lanes[lane_id]
		assert_eq(lane.bus_only, lane.index_from_center == 1)
		bus_lanes += 1 if lane.bus_only else 0
	assert_eq(bus_lanes, 2, "one each way")


## Crossing a local street: cars turn right from the lane next to the bus lane; the bus
## lane goes straight on (into the next bus lane) or turns right, for buses only.
func test_turns_at_a_junction_keep_cars_out_of_bus_lanes() -> void:
	_road(Vector2(0, 4), Vector2(12, 4), &"avenue_4lane_bus")
	_road(Vector2(6, 0), Vector2(6, 8))
	var node := roads.nodes[roads.find_node_at(Vector2(6, 4) * G)]
	var eastbound_general := _lane_at(Vector2(3, 4) * G + Vector2(0, 7))
	var eastbound_bus := _lane_at(Vector2(3, 4) * G + Vector2(0, 21))
	assert_false(eastbound_general.bus_only)
	assert_true(eastbound_bus.bus_only)
	var general_turns := {}
	for connector: Lane in eastbound_general.outgoing_lanes:
		general_turns[connector.turn] = connector.bus_only
	assert_eq(general_turns, { Lane.Turn.STRAIGHT: false, Lane.Turn.RIGHT: false, Lane.Turn.LEFT: false },
		"the inner lane does everything (one general lane each way)")
	for connector: Lane in eastbound_bus.outgoing_lanes:
		assert_true(connector.bus_only)
		assert_true(connector.turn == Lane.Turn.STRAIGHT or connector.turn == Lane.Turn.RIGHT)
		if connector.turn == Lane.Turn.STRAIGHT:
			assert_true(connector.outgoing_lanes[0].bus_only, "straight on into the next bus lane")
	for lane_id in node.connector_lane_ids:
		var connector := roads.lanes[lane_id]
		assert_eq(connector.bus_only, roads.lanes[connector.feeder_lane_id].bus_only or connector.outgoing_lanes[0].bus_only)


func test_cars_route_beside_the_bus_lane_and_buses_along_it() -> void:
	_road(Vector2(0, 4), Vector2(20, 4), &"avenue_4lane_bus")
	_road(Vector2(10, 0), Vector2(10, 8))
	var from := _lane_at(Vector2(2, 4) * G + Vector2(0, 21))
	var to := _lane_at(Vector2(18, 4) * G + Vector2(0, 21))
	var car := sim.router.find_route(from.id, 20.0, to.id, 60.0)
	var bus := sim.router.find_route(from.id, 20.0, to.id, 60.0, true)
	assert_not_null(car, "a car may start and end at the curb...")
	for i in car.lane_ids.size():
		var lane := roads.lanes[car.lane_ids[i]]
		if lane.bus_only:
			assert_true(lane.is_connector() == false and (i == 0 or i == car.lane_ids.size() - 1),
				"...but only crosses the bus lane there")
	for lane_id in bus.lane_ids:
		assert_true(roads.lanes[lane_id].bus_only, "a bus stays in the bus lane")


## Cars and buses together on a bus-lane avenue grid: no car ever takes a turn into or
## out of a bus lane, and the safety rules hold.
func test_cars_never_turn_through_bus_lanes() -> void:
	for y in [2, 6]:
		_road(Vector2(0, y), Vector2(16, y), &"avenue_4lane_bus")
	for x in [2, 8, 14]:
		_road(Vector2(x, 0), Vector2(x, 8))
	var stops: Array[int] = []
	for x in [5, 11]:
		stops.append(sim.transit.add_stop(Vector2(x, 2) * G + Vector2(0, 25)))
	for x in [11, 5]:
		stops.append(sim.transit.add_stop(Vector2(x, 2) * G + Vector2(0, -25)))
	var line_id := sim.transit.add_line(stops)
	assert_ne(line_id, -1)
	sim.stress.target_count = 60
	var violations := 0
	var worst_gap := INF
	var bus_on_bus_lane := 0
	var bus_samples := 0
	for tick in Simulation.TICK_RATE * 40:
		sim.step()
		worst_gap = minf(worst_gap, Checks.min_queue_gap(sim))
		violations += Checks.connector_conflict_violations(sim)
		for vehicle: Vehicle in sim.vehicles.vehicles.values():
			var on_avenue := not vehicle.lane.is_connector() and roads.segments[vehicle.lane.segment_id].road_type.bus_lanes
			if vehicle.kind == Vehicle.Kind.BUS:
				if on_avenue:
					bus_samples += 1
					bus_on_bus_lane += 1 if vehicle.lane.bus_only else 0
			else:
				assert_false(vehicle.lane.is_connector() and vehicle.lane.bus_only, "car on a bus-only turn")
	gut.p("on the avenue, buses in the bus lane %d%% of the time" % roundi(100.0 * bus_on_bus_lane / maxi(bus_samples, 1)))
	assert_eq(violations, 0)
	assert_gte(worst_gap, VehicleSystem.MIN_GAP - 0.01)
	assert_gt(float(bus_on_bus_lane), bus_samples * 0.7, "on the avenue, buses keep to their lane (out of it only to turn left)")
	assert_eq(sim.vehicles.get_stuck_count(30.0), 0)


## Phase 8 "done when": on the Bottleneck's jammed main road, bus lanes (one lane each
## way left for cars) and a bus line along them raise Flow: buses don't stop in or merge
## into the jammed lane, so riders without a car get there sooner and drivers too.
func test_bus_lanes_on_a_congested_corridor_raise_flow() -> void:
	var results := {}
	for with_bus in [false, true]:
		var run := Simulation.new()
		Scenarios.build_bottleneck(run, Vector2i(30, 30))
		if with_bus:
			Scenarios.add_bottleneck_bus_lanes(run, Vector2i(30, 30))
		for tick in Simulation.TICK_RATE * 120:
			run.step()
		var now := run.get_sim_time()
		results[with_bus] = { "flow": run.flow.get_flow_percent(now), "modes": run.flow.get_mode_shares(now) }
		run.dispose()
	gut.p("bottleneck without a bus %s, with bus lanes and a line %s" % [results[false], results[true]])
	assert_gt(results[true].modes.get(Citizen.Mode.BUS, 0.0), 0.1, "people ride")
	assert_gt(results[true].flow, results[false].flow + 2.0, "Flow rises")
