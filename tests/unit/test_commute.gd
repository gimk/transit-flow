extends GutTest
## End to end on a whole Simulation: a citizen commutes home -> work -> home.

const G := 64.0

var sim: Simulation


func before_each() -> void:
	sim = Simulation.new()


func after_each() -> void:
	sim.dispose()


func _road(from: Vector2, to: Vector2) -> int:
	return sim.roads.add_road(from * G, to * G, RoadType.load_by_id(&"local_2lane"))


func _citizen() -> Citizen:
	return sim.citizens.citizens.values()[0]


## Steps the sim, recording each state the first citizen goes through (deduplicated).
func _run(ticks: int) -> Array:
	var states := [_citizen().state]
	for i in ticks:
		sim.step()
		if _citizen().state != states[-1]:
			states.append(_citizen().state)
	return states


func test_one_citizen_per_home() -> void:
	sim.lots.add_lot(Vector2i(1, 3), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(2, 3), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(5, 3), Lot.Type.INDUSTRIAL)
	assert_eq(sim.citizens.citizens.size(), 2)
	sim.lots.remove_lot(sim.lots.get_lot_at_cell(Vector2i(1, 3)))
	assert_eq(sim.citizens.citizens.size(), 1)


func test_full_commute_loop_repeats() -> void:
	_road(Vector2(0, 4), Vector2(6, 4))
	_road(Vector2(6, 4), Vector2(6, 10))
	sim.lots.add_lot(Vector2i(1, 3), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(6, 8), Lot.Type.INDUSTRIAL)
	var S := Citizen.State
	var states := _run(Simulation.TICK_RATE * 120)
	assert_eq(states.slice(0, 7), [S.AT_HOME, S.DRIVING_TO_WORK, S.AT_WORK, S.DRIVING_HOME,
		S.AT_HOME, S.DRIVING_TO_WORK, S.AT_WORK])


func test_vehicle_exists_only_while_driving() -> void:
	_road(Vector2(0, 4), Vector2(6, 4))
	sim.lots.add_lot(Vector2i(1, 3), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(4, 4), Lot.Type.INDUSTRIAL)
	for i in Simulation.TICK_RATE * 60:
		sim.step()
		assert_eq(sim.vehicles.vehicles.size(), 1 if _citizen().is_travelling() else 0)
		if is_failing():
			return


func test_waits_without_a_road_and_starts_once_one_is_built() -> void:
	sim.lots.add_lot(Vector2i(1, 3), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(5, 3), Lot.Type.INDUSTRIAL)
	assert_eq(_run(Simulation.TICK_RATE * 10), [Citizen.State.AT_HOME])
	_road(Vector2(0, 4), Vector2(8, 4))
	assert_has(_run(Simulation.TICK_RATE * 10), Citizen.State.DRIVING_TO_WORK)


func test_bulldozing_the_only_road_mid_trip_sends_citizen_home() -> void:
	var road := _road(Vector2(0, 4), Vector2(8, 4))
	sim.lots.add_lot(Vector2i(1, 3), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(6, 3), Lot.Type.INDUSTRIAL)
	while _citizen().state != Citizen.State.DRIVING_TO_WORK:
		sim.step()
	sim.roads.remove_segment(road)
	sim.step()
	assert_eq(_citizen().state, Citizen.State.AT_HOME)
	assert_eq(sim.vehicles.vehicles.size(), 0)


func test_removing_the_workplace_mid_trip_fails_the_trip() -> void:
	_road(Vector2(0, 4), Vector2(8, 4))
	sim.lots.add_lot(Vector2i(1, 3), Lot.Type.RESIDENTIAL)
	var work := sim.lots.add_lot(Vector2i(6, 3), Lot.Type.INDUSTRIAL)
	while _citizen().state != Citizen.State.DRIVING_TO_WORK:
		sim.step()
	sim.lots.remove_lot(work)
	sim.step()
	assert_eq(_citizen().state, Citizen.State.AT_HOME)
	assert_eq(_citizen().work_lot_id, -1)
	assert_eq(sim.vehicles.vehicles.size(), 0)


func test_save_round_trip_keeps_lots() -> void:
	_road(Vector2(0, 4), Vector2(6, 4))
	sim.lots.add_lot(Vector2i(1, 3), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(4, 4), Lot.Type.INDUSTRIAL)
	var saved := sim.to_dict()
	var loaded := Simulation.from_dict(JSON.parse_string(JSON.stringify(saved, "", true, true)))
	assert_eq_deep(loaded.to_dict(), saved)
	assert_eq(loaded.citizens.citizens.size(), 1, "citizens are recreated from homes")
	loaded.dispose()
