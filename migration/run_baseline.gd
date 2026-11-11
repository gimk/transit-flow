extends SceneTree
## Records the Godot build's behaviour metrics, which the Rust port must match within the
## bands in the output (migration work package M0.3; plan §3). Run from the project folder:
##   /Applications/Godot.app/Contents/MacOS/Godot --headless --script res://migration/run_baseline.gd
## Writes migration/baseline.md (takes a few minutes). The scenarios and measurements
## copy the GUT tests they come from, so the Rust tests can measure the same way:
## - Town day: tests/unit/test_town.gd (test_town_lives_through_a_whole_day),
## - Bottleneck mornings: tests/unit/test_bottleneck.gd (_morning_for_seed),
## - Bottleneck Flow after the morning (120 s): plain, bypass, bus line, bus lanes,
## - stress grids: tests/unit/test_traffic.gd (_stress_grid) and test_road_types.gd.

const Checks := preload("res://tests/traffic_checks.gd")
const OUT_PATH := "res://migration/baseline.md"
const G := 64.0
const BOTTLENECK_ORIGIN := Vector2i(30, 30)
const DAY_TICKS := Simulation.TICK_RATE * SimClock.MINUTES_PER_DAY / int(SimClock.GAME_MINUTES_PER_SIM_SECOND)
## The whole morning: every commuter is at work after ~90 s in the mornings above.
const MORNING_TICKS := Simulation.TICK_RATE * 120

var _lines: PackedStringArray = []


func _initialize() -> void:
	var started := Time.get_ticks_msec()
	_out("# Godot baseline")
	_out("")
	_out("Behaviour metrics of the Godot build (%s), recorded by `migration/run_baseline.gd` on %s. The Rust port must land inside the **band** column (plan §3). The RNG is bit-exact, so the Rust tests should run the same scenarios with the same seeds; where a metric is outside its band, find the first tick where the two runs diverge." % [
		Engine.get_version_info().string, Time.get_date_string_from_system()])
	_out("")
	_town_day()
	_bottleneck_mornings()
	_bottleneck_flow()
	_stress_grids()
	_out("")
	_out("Recorded in %.0f s." % ((Time.get_ticks_msec() - started) / 1000.0))
	var file := FileAccess.open(OUT_PATH, FileAccess.WRITE)
	file.store_string("\n".join(_lines) + "\n")
	file.close()
	print("wrote %s" % OUT_PATH)
	quit()


func _out(line: String) -> void:
	_lines.append(line)
	print(line)


func _row(metric: String, value: String, band: String) -> void:
	_out("| %s | %s | %s |" % [metric, value, band])


# --- Town -----------------------------------------------------------------------------

