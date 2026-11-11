extends SceneTree
## R3 benchmark (GDScript side): the Town for a full game day, then the Town again with
## 2 000 stress vehicles on top for two sim minutes. Prints the average and worst cost
## of a tick. Rust counterpart: `cargo run -p tf_sim --release --bin town_benchmark`.
##   /Applications/Godot.app/Contents/MacOS/Godot --headless --script res://migration/probes/town_benchmark.gd


func _init() -> void:
	_run("Town, one day", 0, int(24.0 * 60.0 / SimClock.GAME_MINUTES_PER_SIM_SECOND * Simulation.TICK_RATE))
	_run("Town + 2000 stress, 2 min", 2000, Simulation.TICK_RATE * 120)
	quit()


func _run(label: String, stress: int, ticks: int) -> void:
	var sim := Simulation.new()
	Scenarios.build_town(sim)
	sim.stress.target_count = stress
	var total := 0
	var worst := 0
	var peak := 0
	for i in ticks:
		var start := Time.get_ticks_usec()
		sim.step()
		var cost := Time.get_ticks_usec() - start
		total += cost
		worst = maxi(worst, cost)
		peak = maxi(peak, sim.vehicles.vehicles.size())
	print("%s: %d ticks, %.3f ms/tick average, %.2f ms worst, peak %d vehicles, %d citizens" % [
		label, ticks, total / 1000.0 / ticks, worst / 1000.0, peak, sim.citizens.citizens.size()])
	sim.dispose()
