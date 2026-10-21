extends GutTest
## Phase 8a: trips as legs, car availability, parking and mode choice.

const G := 64.0

var sim: Simulation
var home: int
var work: int


func before_each() -> void:
	sim = Simulation.new()
	sim.roads.add_road(Vector2(0, 4) * G, Vector2(20, 4) * G, RoadType.load_by_id(&"local_2lane"))
	home = sim.lots.add_lot(Vector2i(2, 3), Lot.Type.RESIDENTIAL)
	work = sim.lots.add_lot(Vector2i(14, 3), Lot.Type.COMMERCIAL)
	sim.step()  # Citizens move in and find jobs.


func after_each() -> void:
	sim.dispose()


func _citizen() -> Citizen:
	return sim.citizens.citizens.values()[0]


func _modes(plan: Dictionary) -> Array:
	return plan.legs.map(func(leg: Dictionary) -> int: return leg.mode)


## Steps until `citizen` reaches `state` (or `max_ticks` pass); returns ended trips.
func _run_until(citizen: Citizen, state: Citizen.State, max_ticks := 2000) -> Array[Dictionary]:
	var trips: Array[Dictionary] = []
	var record := func(c: Citizen, completed: bool) -> void:
		if c == citizen:
			trips.append({ "mode": c.trip_mode, "completed": completed, "ideal": c.trip_ideal_time,
				"took": c.trip_end_time - c.trip_start_time })
	sim.citizens.trip_ended.connect(record)
	for i in max_ticks:
		if citizen.state == state:
			break
		sim.step()
	sim.citizens.trip_ended.disconnect(record)
	return trips


func test_far_trips_are_driven_near_ones_walked() -> void:
	var citizen := _citizen()
	assert_eq(citizen.car_lot_id, home, "the car starts at home")
	assert_eq(sim.citizens.plan_trip(citizen, home, work).mode, Citizen.Mode.CAR, "12 cells: drive")
	var next_door := sim.lots.add_lot(Vector2i(4, 3), Lot.Type.COMMERCIAL)
	assert_eq(sim.citizens.plan_trip(citizen, home, next_door).mode, Citizen.Mode.WALK, "2 cells: walk")


func test_a_commute_by_car_ends_with_the_car_at_work() -> void:
	var citizen := _citizen()
	citizen.leave_now = true
	var trips := _run_until(citizen, Citizen.State.AT_WORK)
	assert_eq(citizen.state, Citizen.State.AT_WORK)
	assert_eq(trips.size(), 1, "one trip, scored once")
	assert_eq(trips[0].mode, Citizen.Mode.CAR)
	assert_true(trips[0].completed)
	assert_eq(citizen.car_lot_id, work, "parked at work")
	assert_eq(sim.citizens.parking.get_used(work), 1, "taking one of its spaces")


func test_without_a_car_people_walk_however_far() -> void:
	var citizen := _citizen()
	citizen.has_car = false
	var plan := sim.citizens.plan_trip(citizen, home, work)
	assert_eq(_modes(plan), [Citizen.Mode.WALK])
	citizen.leave_now = true
	var trips := _run_until(citizen, Citizen.State.AT_WORK, 4000)
	assert_eq(citizen.state, Citizen.State.AT_WORK)
	assert_eq(trips[0].mode, Citizen.Mode.WALK)


func test_a_full_car_park_sends_drivers_next_door_to_walk_the_rest() -> void:
	var next_door := sim.lots.add_lot(Vector2i(15, 3), Lot.Type.COMMERCIAL)
	for i in Parking.CAPACITY[Lot.Type.COMMERCIAL]:
		sim.citizens.parking.take(work)
	var citizen := _citizen()
	var plan := sim.citizens.plan_trip(citizen, home, work)
	assert_eq(_modes(plan), [Citizen.Mode.CAR, Citizen.Mode.WALK])
	assert_eq(plan.legs[0].to, next_door, "parks at the nearest free space")
	citizen.leave_now = true
	var trips := _run_until(citizen, Citizen.State.AT_WORK)
	assert_eq(citizen.state, Citizen.State.AT_WORK, "walked the rest")
	assert_eq(citizen.car_lot_id, next_door)
	assert_eq(trips.size(), 1)
	assert_gt(trips[0].ideal, 0.0)


func test_no_space_within_reach_means_parking_on_the_street() -> void:
	for i in Parking.CAPACITY[Lot.Type.COMMERCIAL]:
		sim.citizens.parking.take(work)
	var plan := sim.citizens.plan_trip(_citizen(), home, work)
	assert_eq(_modes(plan), [Citizen.Mode.CAR])
	assert_eq(plan.legs[0].to, work)


func test_drivers_walk_back_to_their_car_and_take_it_home() -> void:
	var citizen := _citizen()
	var car_park := sim.lots.add_lot(Vector2i(16, 3), Lot.Type.INDUSTRIAL)
	citizen.car_lot_id = car_park
	citizen.state = Citizen.State.AT_WORK
	var plan := sim.citizens.plan_trip(citizen, work, home, true)
	assert_eq(_modes(plan), [Citizen.Mode.WALK, Citizen.Mode.CAR])
	# Even when home is close enough to walk to, the car comes home.
	var near_home := sim.lots.add_lot(Vector2i(3, 3), Lot.Type.COMMERCIAL)
	citizen.car_lot_id = sim.lots.add_lot(Vector2i(4, 3), Lot.Type.COMMERCIAL)
	assert_eq(sim.citizens.plan_trip(citizen, near_home, home, true).mode, Citizen.Mode.CAR)
	assert_eq(sim.citizens.plan_trip(citizen, near_home, home, false).mode, Citizen.Mode.WALK, "but not when it isn't going home")


func test_a_car_left_far_away_cant_be_driven() -> void:
	var citizen := _citizen()
	citizen.car_lot_id = work  # Left at work; they're at home.
	var shop := sim.lots.add_lot(Vector2i(6, 3), Lot.Type.COMMERCIAL)
	assert_eq(_modes(sim.citizens.plan_trip(citizen, home, shop)), [Citizen.Mode.WALK])


func test_flow_scores_whole_trips_and_counts_modes() -> void:
	var citizen := _citizen()
	citizen.leave_now = true
	_run_until(citizen, Citizen.State.AT_WORK)
	var now := sim.get_sim_time()
	assert_eq(sim.flow.get_trip_count(now), 1, "the car leg isn't scored on its own")
	assert_eq(sim.flow.get_mode_shares(now), { Citizen.Mode.CAR: 1.0 })


func test_a_removed_car_park_sends_the_car_home() -> void:
	var citizen := _citizen()
	var car_park := sim.lots.add_lot(Vector2i(16, 3), Lot.Type.INDUSTRIAL)
	citizen.car_lot_id = car_park
	sim.citizens.parking.take(car_park)
	sim.lots.remove_lot(car_park)
	assert_eq(citizen.car_lot_id, home)
	assert_eq(sim.citizens.parking.get_used(car_park), 0)
