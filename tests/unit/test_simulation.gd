extends GutTest


func test_one_second_of_ticks_advances_clock_by_one_sim_second() -> void:
	var sim := Simulation.new()
	for i in Simulation.TICK_RATE:
		sim.step()
	assert_eq(sim.tick_count, Simulation.TICK_RATE)
	assert_almost_eq(sim.get_sim_time(), 1.0, 0.0001)
	assert_eq(sim.clock.format_time(), "06:05")


func test_save_round_trip_through_json() -> void:
	var sim := Simulation.new()
	sim.roads.add_road(Vector2(0, 0), Vector2(256, 0), RoadType.load_by_id(&"local_2lane"))
	for i in 37:
		sim.step()
	var saved := sim.to_dict()
	var loaded := Simulation.from_dict(JSON.parse_string(JSON.stringify(saved, "", true, true)))
	assert_not_null(loaded)
	assert_eq_deep(loaded.to_dict(), saved)
	assert_eq(loaded.clock.format_time(), sim.clock.format_time())


func test_rejects_unknown_save_version() -> void:
	assert_null(Simulation.from_dict({"version": 999}))
