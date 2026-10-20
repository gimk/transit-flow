extends GutTest
## Vehicle motion along routes, driven tick by tick.

const G := 64.0
const DT := Simulation.TICK_DT

var roads: RoadNetwork
var router: Router
var lots: LotSystem
var vehicles: VehicleSystem


func before_each() -> void:
	roads = RoadNetwork.new()
	router = Router.new(roads)
	lots = LotSystem.new(roads)
	vehicles = VehicleSystem.new(roads, router, lots, TrafficLights.new(roads))


func after_each() -> void:
	roads.dispose()


func _road(from: Vector2, to: Vector2) -> int:
	return roads.add_road(from * G, to * G, RoadType.load_by_id(&"local_2lane"))


## Spawns a vehicle heading for the access point of a lot at `destination_cell`.
func _spawn_to_lot(origin_cell: Vector2i, destination_cell: Vector2i) -> Vehicle:
	var origin_lot := lots.add_lot(origin_cell, Lot.Type.RESIDENTIAL)
	var destination_lot := lots.add_lot(destination_cell, Lot.Type.INDUSTRIAL)
	var from := lots.get_access(origin_lot)
	var to := lots.get_access(destination_lot)
	var route := router.find_route(from.lane_id, from.distance, to.lane_id, to.distance)
	assert_not_null(route, "test setup: route exists")
	return vehicles.spawn(route, destination_lot, 7)


## Steps until the vehicle arrives or fails; returns the number of ticks, or -1 on timeout.
func _run_until_gone(vehicle: Vehicle, max_ticks := 4000, on_tick: Callable = Callable()) -> int:
	for tick in max_ticks:
		vehicles.step(DT)
		if not vehicles.vehicles.has(vehicle.id):
			return tick
		if on_tick.is_valid():
			on_tick.call(tick)
	return -1


func test_vehicle_drives_around_a_corner_and_arrives() -> void:
	_road(Vector2(0, 4), Vector2(6, 4))
	_road(Vector2(6, 4), Vector2(6, 10))
	var vehicle := _spawn_to_lot(Vector2i(1, 4), Vector2i(6, 8))
	watch_signals(vehicles)
	assert_ne(_run_until_gone(vehicle), -1, "arrived within the time limit")
	assert_signal_emitted_with_parameters(vehicles, "vehicle_arrived", [vehicle])
	# It stops in front of the lot on either side of the street (turning across the road
	# costs a wait, so it may prefer the near side via the dead end).
	var stopped_at_a_door := false
	for door: Dictionary in lots.get_access_points(vehicle.destination_lot_id):
		if vehicle.lane.id == door.lane_id and absf(vehicle.distance - door.distance) < 0.001:
			stopped_at_a_door = true
	assert_true(stopped_at_a_door)


func test_motion_is_continuous_and_respects_speed_limits() -> void:
	_road(Vector2(0, 4), Vector2(6, 4))
	_road(Vector2(6, 4), Vector2(6, 10))
	var vehicle := _spawn_to_lot(Vector2i(1, 4), Vector2i(6, 8))
	var state := { "max_jump": 0.0, "over_limit": 0.0, "slowest_turn_speed": INF }
	_run_until_gone(vehicle, 4000, func(_tick: int) -> void:
		state.max_jump = maxf(state.max_jump, vehicle.position.distance_to(vehicle.previous_position))
		state.over_limit = maxf(state.over_limit, vehicle.speed - vehicle.lane.speed_limit)
		if vehicle.lane.turn == Lane.Turn.RIGHT:
			state.slowest_turn_speed = minf(state.slowest_turn_speed, vehicle.speed))
	var fastest := RoadType.load_by_id(&"local_2lane").speed_limit
	assert_lt(state.max_jump, fastest * DT + 1.0, "no teleporting between ticks")
	assert_lt(state.over_limit, VehicleSystem.DECELERATION * DT + 1.0, "at most one tick of braking above a lane limit")
	assert_lt(state.slowest_turn_speed, INF, "drove through the right-turn connector")


