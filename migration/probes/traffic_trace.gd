extends SceneTree
## Traffic traces for the R2 review gate: covers what the stress grids don't (road edits
## under traffic: removal, retyping, re-adding; driveways and highway gates with freight).
## Run from the project folder:
##   /Applications/Godot.app/Contents/MacOS/Godot --headless --script res://migration/probes/traffic_trace.gd
## Writes migration/golden/traffic_trace.json:
## { "<scenario>": { "events": [[tick, "a" | "f" | "g", vehicle id]] (arrived, failed,
##   failed after giving up), "end": [[id, lane id, distance, speed]] (rounded to 1e-3),
##   "waiting": number of vehicles still in driveways } }

const G := 64.0


func _init() -> void:
	var result := {}
	result["edits"] = _edits()
	result["freight"] = _freight()
	var file := FileAccess.open("res://migration/golden/traffic_trace.json", FileAccess.WRITE)
	file.store_string(JSON.stringify(result, "", false))
	print("events: edits %d, freight %d" % [result.edits.events.size(), result.freight.events.size()])
	quit()


## A 5 x 5 street grid, lights on every other junction, 250 random trips; at 30 s a block
## is removed, at 60 s another is retyped to an avenue, at 90 s the first comes back.
func _edits() -> Dictionary:
	var sim := Simulation.new()
	for i in 5:
		var at := 1 + i * 3
		_road(sim, Vector2(1, at), Vector2(13, at), &"local_2lane")
		_road(sim, Vector2(at, 1), Vector2(at, 13), &"local_2lane")
	var n := 0
	for node: RoadNode in sim.roads.nodes.values():
		if node.get_degree() >= 3:
			if n % 2 == 0:
				sim.roads.set_node_control(node.id, RoadNode.JunctionControl.TRAFFIC_LIGHT)
			n += 1
	sim.stress.target_count = 250
	var trace := _record(sim)
	for tick in Simulation.TICK_RATE * 120:
		trace.tick = tick
		if tick == 600:
			sim.roads.remove_segment(sim.roads.find_segment_at(Vector2(4, 5.5) * G, 4.0))
		if tick == 1200:
			sim.roads.set_segment_type(sim.roads.find_segment_at(Vector2(8.5, 7) * G, 4.0),
				RoadType.load_by_id(&"avenue_4lane"))
		if tick == 1800:
			_road(sim, Vector2(4, 4), Vector2(4, 7), &"local_2lane")
		sim.step()
	return _finish(sim, trace)


## `test_freight.gd`'s street: a whole game day of trucks, from 10:30.
func _freight() -> Dictionary:
	var sim := Simulation.new()
	Scenarios.add_highway(sim)
	var gate := sim.externals.points[0]
	sim.roads.add_road(gate + Vector2(6, 0) * G, gate + Vector2(20, 0) * G, RoadType.load_by_id(&"local_2lane"))
	var row := Grid.world_to_cell(gate).y
	var first_x := Grid.world_to_cell(gate).x
	for x in 5:
		sim.lots.add_lot(Vector2i(first_x + 8 + x, row), Lot.Type.INDUSTRIAL)
	for x in 3:
		sim.lots.add_lot(Vector2i(first_x + 15 + x, row), Lot.Type.COMMERCIAL)
	sim.clock.total_minutes = 10.5 * 60.0
	var trace := _record(sim)
	for tick in int(24.0 * 60.0 / SimClock.GAME_MINUTES_PER_SIM_SECOND * Simulation.TICK_RATE):
		trace.tick = tick
		sim.step()
	return _finish(sim, trace)


func _record(sim: Simulation) -> Dictionary:
	var trace := { "tick": 0, "events": [] }
	sim.vehicles.vehicle_arrived.connect(func(v: Vehicle) -> void:
		trace.events.append([trace.tick, "a", v.id]))
	sim.vehicles.vehicle_failed.connect(func(v: Vehicle) -> void:
		trace.events.append([trace.tick, "g" if v.gave_up else "f", v.id]))
	return trace


func _finish(sim: Simulation, trace: Dictionary) -> Dictionary:
	var end := []
	for v: Vehicle in sim.vehicles.vehicles.values():
		end.append([v.id, v.lane.id, snappedf(v.distance, 0.001), snappedf(v.speed, 0.001)])
	var result := { "events": trace.events, "end": end, "waiting": sim.vehicles.get_waiting_departures().size() }
	return result


func _road(sim: Simulation, from: Vector2, to: Vector2, type_id: StringName) -> void:
	sim.roads.add_road(from * G, to * G, RoadType.load_by_id(type_id))
