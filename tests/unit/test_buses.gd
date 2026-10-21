extends GutTest
## Phase 8b: bus stops, bus lines, buses, riders, and the bus in mode choice.

const G := 64.0

var sim: Simulation


func before_each() -> void:
	sim = Simulation.new()


func after_each() -> void:
	sim.dispose()


func _road(from: Vector2, to: Vector2, type := &"local_2lane") -> void:
	sim.roads.add_road(from * G, to * G, RoadType.load_by_id(type))


## A stop at the curb on the south (eastbound) or north (westbound) side of the road
## along y = `y`, at x = `x` cells.
func _stop(x: float, y: float, eastbound: bool) -> int:
	return sim.transit.add_stop(Vector2(x, y) * G + Vector2(0, 10 if eastbound else -10))


## A long street (y 4) with a line along it both ways: east on the south side, back west
## on the north side, turning round at the dead ends.
func _street_line() -> int:
	_road(Vector2(0, 4), Vector2(24, 4))
	var stops: Array[int] = []
	for x in [3, 9, 15, 21]:
		stops.append(_stop(x, 4, true))
	for x in [21, 15, 9, 3]:
		stops.append(_stop(x, 4, false))
	return sim.transit.add_line(stops)


func _step(ticks: int) -> void:
	for i in ticks:
		sim.step()


func test_stops_go_at_the_curb() -> void:
	_road(Vector2(0, 4), Vector2(12, 4), &"avenue_4lane")
	var east := _stop(5, 4, true)
	assert_ne(east, -1)
	var stop := sim.transit.stops[east]
	var lane := sim.roads.lanes[stop.lane_id]
	assert_eq(lane.index_from_center, 1, "the outer lane of the avenue")
	assert_almost_eq(stop.direction, Vector2.RIGHT, Vector2(0.01, 0.01), "south side: eastbound")
	assert_gt(stop.position.y, 4 * G + 14.0)
	assert_eq(sim.transit.add_stop(stop.position + Vector2(8, 0)), east, "clicking next to a stop picks it")
	var west := _stop(5, 4, false)
	assert_almost_eq(sim.transit.stops[west].direction, Vector2.LEFT, Vector2(0.01, 0.01))
	assert_eq(sim.transit.add_stop(Vector2(5, 9) * G), -1, "no road there")


## With nobody about, buses drive past every stop round the line; a rider waiting at a
## stop makes the next bus stop there (holding up the lane while they get on).
func test_buses_run_round_the_line_stopping_only_where_needed() -> void:
	var line_id := _street_line()
	var line := sim.transit.lines[line_id]
	assert_gt(line.bus_count, 1, "enough buses for a short headway")
	assert_lt(line.get_loop_time(), INF, "every stop reachable")
	var headed_for := {}
	var dwelt := false
	for i in Simulation.TICK_RATE * 40:
		sim.step()
		for bus: TransitSystem.Bus in sim.transit.buses.values():
			headed_for[bus.stop_index] = true
			dwelt = dwelt or bus.vehicle.dwelling
	assert_eq(sim.transit.buses.size(), line.bus_count)
	assert_eq(headed_for.size(), line.stop_ids.size(), "buses went round every stop")
	assert_false(dwelt, "nobody waiting, nobody getting off: no stopping")
	var stop_id := line.stop_ids[2]
	assert_true(sim.transit.wait_for_bus(999, line_id, stop_id, line.stop_ids[5]))
	var stopped_there := false
	for i in Simulation.TICK_RATE * 40:
		sim.step()
		for bus: TransitSystem.Bus in sim.transit.buses.values():
			stopped_there = stopped_there or (bus.vehicle.dwelling and line.stop_ids[bus.stop_index] == stop_id)
	assert_true(stopped_there, "a bus stopped for the rider")
	assert_eq(sim.transit.stops[stop_id].waiting.size(), 0, "who got on")
	assert_eq(sim.vehicles.get_stuck_count(30.0), 0)


func test_a_citizen_without_a_car_rides_the_bus() -> void:
	_street_line()
	var home := sim.lots.add_lot(Vector2i(3, 4), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(21, 4), Lot.Type.COMMERCIAL)
	sim.step()
	var citizen: Citizen = sim.citizens.citizens.values()[0]
	citizen.has_car = false
	var plan := sim.citizens.plan_trip(citizen, home, citizen.work_lot_id)
	assert_eq(plan.mode, Citizen.Mode.BUS)
	assert_eq(plan.legs.map(func(leg: Dictionary) -> int: return leg.mode),
		[Citizen.Mode.WALK, Citizen.Mode.BUS, Citizen.Mode.WALK])
	var trips: Array[Citizen.Mode] = []
	sim.citizens.trip_ended.connect(func(c: Citizen, completed: bool) -> void:
		if c == citizen and completed:
			trips.append(c.trip_mode))
	citizen.leave_now = true
	for i in Simulation.TICK_RATE * 60:
		sim.step()
		if citizen.state == Citizen.State.AT_WORK:
			break
	assert_eq(citizen.state, Citizen.State.AT_WORK)
	assert_eq(trips, [Citizen.Mode.BUS])