func _town_day() -> void:
	var sim := Simulation.new()
	Scenarios.build_town(sim)
	var stats := { "commutes": {}, "trucks": 0, "failed": 0, "exports": {}, "arrivals": 0 }
	sim.vehicles.vehicle_arrived.connect(func(vehicle: Vehicle) -> void:
		stats.arrivals += 1
		if vehicle.kind == Vehicle.Kind.TRUCK:
			stats.trucks += 1
			if vehicle.destination_lot_id == -1:
				stats.exports[vehicle.lane.to_node_id] = true)
	sim.citizens.trip_ended.connect(func(citizen: Citizen, completed: bool) -> void:
		if completed and citizen.state == Citizen.State.TO_WORK and not stats.commutes.has(citizen.id):
			stats.commutes[citizen.id] = (citizen.trip_end_time - citizen.trip_start_time) / citizen.trip_ideal_time)
	sim.vehicles.vehicle_failed.connect(func(_v: Vehicle) -> void: stats.failed += 1)
	var everyone_at_work := -1.0
	var violations := 0
	var worst_gap := INF
	var peak := 0
	var most_stuck := 0
	for tick in DAY_TICKS:
		sim.step()
		violations += Checks.connector_conflict_violations(sim)
		worst_gap = minf(worst_gap, Checks.min_queue_gap(sim))
		peak = maxi(peak, sim.vehicles.vehicles.size())
		var drivers := sim.citizens.citizens.values().filter(func(c: Citizen) -> bool: return c.has_car)
		var at_work := drivers.filter(func(c: Citizen) -> bool: return c.state == Citizen.State.AT_WORK).size()
		if everyone_at_work < 0.0 and not drivers.is_empty() and at_work >= drivers.size() * 0.95:
			everyone_at_work = sim.clock.get_time_of_day_hours()
		if tick % Simulation.TICK_RATE == 0:
			most_stuck = maxi(most_stuck, sim.vehicles.get_stuck_count(30.0))
	var commutes: Array = stats.commutes.values()
	commutes.sort()
	var median: float = commutes[commutes.size() / 2] if not commutes.is_empty() else 0.0
	var flow := sim.flow.get_flow_percent(sim.get_sim_time())
	var shares := sim.flow.get_mode_shares(sim.get_sim_time())
	var carless := sim.citizens.citizens.values().filter(func(c: Citizen) -> bool: return not c.has_car).size()
	_out("## Town, one day from 06:00")
	_out("")
	_out("`Scenarios.build_town`, %d ticks, the safety checks on every tick (as in `test_town_lives_through_a_whole_day`)." % DAY_TICKS)
	_out("")
	_out("| Metric | Godot | Band |")
	_out("|---|---|---|")
	_row("Citizens (cars / no car)", "%d (%d / %d)" % [sim.citizens.citizens.size(), sim.citizens.citizens.size() - carless, carless], "exact")
	_row("Flow %", "%.1f" % flow, "± 5 points")
	_row("Failed vehicle trips", str(stats.failed), "≤ Godot + 3")
	_row("Vehicle arrivals", str(stats.arrivals), "± 10%")
	_row("Truck trips", str(stats.trucks), "± 15%")
	_row("Highways used for exports", str(stats.exports.size()), "exact")
	_row("Peak vehicles", str(peak), "± 15%")
	_row("95% of drivers at work (hour)", "%.2f" % everyone_at_work, "± 1 h")
	_row("Commuters who reached work", str(commutes.size()), "± 5")
	_row("Median commute (× ideal)", "%.2f" % median, "± 0.3")
	for mode in shares:
		_row("Mode share: %s" % Citizen.Mode.keys()[mode].to_lower(), "%.1f%%" % (shares[mode] * 100.0), "± 5 points")
	_row("Conflict-zone violations", str(violations), "0")
	_row("Smallest queue gap (px)", "%.2f" % worst_gap, "≥ %.2f" % (VehicleSystem.MIN_GAP - 0.01))
	_row("Most vehicles stuck 30 s+", str(most_stuck), "0 if Godot is 0")
	_out("")
	_dispose(sim)


# --- Bottleneck -------------------------------------------------------------------------

func _bottleneck_mornings() -> void:
	_out("## Bottleneck mornings")
	_out("")
	_out("`test_bottleneck.gd`'s `_morning_for_seed`: the citizens' RNG seeded with the given seed before `build_bottleneck` (origin %s), run until every commuter has reached work (max 300 s). *Driving* = total time commuters spent driving to work; *took bypass* = commuters who drove on a bypass road." % BOTTLENECK_ORIGIN)
	_out("")
	_out("| Seed | Bypass | Everyone at work (s) | Driving (s) | Took bypass | Band |")
	_out("|---|---|---|---|---|---|")
	for seed in [1, 2, 3]:
		for bypass in [false, true]:
			var result := _morning(seed, bypass)
			_out("| %d | %s | %.1f | %.0f | %d | time ± 15%%, driving ± 15%% |" % [
				seed, "yes" if bypass else "no", result.time, result.driving, result.took_bypass])
	_out("")


