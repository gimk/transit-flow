extends GutTest
## Phase 3b: citizens, jobs and the daily schedule, on a whole Simulation.

const G := 64.0

var sim: Simulation


func before_each() -> void:
	sim = Simulation.new()


func after_each() -> void:
	sim.dispose()


func _road(from: Vector2, to: Vector2) -> int:
	return sim.roads.add_road(from * G, to * G, RoadType.load_by_id(&"local_2lane"))


func _steps(count: int) -> void:
	for i in count:
		sim.step()


## Runs the sim for `game_hours`, recording (hour of day, new state) whenever a citizen's
## state changes. Returns citizen id -> Array of [hour, state].
func _run_hours(game_hours: float) -> Dictionary:
	var history := {}
	var last := {}
	var ticks := int(game_hours * 60.0 / SimClock.GAME_MINUTES_PER_SIM_SECOND * Simulation.TICK_RATE)
	for i in ticks:
		sim.step()
		for citizen: Citizen in sim.citizens.citizens.values():
			if last.get(citizen.id, -1) != citizen.state:
				last[citizen.id] = citizen.state
				if not history.has(citizen.id):
					history[citizen.id] = []
				history[citizen.id].append([sim.clock.get_time_of_day_hours(), citizen.state])
	return history


## A street with homes on the north side and workplaces on the south side.
func _small_town(homes: int, factories: int, shops: int) -> void:
	_road(Vector2(0, 4), Vector2(20, 4))
	for x in homes:
		sim.lots.add_lot(Vector2i(1 + x, 3), Lot.Type.RESIDENTIAL)
	for x in factories:
		sim.lots.add_lot(Vector2i(10 + x, 4), Lot.Type.INDUSTRIAL)
	for x in shops:
		sim.lots.add_lot(Vector2i(16 + x, 4), Lot.Type.COMMERCIAL)


func test_each_home_has_two_residents_who_find_the_nearest_free_jobs() -> void:
	_road(Vector2(0, 4), Vector2(20, 4))
	sim.lots.add_lot(Vector2i(1, 3), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(2, 3), Lot.Type.RESIDENTIAL)
	var near_shop := sim.lots.add_lot(Vector2i(3, 4), Lot.Type.COMMERCIAL)  # 2 jobs
	var far_factory := sim.lots.add_lot(Vector2i(15, 4), Lot.Type.INDUSTRIAL)  # 3 jobs
	_steps(2)
	assert_eq(sim.citizens.citizens.size(), 4)
	var workplaces := sim.citizens.citizens.values().map(func(c: Citizen) -> int: return c.work_lot_id)
	assert_eq(workplaces.count(near_shop), 2, "nearest workplace fills up first")
	assert_eq(workplaces.count(far_factory), 2)
	assert_eq(sim.citizens.get_job_count(), 5)
	assert_eq(sim.citizens.get_employed_count(), 4)


func test_without_enough_jobs_some_stay_home() -> void:
	_small_town(3, 0, 1)  # 6 people, 2 jobs.
	var history := _run_hours(4.0)
	var commuters := 0
	for citizen: Citizen in sim.citizens.citizens.values():
		if citizen.work_lot_id != -1:
			commuters += 1
		else:
			assert_eq(history[citizen.id].size(), 1, "the unemployed never leave home")
	assert_eq(commuters, 2)


func test_a_day_follows_the_rush_hours() -> void:
	_small_town(4, 3, 0)
	var history := _run_hours(24.0)
	var S := Citizen.State
	for citizen_id: int in history:
		var events: Array = history[citizen_id]
		var left_home := events.filter(func(e: Array) -> bool: return e[1] == S.TO_WORK)
		var left_work := events.filter(func(e: Array) -> bool: return e[1] == S.TO_HOME)
		assert_eq(left_home.size(), 1, "one commute a day")
		assert_eq(left_work.size(), 1)
		assert_between(left_home[0][0], 6.5, 9.1, "left home in the morning rush")
		assert_between(left_work[0][0], 16.0, 19.1, "left work in the evening rush")
		assert_eq(events[-1][1], S.AT_HOME, "home again at the end of the day")


func test_some_people_go_shopping_after_work() -> void:
	_small_town(8, 3, 2)
	var history := _run_hours(24.0)
	var shoppers := 0
	for events: Array in history.values():
		if events.any(func(e: Array) -> bool: return e[1] == Citizen.State.AT_SHOP):
			shoppers += 1
	assert_between(shoppers, 1, 15, "some, not all, went shopping")


func test_bulldozing_the_only_road_mid_trip_still_ends_at_the_destination() -> void:
	var road := _road(Vector2(0, 4), Vector2(8, 4))
	sim.lots.add_lot(Vector2i(1, 3), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(6, 3), Lot.Type.INDUSTRIAL)
	sim.citizens.depart_everyone_now()
	while not sim.citizens.citizens.values().any(func(c: Citizen) -> bool: return c.state == Citizen.State.TO_WORK):
		sim.step()
	sim.roads.remove_segment(road)
	sim.step()
	assert_eq(sim.vehicles.vehicles.size(), 0)
	assert_true(sim.citizens.citizens.values().all(
		func(c: Citizen) -> bool: return c.state == Citizen.State.AT_WORK or c.state == Citizen.State.AT_HOME))


func test_losing_the_workplace_mid_trip_sends_people_home_job_hunting() -> void:
	_road(Vector2(0, 4), Vector2(8, 4))
	sim.lots.add_lot(Vector2i(1, 3), Lot.Type.RESIDENTIAL)
	var work := sim.lots.add_lot(Vector2i(6, 3), Lot.Type.INDUSTRIAL)
	sim.citizens.depart_everyone_now()
	while sim.vehicles.vehicles.is_empty():
		sim.step()
	sim.lots.remove_lot(work)
	sim.step()
	for citizen: Citizen in sim.citizens.citizens.values():
		assert_eq(citizen.state, Citizen.State.AT_HOME)
		assert_eq(citizen.work_lot_id, -1)
	assert_eq(sim.vehicles.vehicles.size(), 0)


func test_save_round_trip_keeps_lots_and_recreates_residents() -> void:
	_small_town(2, 1, 1)
	_steps(2)
	var saved := sim.to_dict()
	var loaded := Simulation.from_dict(JSON.parse_string(JSON.stringify(saved, "", true, true)))
	assert_eq_deep(loaded.to_dict(), saved)
	assert_eq(loaded.citizens.citizens.size(), 4, "residents are recreated from homes")
	loaded.dispose()


func test_shoppers_never_pick_the_shop_they_work_at() -> void:
	sim.roads.add_road(Vector2(0, 4) * 64, Vector2(12, 4) * 64, RoadType.load_by_id(&"local_2lane"))
	var only_shop := sim.lots.add_lot(Vector2i(5, 3), Lot.Type.COMMERCIAL)
	assert_eq(sim.citizens._pick_shop(only_shop), -1, "the only shop is their own: no shopping")
	var other := sim.lots.add_lot(Vector2i(8, 3), Lot.Type.COMMERCIAL)
	for i in 10:
		assert_eq(sim.citizens._pick_shop(only_shop), other)