## A driver takes the car on an empty street, the bus when the street is jammed (driving
## gets slower; the bus timetable doesn't, until buses get caught in it too), and the bus
## when the car is parked out of reach.
func test_the_bus_when_it_is_quicker_the_car_when_it_is_not() -> void:
	_street_line()
	var home := sim.lots.add_lot(Vector2i(3, 4), Lot.Type.RESIDENTIAL)
	var work := sim.lots.add_lot(Vector2i(21, 4), Lot.Type.COMMERCIAL)
	sim.step()
	var citizen: Citizen = sim.citizens.citizens.values()[0]
	citizen.has_car = true
	citizen.car_preference = 1.0
	var free := sim.citizens.plan_trip(citizen, home, work)
	for lane: Lane in sim.roads.lanes.values():
		sim.router.graph.congestion[lane.id] = 1.0  # Twice as slow.
	sim.router.invalidate_cache()
	var jammed := sim.citizens.plan_trip(citizen, home, work)
	gut.p("car cost %.1f on free roads, %.1f jammed; bus %.1f" % [free.cost, sim.citizens._plan_drive(citizen, home, work).cost, jammed.cost])
	assert_eq(free.mode, Citizen.Mode.CAR)
	assert_eq(jammed.mode, Citizen.Mode.BUS)
	sim.router.graph.congestion.clear()
	sim.router.invalidate_cache()
	citizen.car_lot_id = sim.lots.add_lot(Vector2i(12, 4), Lot.Type.COMMERCIAL)
	assert_eq(sim.citizens.plan_trip(citizen, home, work).mode, Citizen.Mode.BUS, "the car is out of reach")


func test_riders_change_lines_at_a_shared_stop() -> void:
	# An east-west street and a north-south one crossing it at (12, 4).
	_road(Vector2(0, 4), Vector2(24, 4))
	_road(Vector2(12, 4), Vector2(12, 20))
	var shared := _stop(9, 4, true)
	var east_west := sim.transit.add_line([_stop(2, 4, true), shared, _stop(9, 4, false), _stop(2, 4, false)])
	var down := sim.transit.add_stop(Vector2(12, 7) * G + Vector2(-10, 0))
	var bottom := sim.transit.add_stop(Vector2(12, 18) * G + Vector2(-10, 0))
	var up := sim.transit.add_stop(Vector2(12, 18) * G + Vector2(10, 0))
	var north_south := sim.transit.add_line([shared, down, bottom, up])
	assert_ne(east_west, -1)
	assert_ne(north_south, -1)
	var plan := sim.transit.plan_ride(Vector2(2, 3.5) * G, Vector2(11.5, 18) * G, 4 * G, 1.5)
	var modes: Array = plan.legs.map(func(leg: Dictionary) -> int: return leg.mode)
	assert_eq(modes, [Citizen.Mode.WALK, Citizen.Mode.BUS, Citizen.Mode.BUS, Citizen.Mode.WALK])
	assert_eq(plan.legs[1].line, east_west)
	assert_eq(plan.legs[1].to, shared)
	assert_eq(plan.legs[2].line, north_south)
	assert_eq(plan.legs[2].from, shared)


func test_removing_a_line_strands_its_riders() -> void:
	var line_id := _street_line()
	var home := sim.lots.add_lot(Vector2i(3, 4), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(21, 4), Lot.Type.COMMERCIAL)
	sim.step()
	var citizen: Citizen = sim.citizens.citizens.values()[0]
	citizen.has_car = false
	citizen.leave_now = true
	var outcome := {}
	sim.citizens.trip_ended.connect(func(c: Citizen, completed: bool) -> void:
		if c == citizen:
			outcome.completed = completed)
	while not citizen.on_bus:
		sim.step()
	sim.transit.remove_line(line_id)
	assert_false(outcome.completed, "the trip failed")
	assert_eq(citizen.state, Citizen.State.AT_WORK, "but the day goes on")
	assert_eq(sim.transit.buses.size(), 0)
	assert_eq(sim.vehicles.vehicles.values().filter(func(v: Vehicle) -> bool: return v.in_service).size(), 0)
	assert_eq(sim.lots.lots.has(home), true)


func test_stops_and_lines_survive_road_edits() -> void:
	var line_id := _street_line()
	_step(Simulation.TICK_RATE * 5)
	# A crossing street splits the road under two of the stops' lanes...
	_road(Vector2(12, 0), Vector2(12, 8))
	# ...and the west end becomes an avenue: its stops move out to the new curb lane.
	sim.roads.set_segment_type(sim.roads.find_segment_at(Vector2(3, 4) * G, 4.0), RoadType.load_by_id(&"avenue_4lane"))
	_step(Simulation.TICK_RATE * 30)
	for stop: BusStop in sim.transit.stops.values():
		assert_true(stop.is_active(), "stop %d still on a road" % stop.id)
		assert_eq(sim.roads.lanes[stop.lane_id].index_from_center,
			sim.roads.segments[sim.roads.lanes[stop.lane_id].segment_id].road_type.lanes_per_direction - 1, "at the curb")
	assert_lt(sim.transit.lines[line_id].get_loop_time(), INF)
	assert_eq(sim.transit.buses.size(), sim.transit.lines[line_id].bus_count, "buses kept running (or came back)")
	assert_eq(sim.vehicles.get_stuck_count(20.0), 0)