func _morning(seed: int, bypass: bool) -> Dictionary:
	var sim := Simulation.new()
	sim.citizens._rng.seed = seed
	var info := Scenarios.build_bottleneck(sim, BOTTLENECK_ORIGIN)
	var before := sim.roads.segments.keys()
	if bypass:
		Scenarios.build_bypass(sim, info)
	var junction_y: float = info.junction.y
	var watched := sim.roads.segments.keys().filter(func(id: int) -> bool:
		if before.has(id):
			return false
		var segment: RoadSegment = sim.roads.segments[id]
		return not (is_equal_approx(sim.roads.nodes[segment.start_node_id].position.y, junction_y)
			and is_equal_approx(sim.roads.nodes[segment.end_node_id].position.y, junction_y)))
	var driving := { "total": 0.0 }
	sim.vehicles.vehicle_arrived.connect(func(vehicle: Vehicle) -> void:
		var citizen: Citizen = sim.citizens.citizens.get(vehicle.owner_id)
		if citizen != null and vehicle.destination_lot_id == citizen.work_lot_id:
			driving.total += vehicle.trip_end_time - vehicle.trip_start_time)
	var reached := {}
	var seen := {}
	var time := INF
	for tick in Simulation.TICK_RATE * 300:
		sim.step()
		for vehicle: Vehicle in sim.vehicles.vehicles.values():
			var commuter: Citizen = sim.citizens.citizens.get(vehicle.owner_id)
			if commuter != null and commuter.state == Citizen.State.TO_WORK and watched.has(vehicle.lane.segment_id):
				seen[vehicle.owner_id] = true
		for citizen: Citizen in sim.citizens.citizens.values():
			if citizen.state == Citizen.State.AT_WORK:
				reached[citizen.id] = true
		if reached.size() == sim.citizens.citizens.size():
			time = sim.get_sim_time()
			break
	var result := { "time": time, "driving": driving.total, "took_bypass": seen.size() }
	_dispose(sim)
	return result


func _bottleneck_flow() -> void:
	_out("## Bottleneck, Flow after the morning")
	_out("")
	_out("Default seeds, %d ticks (120 s, 10 game hours) from the start of the scenario, by which time every commuter has reached work." % MORNING_TICKS)
	_out("")
	_out("| Setup | Flow % | Vehicle arrivals | Mode shares | Band |")
	_out("|---|---|---|---|---|")
	for setup in ["plain", "bypass", "bus line (curbside)", "bus lanes"]:
		var sim := Simulation.new()
		var info := Scenarios.build_bottleneck(sim, BOTTLENECK_ORIGIN)
		match setup:
			"bypass":
				Scenarios.build_bypass(sim, info)
			"bus line (curbside)":
				Scenarios.add_bottleneck_bus_line(sim, BOTTLENECK_ORIGIN)
			"bus lanes":
				Scenarios.add_bottleneck_bus_lanes(sim, BOTTLENECK_ORIGIN)
		var arrivals := { "count": 0 }
		sim.vehicles.vehicle_arrived.connect(func(_v: Vehicle) -> void: arrivals.count += 1)
		for tick in MORNING_TICKS:
			sim.step()
		var shares := sim.flow.get_mode_shares(sim.get_sim_time())
		var share_text: PackedStringArray = []
		for mode in shares:
			share_text.append("%s %.0f%%" % [Citizen.Mode.keys()[mode].to_lower(), shares[mode] * 100.0])
		_out("| %s | %.1f | %d | %s | Flow ± 5 points, arrivals ± 10%% |" % [
			setup, sim.flow.get_flow_percent(sim.get_sim_time()), arrivals.count, ", ".join(share_text)])
		_dispose(sim)
	_out("")


# --- Stress grids -----------------------------------------------------------------------

