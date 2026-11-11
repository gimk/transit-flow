extends SceneTree
## Town trace for M7.3: `Scenarios.build_town` living through a whole game day from 06:00
## (5760 ticks), as in `test_town_lives_through_a_whole_day`. Run from the project folder:
##   /Applications/Godot.app/Contents/MacOS/Godot --headless --script res://migration/probes/town_trace.gd
## Writes migration/golden/town_trace.json:
## { "events": [[tick, kind, id, a, b, c, d]]: same format as citizen_trace.gd, plus
##     "a": a vehicle arrived (id: vehicle; a: owner, b: Vehicle.Kind, c: destination lot id),
##   "flow": [[tick, flow * 1e6 (null: none), trip count, [[mode, share * 1e6]]]] every 240 ticks,
##   "series": [[tick, vehicles, pedestrians, waiting departures, riders, stuck 30 s+]] every 120 ticks,
##   "stats": { commutes, trucks, failed, exports, arrivals, peak, everyone_at_work_ms, median_ratio * 1e6,
##     violations, worst_gap * 1e6, most_stuck },
##   "setup": [citizens, jobs, lots, vehicles after the first step],
##   "citizens", "parking", "counts", "vehicles": as in citizen_trace.gd }

const Checks := preload("res://tests/traffic_checks.gd")
const DAY_TICKS := Simulation.TICK_RATE * SimClock.MINUTES_PER_DAY / int(SimClock.GAME_MINUTES_PER_SIM_SECOND)


func _init() -> void:
	var sim := Simulation.new()
	Scenarios.build_town(sim)
	var trace := _record(sim)
	var stats := { "trucks": 0, "failed": 0, "exports": {}, "arrivals": 0, "commutes": {} }
	sim.vehicles.vehicle_arrived.connect(func(v: Vehicle) -> void:
		stats.arrivals += 1
		if v.kind == Vehicle.Kind.TRUCK:
			stats.trucks += 1
			if v.destination_lot_id == -1:
				stats.exports[v.lane.to_node_id] = true)
	sim.citizens.trip_ended.connect(func(c: Citizen, completed: bool) -> void:
		if completed and c.state == Citizen.State.TO_WORK and not stats.commutes.has(c.id):
			stats.commutes[c.id] = (c.trip_end_time - c.trip_start_time) / c.trip_ideal_time)
	sim.vehicles.vehicle_failed.connect(func(_v: Vehicle) -> void: stats.failed += 1)
	var everyone_at_work := -1.0
	var violations := 0
	var worst_gap := INF
	var peak := 0
	var most_stuck := 0
	var setup := []
	var series := []
	for tick in DAY_TICKS:
		trace.tick = tick
		sim.step()
		_poll(sim, trace)
		violations += Checks.connector_conflict_violations(sim)
		worst_gap = minf(worst_gap, Checks.min_queue_gap(sim))
		peak = maxi(peak, sim.vehicles.vehicles.size())
		var drivers := sim.citizens.citizens.values().filter(func(c: Citizen) -> bool: return c.has_car)
		var at_work := drivers.filter(func(c: Citizen) -> bool: return c.state == Citizen.State.AT_WORK).size()
		if everyone_at_work < 0.0 and not drivers.is_empty() and at_work >= drivers.size() * 0.95:
			everyone_at_work = sim.clock.get_time_of_day_hours()
		var stuck := 0
		if tick % Simulation.TICK_RATE == 0:
			stuck = sim.vehicles.get_stuck_count(30.0)
			most_stuck = maxi(most_stuck, stuck)
		if tick == 0:
			setup = [sim.citizens.citizens.size(), sim.citizens.get_job_count(), sim.lots.lots.size(), sim.vehicles.vehicles.size()]
		if tick % 120 == 119:
			series.append([tick, sim.vehicles.vehicles.size(), sim.pedestrians.pedestrians.size(),
				sim.vehicles.get_waiting_departures().size(), sim.transit._riders.size(), sim.vehicles.get_stuck_count(30.0)])
		if tick % 240 == 239:
			var now := sim.get_sim_time()
			var flow := sim.flow.get_flow_percent(now)
			var shares := []
			var modes: Dictionary = sim.flow.get_mode_shares(now)
			var keys := modes.keys()
			keys.sort()
			for mode: int in keys:
				shares.append([mode, roundi(modes[mode] * 1e6)])
			trace.flow.append([tick, null if flow < 0.0 else roundi(flow * 1e6), sim.flow.get_trip_count(now), shares])

	var commutes: Array = stats.commutes.values()
	commutes.sort()
	var result := { "events": trace.events, "flow": trace.flow, "series": series, "setup": setup }
	result["stats"] = { "commutes": commutes.size(), "trucks": stats.trucks, "failed": stats.failed,
		"exports": stats.exports.size(), "arrivals": stats.arrivals, "peak": peak,
		"everyone_at_work_ms": roundi(everyone_at_work * 1000.0),
		"median_ratio": roundi(commutes[commutes.size() / 2] * 1e6), "violations": violations,
		"worst_gap": roundi(worst_gap * 1e6), "most_stuck": most_stuck }
	var citizens := []
	for c: Citizen in sim.citizens.citizens.values():
		citizens.append([c.id, c.state, c.work_lot_id, c.car_lot_id, int(c.has_car), c.last_commute_day,
			c.shopping_decided_day, int(c.going_shopping), roundi(c.shop_until * 1000.0), roundi(c.retry_at * 1000.0),
			roundi(c.leave_home_hour * 1e6), roundi(c.leave_work_hour * 1e6), roundi(c.car_preference * 1e6)])
	result["citizens"] = citizens
	var parking := []
	for lot_id: int in sim.lots.lots:
		if sim.citizens.parking.get_used(lot_id) > 0:
			parking.append([lot_id, sim.citizens.parking.get_used(lot_id)])
	result["parking"] = parking
	result["counts"] = [sim.citizens.citizens.size(), sim.citizens.get_job_count(), sim.citizens.get_employed_count(),
		sim.lots.lots.size(), sim.vehicles.vehicles.size(), sim.pedestrians.pedestrians.size(),
		sim.vehicles.get_waiting_departures().size(), sim.transit._riders.size()]
	var vehicles := []
	for v: Vehicle in sim.vehicles.vehicles.values():
		vehicles.append([v.id, v.lane.id, snappedf(v.distance, 0.001), snappedf(v.speed, 0.001)])
	result["vehicles"] = vehicles
	var file := FileAccess.open("res://migration/golden/town_trace.json", FileAccess.WRITE)
	file.store_string(JSON.stringify(result, "", false))
	var kinds := {}
	for e: Array in trace.events:
		kinds[e[1]] = kinds.get(e[1], 0) + 1
	print("events %d %s; stats %s; flow %s" % [trace.events.size(), kinds, result.stats, trace.flow.back()])
	quit()


