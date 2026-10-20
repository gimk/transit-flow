extends GutTest


func test_one_second_of_ticks_advances_clock_by_one_sim_second() -> void:
	var sim := Simulation.new()
	for i in Simulation.TICK_RATE:
		sim.step()
	assert_eq(sim.tick_count, Simulation.TICK_RATE)
	assert_almost_eq(sim.get_sim_time(), 1.0, 0.0001)
	assert_eq(sim.clock.format_time(), "06:05")