func _stress_grids() -> void:
	_out("## Stress grids")
	_out("")
	_out("Random trips (`StressTraffic`, seed 0) on a 5 × 5 grid of roads 3 cells apart, safety checks on every tick. The first two rows are `test_traffic.gd`'s `_stress_grid` (local streets, 250 vehicles, 120 s, every junction of 3+ roads under the control); the others `test_road_types.gd`'s `_stress_grid` (even / odd rows and columns of the two road types, one-way ones alternating direction, 200 vehicles, 90 s).")
	_out("")
	_out("| Grid | Vehicles at end | Arrivals | Stuck 30 s+ | Smallest gap | Violations | Band |")
	_out("|---|---|---|---|---|---|---|")
	for control in [RoadNode.JunctionControl.NONE, RoadNode.JunctionControl.TRAFFIC_LIGHT]:
		var sim := Simulation.new()
		for i in 5:
			var at := 1 + i * 3
			_road(sim, Vector2(1, at), Vector2(13, at), &"local_2lane")
			_road(sim, Vector2(at, 1), Vector2(at, 13), &"local_2lane")
		for node: RoadNode in sim.roads.nodes.values():
			if node.get_degree() >= 3:
				sim.roads.set_node_control(node.id, control)
		_stress_row(sim, "local, %s, 250" % RoadNode.JunctionControl.keys()[control].to_lower(), 250, 120)
	for mix: Array in [
		[&"avenue_4lane", &"local_2lane"],
		[&"local_2lane", &"one_way_2lane", &"one_way_1lane"],
		[&"avenue_4lane", &"one_way_2lane", &"one_way_1lane"],
	]:
		var sim := Simulation.new()
		var odd_h: StringName = mix[1]
		var odd_v: StringName = mix[2] if mix.size() > 2 else mix[1]
		for i in 5:
			var at := 1 + i * 3
			var horizontal: StringName = mix[0] if i % 2 == 0 else odd_h
			var vertical: StringName = mix[0] if i % 2 == 0 else odd_v
			if i % 4 == 1:
				_road(sim, Vector2(13, at), Vector2(1, at), horizontal)
				_road(sim, Vector2(at, 13), Vector2(at, 1), vertical)
			else:
				_road(sim, Vector2(1, at), Vector2(13, at), horizontal)
				_road(sim, Vector2(at, 1), Vector2(at, 13), vertical)
		_stress_row(sim, " / ".join(mix) + ", 200", 200, 90)
	_out("")
	_out("Bands: arrivals ± 15%, vehicles at end ± 15%, stuck 0 where Godot has 0, smallest gap ≥ 3.99, violations 0.")


func _road(sim: Simulation, from: Vector2, to: Vector2, type_id: StringName) -> void:
	sim.roads.add_road(from * G, to * G, RoadType.load_by_id(type_id))


func _stress_row(sim: Simulation, label: String, target: int, seconds: int) -> void:
	sim.stress.target_count = target
	var arrived := { "count": 0 }
	sim.vehicles.vehicle_arrived.connect(func(_v: Vehicle) -> void: arrived.count += 1)
	var violations := 0
	var worst_gap := INF
	for tick in Simulation.TICK_RATE * seconds:
		sim.step()
		violations += Checks.connector_conflict_violations(sim)
		worst_gap = minf(worst_gap, Checks.min_queue_gap(sim))
	_out("| %s | %d | %d | %d | %.2f | %d | see below |" % [
		label, sim.vehicles.vehicles.size(), arrived.count, sim.vehicles.get_stuck_count(30.0), worst_gap, violations])
	_dispose(sim)


## Disconnects this script's lambdas (they hold `sim`, a reference cycle that would leak
## it), then disposes of the simulation.
func _dispose(sim: Simulation) -> void:
	for emitter: Object in [sim.vehicles, sim.citizens]:
		for signal_info in emitter.get_signal_list():
			for connection in emitter.get_signal_connection_list(signal_info.name):
				if connection.callable.is_custom():
					emitter.disconnect(signal_info.name, connection.callable)
	sim.dispose()