func _record(sim: Simulation) -> Dictionary:
	var trace := { "tick": 0, "events": [], "flow": [], "state": {}, "start": {} }
	sim.vehicles.vehicle_arrived.connect(func(v: Vehicle) -> void:
		trace.events.append([trace.tick, "a", v.id, v.owner_id, v.kind, v.destination_lot_id, 0]))
	sim.vehicles.vehicle_failed.connect(func(v: Vehicle) -> void:
		trace.events.append([trace.tick, "g" if v.gave_up else "f", v.id, v.owner_id, 0, 0, 0]))
	sim.pedestrians.pedestrian_arrived.connect(func(p: Pedestrian) -> void:
		trace.events.append([trace.tick, "p", p.id, p.owner_id, 0, 0, 0]))
	sim.transit.rider_arrived.connect(func(citizen_id: int, door: Vector2) -> void:
		trace.events.append([trace.tick, "r", citizen_id, roundi(door.x * 10.0), roundi(door.y * 10.0), 0, 0]))
	sim.transit.rider_stranded.connect(func(citizen_id: int) -> void:
		trace.events.append([trace.tick, "x", citizen_id, 0, 0, 0, 0]))
	sim.citizens.trip_ended.connect(func(c: Citizen, completed: bool) -> void:
		trace.events.append([trace.tick, "t", c.id, int(completed), c.trip_mode, roundi(c.trip_ideal_time * 1000.0),
			roundi((c.trip_end_time - c.trip_start_time) * 1000.0)]))
	return trace


## Records a citizen whenever their state changes or a new trip started.
func _poll(sim: Simulation, trace: Dictionary) -> void:
	for c: Citizen in sim.citizens.citizens.values():
		if c.trip_start_time != trace.start.get(c.id, 0.0):
			trace.start[c.id] = c.trip_start_time
			trace.events.append([trace.tick, "b", c.id, c.trip_mode, c.trip_lot_id,
				-1 if c.trip_reference_time >= INF else roundi(c.trip_reference_time * 1000.0), 0])
		if trace.state.get(c.id, -1) != c.state:
			trace.state[c.id] = c.state
			trace.events.append([trace.tick, "c", c.id, c.state, c.trip_lot_id, 0, 0])
