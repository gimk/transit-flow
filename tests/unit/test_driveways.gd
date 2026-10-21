extends GutTest
## Phase 6: cars leave and arrive through driveways, disturbing the traffic going by.

const G := 64.0
const DT := Simulation.TICK_DT
const Checks := preload("res://tests/traffic_checks.gd")
const NOWHERE: Array[Dictionary] = []

var sim: Simulation
var roads: RoadNetwork


func before_each() -> void:
	sim = Simulation.new()
	roads = sim.roads
	# An east-west street along y = 4, open to the east, a dead end to the west.
	roads.add_road(Vector2(0, 4) * G, Vector2(16, 4) * G, RoadType.load_by_id(&"local_2lane"))


func after_each() -> void:
	sim.dispose()


## Lots on the north side of the street: their near lane is the westbound one.
func _lot(x: int) -> int:
	return sim.lots.add_lot(Vector2i(x, 3), Lot.Type.RESIDENTIAL)


func _westbound() -> Lane:
	return roads.lanes[roads.find_lane_position(Vector2(8, 4) * G + Vector2(0, -7), 4.0).lane_id]


func _eastbound() -> Lane:
	return roads.lanes[roads.find_lane_position(Vector2(8, 4) * G + Vector2(0, 7), 4.0).lane_id]


## A car every `every` ticks along a lane, from near its start to near its end.
func _stream(lane: Lane, tick: int, every: int, owner := 9) -> void:
	if tick % every == 0 and sim.vehicles.can_spawn_at(lane.id, 10.0):
		sim.vehicles.spawn(sim.router.find_route(lane.id, 10.0, lane.id, lane.length - 5.0), -1, owner)


func test_pulling_out_waits_for_a_gap_in_the_traffic() -> void:
	var home := _lot(8)
	var shop := _lot(2)  # West: a right turn onto the near lane.
	var west := _westbound()
	var car: Vehicle = null
	var waited := 0
	var worst_gap := INF
	for tick in 1200:
		_stream(west, tick, 25)
		if tick == 100:
			car = sim.vehicles.depart(home, NOWHERE, shop, NOWHERE, 1)
			assert_not_null(car)
			assert_true(car.crossing_lanes.is_empty(), "joins the near lane: nothing to cross")
		if car != null and sim.vehicles.get_waiting_departures().has(car):
			waited += 1
		sim.step()
		worst_gap = minf(worst_gap, Checks.min_queue_gap(sim))
	assert_gt(waited, 0, "had to wait for a gap")
	assert_false(sim.vehicles.get_waiting_departures().has(car), "got out in the end")
	assert_false(sim.vehicles.vehicles.has(car.id), "and arrived")
	assert_gte(worst_gap, VehicleSystem.MIN_GAP - 0.01, "never squeezed in too close")


func test_turning_out_across_the_road_waits_for_both_directions() -> void:
	var home := _lot(8)
	var exit := [{ "lane_id": _eastbound().id, "distance": _eastbound().length - 5.0 }]
	var points: Array[Dictionary] = []
	points.assign(exit)
	var west := _westbound()
	var car: Vehicle = null
	for tick in 1200:
		_stream(west, tick, 30)
		if tick == 100:
			car = sim.vehicles.depart(home, NOWHERE, -1, points, 1)
			assert_false(car.crossing_lanes.is_empty(), "must cross the westbound lane first")
		var waiting_before := car != null and sim.vehicles.get_waiting_departures().has(car)
		sim.step()
		if waiting_before and sim.vehicles.vehicles.has(car.id):
			# It just pulled out: nobody was on the crossing point or about to be.
			var at: float = roads.find_lane_position(sim.lots.lots[home].get_center(), 48.0).distance
			for other: Vehicle in west.queue:
				var clear := absf(other.distance - at) - other.length * 0.5 - VehicleSystem.CROSSING_HALF_WIDTH
				assert_gt(clear, 0.0, "the crossing point was clear")
	assert_false(sim.vehicles.vehicles.has(car.id) or sim.vehicles.get_waiting_departures().has(car), "made it")