func test_stops_and_lines_are_saved() -> void:
	var line_id := _street_line()
	var loaded := Simulation.from_dict(JSON.parse_string(JSON.stringify(sim.to_dict())))
	assert_eq(loaded.transit.stops.size(), 8)
	assert_eq(loaded.transit.lines[line_id].stop_ids, sim.transit.lines[line_id].stop_ids)
	assert_eq(loaded.transit.lines[line_id].bus_count, sim.transit.lines[line_id].bus_count)
	for stop: BusStop in loaded.transit.stops.values():
		assert_true(stop.is_active())
	loaded.dispose()


func test_no_trip_uses_more_than_three_modes() -> void:
	_street_line()
	for x in range(1, 23, 2):
		sim.lots.add_lot(Vector2i(x, 3), Lot.Type.RESIDENTIAL if x < 12 else Lot.Type.COMMERCIAL)
	sim.step()
	for citizen: Citizen in sim.citizens.citizens.values():
		if citizen.work_lot_id == -1:
			continue
		for has_car in [true, false]:
			citizen.has_car = has_car
			var plan := sim.citizens.plan_trip(citizen, citizen.home_lot_id, citizen.work_lot_id)
			var modes := {}
			var buses := 0
			for leg: Dictionary in plan.legs:
				modes[leg.mode] = true
				buses += 1 if leg.mode == Citizen.Mode.BUS else 0
			assert_lte(modes.size(), 3)
			assert_lte(buses, TransitSystem.MAX_TRANSFERS + 1)


## The Bottleneck suburb, where one person in five has no car: a bus line along the main
## road gets them to work much sooner than walking (overall Flow is printed: the buses
## also hold up the jammed cars at their stops, so it barely moves).
func test_a_bus_line_speeds_up_trips_without_a_car() -> void:
	var results := {}
	for with_bus in [false, true]:
		var run := Simulation.new()
		Scenarios.build_bottleneck(run, Vector2i(30, 30))
		if with_bus:
			Scenarios.add_bottleneck_bus_line(run, Vector2i(30, 30))
		var carless := { "time": 0.0, "trips": 0 }
		run.citizens.trip_ended.connect(func(c: Citizen, completed: bool) -> void:
			if not c.has_car and completed and c.trip_lot_id == c.work_lot_id:
				carless.time += c.trip_end_time - c.trip_start_time
				carless.trips += 1)
		for tick in Simulation.TICK_RATE * 120:
			run.step()
		var now := run.get_sim_time()
		results[with_bus] = { "flow": run.flow.get_flow_percent(now), "modes": run.flow.get_mode_shares(now),
			"carless commute": carless.time / maxi(carless.trips, 1), "carless trips": carless.trips }
		run.dispose()
	gut.p("bottleneck without a bus: %s" % results[false])
	gut.p("bottleneck with a bus line: %s" % results[true])
	assert_gt(results[true].modes.get(Citizen.Mode.BUS, 0.0), 0.1, "people ride")
	assert_eq(results[true]["carless trips"], results[false]["carless trips"], "the same commuters")
	assert_lt(results[true]["carless commute"], results[false]["carless commute"] * 0.85, "and get there sooner")


## On a grid with many ways round, and traffic to tempt them off it, buses keep to the
## lanes their line was laid out on (BusLine.routes), turnarounds included.
func test_buses_keep_to_their_line() -> void:
	for y in [2, 4, 6]:
		_road(Vector2(2, y), Vector2(22, y))
	for x in [2, 8, 16, 22]:
		_road(Vector2(x, 2), Vector2(x, 6))
	var stops: Array[int] = [_stop(5, 4, true), _stop(12, 4, true), _stop(19, 4, true),
		_stop(19, 4, false), _stop(12, 4, false), _stop(5, 4, false)]
	var line := sim.transit.lines[sim.transit.add_line(stops)]
	var line_lanes := {}
	for route: Route in line.routes:
		for lane_id in route.lane_ids:
			line_lanes[lane_id] = true
	sim.stress.target_count = 80
	var off_line := 0
	for i in Simulation.TICK_RATE * 60:
		sim.step()
		for bus: TransitSystem.Bus in sim.transit.buses.values():
			if not line_lanes.has(bus.vehicle.lane.id):
				off_line += 1
	assert_eq(sim.transit.buses.size(), line.bus_count)
	assert_eq(off_line, 0, "never off the line's lanes")
