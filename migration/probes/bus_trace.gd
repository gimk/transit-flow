extends SceneTree
## Bus traces for M6.2: buses running their lines (bays and curbside stops, a stress
## load for cars to pass them), riders waiting at stops, road edits and a stop removed
## under them. Run from the project folder:
##   /Applications/Godot.app/Contents/MacOS/Godot --headless --script res://migration/probes/bus_trace.gd
## Writes migration/golden/bus_trace.json:
## { "<scenario>": {
##     "events": [[tick, kind, id, a, b]]:
##       "a" / "f" / "g": a vehicle arrived / failed / failed after giving up (id: vehicle),
##       "s": a bus changed state (id: vehicle; a: stop index * 4 + dwelling * 2 + in_bay),
##       "r": a rider got off (id: citizen; a, b: the door x, y times 10, rounded),
##       "x": a rider was stranded (id: citizen),
##     "end": [[vehicle id, lane id, distance, speed]] (rounded to 1e-3),
##     "waiting": vehicles still in driveways,
##     "buses": [[vehicle id, line id, stop index, last index (-1: none), riders, dwell_until]],
##     "lines": [[id, bus count, [segment times]]] (1e9: unreachable),
##     "stops": [[id, lane id (-1: none), waiting, boarded, bay]] },
##   "plans": [{ "from", "to", "max_walk", "weight", "cost" (null: no plan),
##     "legs": [[1 for a bus leg (0 for a walk), line id, from stop id, to stop id]] }]
##     (`plan_ride` on two crossing lines, after a minute of buses running) }

const G := 64.0


func _init() -> void:
	var result := {}
	result["bays"] = _bays()
	result["grid"] = _grid()
	result["plans"] = _plans()
	var file := FileAccess.open("res://migration/golden/bus_trace.json", FileAccess.WRITE)
	file.store_string(JSON.stringify(result, "", false))
	print("events: bays %d, grid %d; plans %d" % [result.bays.events.size(), result.grid.events.size(), result.plans.plans.size()])
	quit()


## One street with a bay and a curbside stop each way, riders turning up at every stop,
## 40 cars driving about: they pass the buses in the bays and queue behind the others.
func _bays() -> Dictionary:
	var sim := Simulation.new()
	_road(sim, Vector2(0, 4), Vector2(24, 4), &"local_2lane")
	var stops: Array[int] = [
		sim.transit.add_stop(Vector2(6, 4) * G + Vector2(0, 10), true),
		sim.transit.add_stop(Vector2(18, 4) * G + Vector2(0, 10), false),
		sim.transit.add_stop(Vector2(18, 4) * G + Vector2(0, -10), true),
		sim.transit.add_stop(Vector2(6, 4) * G + Vector2(0, -10), false)]
	var line_id := sim.transit.add_line(stops)
	for i in 30:
		sim.transit.wait_for_bus(1000 + i, line_id, stops[0], stops[2])
	sim.stress.target_count = 40
	var trace := _record(sim)
	for tick in Simulation.TICK_RATE * 90:
		trace.tick = tick
		if tick % 60 == 0:
			var k: int = tick / 60
			sim.transit.wait_for_bus(2000 + k, line_id, stops[k % 4], stops[(k + 2) % 4])
		sim.step()
		_poll(sim, trace)
	return _finish(sim, trace)


## A street grid with two lines, 80 cars; a street is added at 30 s, the west end becomes
## an avenue at 60 s, a stop is removed at 75 s and a block of the second line at 90 s.
func _grid() -> Dictionary:
	var sim := Simulation.new()
	for y in [2, 4, 6]:
		_road(sim, Vector2(2, y), Vector2(22, y), &"local_2lane")
	for x in [2, 8, 16, 22]:
		_road(sim, Vector2(x, 2), Vector2(x, 6), &"local_2lane")
	var a: Array[int] = []
	for x in [5, 12, 19]:
		a.append(sim.transit.add_stop(Vector2(x, 4) * G + Vector2(0, 10)))
	for x in [19, 12, 5]:
		a.append(sim.transit.add_stop(Vector2(x, 4) * G + Vector2(0, -10)))
	var b: Array[int] = []
	for x in [5, 19]:
		b.append(sim.transit.add_stop(Vector2(x, 6) * G + Vector2(0, 10), true))
	for x in [19, 5]:
		b.append(sim.transit.add_stop(Vector2(x, 6) * G + Vector2(0, -10), true))
	var line_a := sim.transit.add_line(a)
	var line_b := sim.transit.add_line(b)
	sim.stress.target_count = 80
	var trace := _record(sim)
	for tick in Simulation.TICK_RATE * 120:
		trace.tick = tick
		if tick % 40 == 0:
			var k: int = tick / 40
			sim.transit.wait_for_bus(3000 + k, line_a, a[k % 6], a[(k + 3) % 6])
			sim.transit.wait_for_bus(5000 + k, line_b, b[k % 4], b[(k + 2) % 4])
		if tick == 600:
			_road(sim, Vector2(14, 2), Vector2(14, 6), &"local_2lane")
		if tick == 1200:
			sim.roads.set_segment_type(sim.roads.find_segment_at(Vector2(5, 4) * G, 4.0),
				RoadType.load_by_id(&"avenue_4lane"))
		if tick == 1500:
			sim.transit.remove_stop(a[1])
		if tick == 1800:
			sim.roads.remove_segment(sim.roads.find_segment_at(Vector2(11, 6) * G, 4.0))
		sim.step()
		_poll(sim, trace)
	return _finish(sim, trace)