func test_turning_in_across_the_road_holds_up_the_traffic_behind() -> void:
	var shop := _lot(10)  # North side: from the eastbound lane, a left turn in.
	var east := _eastbound()
	var west := _westbound()
	var start: Array[Dictionary] = [{ "lane_id": east.id, "distance": 60.0 }]
	var behind_start: Array[Dictionary] = [{ "lane_id": east.id, "distance": 25.0 }]
	var beyond: Array[Dictionary] = [{ "lane_id": east.id, "distance": east.length - 5.0 }]
	var turning := sim.vehicles.depart(-1, start, shop, NOWHERE, 1)
	var follower: Vehicle = null
	var waited_to_turn := false
	var crossed := 0
	var follower_stopped_behind := false
	var worst_gap := INF
	for tick in 1500:
		_stream(west, tick, 18)
		if tick == 10:
			follower = sim.vehicles.depart(-1, behind_start, -1, beyond, 2)
		sim.step()
		worst_gap = minf(worst_gap, Checks.min_queue_gap(sim))
		if sim.vehicles.vehicles.has(turning.id) and turning.driveway_wait_since < INF:
			waited_to_turn = true
			crossed = turning.crossing_lanes.size()
			if follower != null and sim.vehicles.vehicles.has(follower.id) and follower.speed < 1.0:
				follower_stopped_behind = follower_stopped_behind or follower.distance < turning.distance
	gut.p("turning car waited: %s, follower stopped behind: %s" % [waited_to_turn, follower_stopped_behind])
	assert_eq(crossed, 1, "it turns across the westbound lane")
	assert_true(waited_to_turn, "waited for a gap in the oncoming traffic")
	assert_true(follower_stopped_behind, "the car behind had to stop for it")
	assert_false(sim.vehicles.vehicles.has(turning.id), "turned in (or went round) in the end")
	assert_gte(worst_gap, VehicleSystem.MIN_GAP - 0.01)


func test_arriving_cars_slow_down_to_turn_in() -> void:
	var shop := _lot(10)
	var west := _westbound()
	var start: Array[Dictionary] = [{ "lane_id": west.id, "distance": 10.0 }]
	var car := sim.vehicles.depart(-1, start, shop, NOWHERE, 1)
	assert_true(car.crossing_lanes.is_empty(), "near side: no crossing")
	var last_speed := 0.0
	var top_speed := 0.0
	for tick in 600:
		sim.step()
		if not sim.vehicles.vehicles.has(car.id):
			break
		last_speed = car.speed
		top_speed = maxf(top_speed, car.speed)
	assert_gt(top_speed, 60.0, "drove at speed on the way")
	assert_between(last_speed, VehicleSystem.MIN_SPEED, VehicleSystem.DRIVEWAY_SPEED + 4.0, "turned in at driveway speed")


func test_cars_waiting_in_driveways_are_drawn_on_their_lot() -> void:
	var home := _lot(8)
	var shop := _lot(2)
	var car := sim.vehicles.depart(home, NOWHERE, shop, NOWHERE, 1)
	var second := sim.vehicles.depart(home, NOWHERE, shop, NOWHERE, 2)
	var snapshot := ViewSnapshot.capture(sim)
	assert_eq(snapshot.waiting_count, 2)
	assert_true(snapshot.vehicle_index.has(car.id) and snapshot.vehicle_index.has(second.id))
	var lot_center := sim.lots.lots[home].get_center()
	var first_at := snapshot.positions[snapshot.vehicle_index[car.id]]
	var second_at := snapshot.positions[snapshot.vehicle_index[second.id]]
	assert_lt(first_at.y, 4 * G - 7.0, "off the road, on the lot side")
	assert_lt(second_at.distance_to(lot_center), first_at.distance_to(lot_center), "the second one waits behind the first")


func test_lot_right_at_an_avenue_junction_can_pull_out() -> void:
	# A wide avenue's lanes start well back from the junction, so a corner lot's driveway
	# point lands at the very start of the lane: the car must still get out.
	roads.add_road(Vector2(8, 0) * G, Vector2(8, 8) * G, RoadType.load_by_id(&"avenue_4lane"))
	var corner := sim.lots.add_lot(Vector2i(8, 4), Lot.Type.COMMERCIAL)
	var home := _lot(14)
	var car := sim.vehicles.depart(corner, NOWHERE, home, NOWHERE, 1)
	assert_not_null(car)
	for tick in 200:
		sim.step()
	assert_false(sim.vehicles.get_waiting_departures().has(car), "pulled out")


func test_a_trip_to_the_same_lot_needs_no_car() -> void:
	var shop := _lot(8)
	assert_null(sim.vehicles.depart(shop, NOWHERE, shop, NOWHERE, 1), "already there")
	assert_true(sim.vehicles.get_waiting_departures().is_empty())
