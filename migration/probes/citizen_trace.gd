extends SceneTree
## Citizen trace for M7.1/M7.2: a small city (homes, shops, factories on zoned land, a
## highway with trucks, a bus line, parking) living through a day, with a road cut and
## re-laid, a shop removed and a rush of people leaving at once. Run from the project folder:
##   /Applications/Godot.app/Contents/MacOS/Godot --headless --script res://migration/probes/citizen_trace.gd
## Writes migration/golden/citizen_trace.json:
## { "events": [[tick, kind, id, a, b, c, d]]:
##     "a" / "f" / "g": a vehicle arrived / failed / gave up (id: vehicle; a: owner),
##     "p": a pedestrian arrived (id: pedestrian; a: owner),
##     "r": a rider got off (id: citizen; a, b: the door x, y times 10, rounded),
##     "x": a rider was stranded (id: citizen),
##     "t": a trip ended (id: citizen; a: completed; b: Citizen.Mode; c: ideal ms; d: took ms),
##     "c": a citizen changed state (id: citizen; a: state; b: trip lot id),
##     "b": a trip started (id: citizen; a: Citizen.Mode; b: trip lot id; c: reference ms, -1: INF),
##   "flow": [[tick, flow * 1e6 (null: none), trip count, [[mode, share * 1e6]]]] every 400 ticks,
##   "citizens": [[id, state, work lot, car lot, has car, last commute day, shopping decided day,
##     going shopping, shop_until ms, retry_at ms, leave home * 1e6, leave work * 1e6, car preference * 1e6]],
##   "parking": [[lot id, spaces used]], "counts": [citizens, jobs, employed, lots, vehicles, pedestrians,
##     waiting departures, riders], "vehicles": [[id, lane id, distance, speed]],
##   "plans": [[citizen id, has car, going home, mode (-1: none), legs, cost * 1e6]],
##   "sort": ids of 60 points sorted by distance to a centre with `sort_custom` (many ties) }

const G := 64.0
const TICKS := 6000


func _init() -> void:
	var sim := Simulation.new()
	Scenarios.add_highway(sim)
	var origin := Grid.world_to_cell(sim.externals.points[0]) + Vector2i(6, 0)
	var at := func(x: float, y: float) -> Vector2: return (Vector2(origin) + Vector2(x, y)) * G
	var local := RoadType.load_by_id(&"local_2lane")
	sim.roads.add_road(at.call(0, 0), at.call(16, 0), local)
	for x in [0, 4, 8, 12, 16]:
		sim.roads.add_road(at.call(x, -3), at.call(x, 3), local)
	sim.roads.add_road(at.call(0, -3), at.call(16, -3), local)
	sim.roads.add_road(at.call(0, 3), at.call(16, 3), local)
	sim.zones.paint(_cells(origin + Vector2i(0, -3), origin + Vector2i(7, 2)), Lot.Type.RESIDENTIAL)
	sim.zones.paint(_cells(origin + Vector2i(8, -3), origin + Vector2i(15, -1)), Lot.Type.COMMERCIAL)
	sim.zones.paint(_cells(origin + Vector2i(8, 0), origin + Vector2i(15, 2)), Lot.Type.INDUSTRIAL)
	# A bus line along the middle street: east on the south side, back on the north side.
	var stops: Array[int] = []
	for x in [1, 5, 9, 13]:
		stops.append(sim.transit.add_stop(at.call(x, 0) + Vector2(0, 10)))
	for x in [13, 9, 5, 1]:
		stops.append(sim.transit.add_stop(at.call(x, 0) + Vector2(0, -10)))
	var line_id := sim.transit.add_line(stops)

	var trace := _record(sim)
	for tick in TICKS:
		trace.tick = tick
		if tick == 2400:
			sim.roads.remove_segment(sim.roads.find_segment_at(at.call(8, 1.5), 4.0))
		if tick == 3000:
			sim.roads.add_road(at.call(8, 0), at.call(8, 3), local)
		if tick == 3300:
			sim.lots.remove_lot(sim.lots.get_lots_of_type(Lot.Type.COMMERCIAL)[0].id)
		if tick == 4000:
			sim.citizens.depart_everyone_now(10.0)
		sim.step()
		_poll(sim, trace)
		if tick % 400 == 399:
			var now := sim.get_sim_time()
			var flow := sim.flow.get_flow_percent(now)
			var shares := []
			var modes: Dictionary = sim.flow.get_mode_shares(now)
			var keys := modes.keys()
			keys.sort()
			for mode: int in keys:
				shares.append([mode, roundi(modes[mode] * 1e6)])
			trace.flow.append([tick, null if flow < 0.0 else roundi(flow * 1e6), sim.flow.get_trip_count(now), shares])

	var result := { "events": trace.events, "flow": trace.flow }
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
	var plans := []
	for c: Citizen in sim.citizens.citizens.values():
		if c.work_lot_id == -1:
			continue
		for has_car in [true, false]:
			for going_home in [false, true]:
				c.has_car = has_car
				var from := c.home_lot_id if not going_home else c.work_lot_id
				var to := c.work_lot_id if not going_home else c.home_lot_id
				var plan := sim.citizens.plan_trip(c, from, to, going_home)
				plans.append([c.id, int(has_car), int(going_home), plan.get("mode", -1), plan.get("legs", []).size(),
					null if plan.is_empty() else roundi(plan.cost * 1e6)])
	result["plans"] = plans
	result["sort"] = _sort()
	var file := FileAccess.open("res://migration/golden/citizen_trace.json", FileAccess.WRITE)
	file.store_string(JSON.stringify(result, "", false))
	var kinds := {}
	for e: Array in trace.events:
		kinds[e[1]] = kinds.get(e[1], 0) + 1
	print("events %d %s; citizens %d; line %d; flow %s" % [trace.events.size(), kinds, citizens.size(), line_id, trace.flow.back()])
	quit()


## 60 points on a 6x6 grid sorted by squared distance to the centre (3, 3): ties all over.
func _sort() -> Array:
	var rng := RandomNumberGenerator.new()
	rng.seed = 5
	var points := []
	for i in 60:
		points.append([i, Vector2(rng.randi() % 6, rng.randi() % 6)])
	var centre := Vector2(2.5, 2.5)
	points.sort_custom(func(a: Array, b: Array) -> bool:
		return centre.distance_squared_to(a[1]) < centre.distance_squared_to(b[1]))
	return points.map(func(p: Array) -> int: return p[0])


func _cells(from: Vector2i, to: Vector2i) -> Array[Vector2i]:
	var cells: Array[Vector2i] = []
	for x in range(from.x, to.x + 1):
		for y in range(from.y, to.y + 1):
			cells.append(Vector2i(x, y))
	return cells


func _record(sim: Simulation) -> Dictionary:
	var trace := { "tick": 0, "events": [], "flow": [], "state": {}, "start": {} }
	sim.vehicles.vehicle_arrived.connect(func(v: Vehicle) -> void:
		trace.events.append([trace.tick, "a", v.id, v.owner_id, 0, 0, 0]))
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