## Two lines crossing at a shared stop, a minute of buses (so the timings are measured),
## then rides planned between random points.
func _plans() -> Dictionary:
	var sim := Simulation.new()
	_road(sim, Vector2(0, 4), Vector2(24, 4), &"local_2lane")
	_road(sim, Vector2(12, 4), Vector2(12, 20), &"local_2lane")
	var shared := sim.transit.add_stop(Vector2(9, 4) * G + Vector2(0, 10))
	sim.transit.add_line([sim.transit.add_stop(Vector2(2, 4) * G + Vector2(0, 10)), shared,
		sim.transit.add_stop(Vector2(9, 4) * G + Vector2(0, -10)), sim.transit.add_stop(Vector2(2, 4) * G + Vector2(0, -10))])
	sim.transit.add_line([shared, sim.transit.add_stop(Vector2(12, 7) * G + Vector2(-10, 0)),
		sim.transit.add_stop(Vector2(12, 18) * G + Vector2(-10, 0)), sim.transit.add_stop(Vector2(12, 18) * G + Vector2(10, 0))])
	sim.transit.add_line([sim.transit.add_stop(Vector2(20, 4) * G + Vector2(0, 10)),
		sim.transit.add_stop(Vector2(15, 4) * G + Vector2(0, -10))])
	for tick in Simulation.TICK_RATE * 60:
		sim.step()
	var rng := RandomNumberGenerator.new()
	rng.seed = 7
	var plans := []
	for i in 200:
		var from := _near_a_street(rng)
		var to := _near_a_street(rng)
		var max_walk := rng.randf_range(2.0, 6.0) * G
		var weight := rng.randf_range(1.0, 2.5)
		var plan := sim.transit.plan_ride(from, to, max_walk, weight)
		var legs := []
		for leg: Dictionary in plan.get("legs", []):
			legs.append([1, leg.line, leg.from, leg.to] if leg.mode == Citizen.Mode.BUS else [0, 0, 0, 0])
		plans.append({ "from": [from.x, from.y], "to": [to.x, to.y], "max_walk": max_walk, "weight": weight,
			"cost": plan.get("cost"), "legs": legs })
	return { "plans": plans }


## A random point within a cell of one of the two streets.
func _near_a_street(rng: RandomNumberGenerator) -> Vector2:
	if rng.randf() < 0.5:
		return Vector2(rng.randf_range(0.0, 24.0), rng.randf_range(3.0, 5.0)) * G
	return Vector2(rng.randf_range(11.0, 13.0), rng.randf_range(4.0, 19.0)) * G


func _record(sim: Simulation) -> Dictionary:
	var trace := { "tick": 0, "events": [], "state": {} }
	sim.vehicles.vehicle_arrived.connect(func(v: Vehicle) -> void:
		trace.events.append([trace.tick, "a", v.id, 0, 0]))
	sim.vehicles.vehicle_failed.connect(func(v: Vehicle) -> void:
		trace.events.append([trace.tick, "g" if v.gave_up else "f", v.id, 0, 0]))
	sim.transit.rider_arrived.connect(func(citizen_id: int, door: Vector2) -> void:
		trace.events.append([trace.tick, "r", citizen_id, roundi(door.x * 10.0), roundi(door.y * 10.0)]))
	sim.transit.rider_stranded.connect(func(citizen_id: int) -> void:
		trace.events.append([trace.tick, "x", citizen_id, 0, 0]))
	return trace


## Records a bus whenever its stop, dwelling or bay state changes.
func _poll(sim: Simulation, trace: Dictionary) -> void:
	for bus: TransitSystem.Bus in sim.transit.buses.values():
		var state := bus.stop_index * 4 + (2 if bus.vehicle.dwelling else 0) + (1 if bus.vehicle.in_bay else 0)
		if trace.state.get(bus.vehicle.id, -1) != state:
			trace.state[bus.vehicle.id] = state
			trace.events.append([trace.tick, "s", bus.vehicle.id, state, 0])


func _t(x: float) -> float:
	return 1e9 if x >= INF else snappedf(x, 0.001)


func _finish(sim: Simulation, trace: Dictionary) -> Dictionary:
	var end := []
	for v: Vehicle in sim.vehicles.vehicles.values():
		end.append([v.id, v.lane.id, snappedf(v.distance, 0.001), snappedf(v.speed, 0.001)])
	var buses := []
	for bus: TransitSystem.Bus in sim.transit.buses.values():
		buses.append([bus.vehicle.id, bus.line_id, bus.stop_index, bus.last_index, bus.riders.size(), _t(bus.dwell_until)])
	var lines := []
	for line: BusLine in sim.transit.lines.values():
		var times := []
		for t in line.segment_times:
			times.append(_t(t))
		lines.append([line.id, line.bus_count, times])
	var stops := []
	for stop: BusStop in sim.transit.stops.values():
		stops.append([stop.id, stop.lane_id, stop.waiting.size(), stop.boarded, stop.bay])
	return { "events": trace.events, "end": end, "waiting": sim.vehicles.get_waiting_departures().size(),
		"buses": buses, "lines": lines, "stops": stops }


func _road(sim: Simulation, from: Vector2, to: Vector2, type_id: StringName) -> void:
	sim.roads.add_road(from * G, to * G, RoadType.load_by_id(type_id))