func test_vehicle_brakes_before_the_turn() -> void:
	_road(Vector2(0, 4), Vector2(8, 4))
	_road(Vector2(8, 4), Vector2(8, 10))
	var vehicle := _spawn_to_lot(Vector2i(0, 4), Vector2i(8, 8))
	var entry := { "speed": -1.0 }
	_run_until_gone(vehicle, 4000, func(_tick: int) -> void:
		if entry.speed < 0.0 and vehicle.lane.turn == Lane.Turn.RIGHT:
			entry.speed = vehicle.speed)
	var turn_limit := RoadType.load_by_id(&"local_2lane").speed_limit * RoadNetwork.TURN_SPEED_FACTORS[Lane.Turn.RIGHT]
	assert_between(entry.speed, 0.0, turn_limit + VehicleSystem.DECELERATION * DT + 1.0, "entered the turn at turn speed")


func test_reroutes_when_a_road_on_the_route_is_bulldozed() -> void:
	# Two ways from the corner (4,4) to the corner (10,4): direct along y=4, or around via y=8.
	_road(Vector2(1, 4), Vector2(4, 4))
	var direct := _road(Vector2(4, 4), Vector2(10, 4))
	_road(Vector2(4, 4), Vector2(4, 8))
	_road(Vector2(4, 8), Vector2(10, 8))
	_road(Vector2(10, 8), Vector2(10, 4))
	_road(Vector2(10, 4), Vector2(13, 4))
	var vehicle := _spawn_to_lot(Vector2i(1, 4), Vector2i(12, 4))
	for i in 10:
		vehicles.step(DT)
	roads.remove_segment(direct)
	watch_signals(vehicles)
	assert_ne(_run_until_gone(vehicle), -1)
	assert_signal_emitted(vehicles, "vehicle_arrived")
	assert_signal_not_emitted(vehicles, "vehicle_failed")


func test_fails_cleanly_when_no_route_remains() -> void:
	var only := _road(Vector2(0, 4), Vector2(8, 4))
	var vehicle := _spawn_to_lot(Vector2i(1, 4), Vector2i(6, 4))
	vehicles.step(DT)
	roads.remove_segment(only)
	watch_signals(vehicles)
	vehicles.step(DT)
	assert_signal_emitted_with_parameters(vehicles, "vehicle_failed", [vehicle])
	assert_eq(vehicles.vehicles.size(), 0)


func test_finishes_a_connector_that_was_rebuilt_under_it() -> void:
	_road(Vector2(0, 4), Vector2(6, 4))
	_road(Vector2(6, 4), Vector2(6, 10))
	var vehicle := _spawn_to_lot(Vector2i(1, 4), Vector2i(6, 8))
	while not vehicle.lane.is_connector():
		vehicles.step(DT)
	# Building another road at the same junction regenerates its connectors.
	_road(Vector2(6, 4), Vector2(10, 4))
	assert_false(roads.lanes.has(vehicle.lane.id), "the connector it is on was replaced")
	watch_signals(vehicles)
	assert_ne(_run_until_gone(vehicle), -1)
	assert_signal_emitted(vehicles, "vehicle_arrived")


func test_keeps_driving_when_its_road_is_split_by_a_new_crossing() -> void:
	_road(Vector2(0, 4), Vector2(10, 4))
	var vehicle := _spawn_to_lot(Vector2i(0, 4), Vector2i(9, 4))
	for i in 20:
		vehicles.step(DT)
	var lane_before := vehicle.lane.id
	var position_before := vehicle.position
	# Cross ahead of the car, and also right where the car is.
	_road(Vector2(6, 0), Vector2(6, 8))
	_road(Vector2(floorf(vehicle.position.x / G), 0), Vector2(floorf(vehicle.position.x / G), 8))
	assert_false(roads.lanes.has(lane_before), "the lane it was on was replaced")
	watch_signals(vehicles)
	vehicles.step(DT)
	assert_signal_not_emitted(vehicles, "vehicle_failed")
	assert_lt(vehicle.position.distance_to(position_before), 10.0, "no jump")
	assert_ne(_run_until_gone(vehicle), -1)
	assert_signal_emitted(vehicles, "vehicle_arrived")
