extends SceneTree
## Dumps reference data from the Godot build for the Rust port's tests (migration work
## package M0.3). Run from the project folder:
##   /Applications/Godot.app/Contents/MacOS/Godot --headless --script res://migration/export_golden.gd
## Writes migration/golden/<name>.json. Everything here is RNG-free: road geometry,
## junctions, plots, the walk network, transit stops and routes.
##
## Layout files (bottleneck, bottleneck_bypass, bottleneck_bus_lanes, town, junctions, grid):
## {
##   "save": Simulation.to_dict() of the layout (the input: Rust loads it with from_dict),
##   "roads": network dump of the *reloaded* save (see below), so ids are the ones
##            from_dict assigns,
##   "lots": [{ id, cell, type, plot: [x, y, w, h], access: [{ lane_id, distance, ... }] }],
##   "walk": { "points": [{ id, position, connections: [ids] }],
##             "crosswalk_edges": { "a,b": [node id, segment id] },
##             "sidewalks": { "a,b": [segment id, side] },
##             "paths": [{ from, to, length, points, crossings }] },
##   "transit": { "stops": [{ id, position, direction, lane_id, distance, bay }],
##                "lines": [{ id, stop_ids, color, path, free_flow_times, routes }],
##                "mid_block": { "a,b": { ... } } },
##   "routes": [{ from_lane, from_distance, to_lane, to_distance, bus, route }],
##   "lot_routes": [{ from_lot, to_lot, route }]
## }
## A network dump is { "nodes": [...], "segments": [...], "lanes": [...] }:
## - node: id, position, elevation_layer, control, signal_phase_count, segment_ids,
##   connector_lane_ids;
## - segment: id, start_node_id, end_node_id, road_type, elevation_layer, lane_ids,
##   polyline, length, centerline (curve, below);
## - lane: id, segment_id, from_node_id, to_node_id, index_from_center, turn,
##   speed_limit, bus_only, feeder_lane_id, signal_phase, is_straight, length,
##   outgoing_lane_ids, conflicting_lane_ids, conflict_zones { "id": [from, to] },
##   curve, and baked samples (below);
## - curve: { "points": [[x, y]], "in": [[x, y]], "out": [[x, y]], "bake_interval",
##   "baked_points": [[x, y]], "baked_length",
##   "samples": [[offset, x, y, rotation]] } with sample_baked_with_rotation at 0, 1/8 ...
##   and the full length.
## A route is { lane_ids, start_distance, end_distance, travel_time, free_flow_time }, or
## null.
##
## edits.json: a sequence of RoadNetwork commands on one network, each followed by the
## network dump: [{ "command", "args", "result", "changed": [node ids, segment ids],
## "roads": network dump }]. Road types by id, positions as [x, y].
##
## Vectors are [x, y]; dictionary keys that are Vector2i are "x,y"; enums are ints.

const OUT_DIR := "res://migration/golden/"
const G := 64.0
const ROUTE_SAMPLES := 80
const BUS_ROUTE_SAMPLES := 12
const LOT_ROUTE_SAMPLES := 20
const WALK_SAMPLES := 12


func _initialize() -> void:
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path(OUT_DIR))
	var bottleneck_origin := Vector2i(30, 30)  # As in test_bottleneck.gd.

	var sim := Simulation.new()
	Scenarios.build_bottleneck(sim, bottleneck_origin)
	_export_layout("bottleneck", sim)

	sim = Simulation.new()
	Scenarios.build_bypass(sim, Scenarios.build_bottleneck(sim, bottleneck_origin))
	_export_layout("bottleneck_bypass", sim)

	sim = Simulation.new()
	Scenarios.build_bottleneck(sim, bottleneck_origin)
	Scenarios.add_bottleneck_bus_lanes(sim, bottleneck_origin)
	_export_layout("bottleneck_bus_lanes", sim)

	sim = Simulation.new()
	Scenarios.build_town(sim)
	_export_layout("town", sim)

	sim = Simulation.new()
	_build_junctions(sim)
	_export_layout("junctions", sim)

	# A plain 6 x 6 grid of local streets, two cells apart: many equally quick routes, so
	# it tests that A* breaks ties as Godot does.
	sim = Simulation.new()
	for i in 6:
		sim.roads.add_road(Vector2(1, 1 + i * 2) * G, Vector2(11, 1 + i * 2) * G, RoadType.load_by_id(&"local_2lane"))
		sim.roads.add_road(Vector2(1 + i * 2, 1) * G, Vector2(1 + i * 2, 11) * G, RoadType.load_by_id(&"local_2lane"))
	_export_layout("grid", sim)

	_export_edits()
	quit()


## Every kind of junction the Town doesn't have: avenue x local with protected lefts, a
## three-lane one-way crossing an avenue, bus lanes crossing a one-way pair, a curve
## meeting a straight road, dead ends.
func _build_junctions(sim: Simulation) -> void:
	var t := func(id: StringName) -> RoadType: return RoadType.load_by_id(id)
	var r := sim.roads
	r.add_road(Vector2(0, 8) * G, Vector2(24, 8) * G, t.call(&"avenue_4lane"))
	r.add_road(Vector2(5, 2) * G, Vector2(5, 14) * G, t.call(&"local_2lane"))
	r.set_node_control(r.find_node_at(Vector2(5, 8) * G), RoadNode.JunctionControl.TRAFFIC_LIGHT_PROTECTED_LEFT)
	r.add_road(Vector2(11, 14) * G, Vector2(11, 2) * G, t.call(&"one_way_3lane"))
	r.add_road(Vector2(0, 16) * G, Vector2(24, 16) * G, t.call(&"avenue_4lane_bus"))
	r.add_road(Vector2(17, 2) * G, Vector2(17, 22) * G, t.call(&"one_way_2lane"))
	r.add_road(Vector2(21, 22) * G, Vector2(21, 2) * G, t.call(&"one_way_1lane"))
	r.set_node_control(r.find_node_at(Vector2(17, 16) * G), RoadNode.JunctionControl.TRAFFIC_LIGHT)
	r.add_curved_road(Vector2(2, 18) * G, Vector2(2, 24) * G, Vector2(8, 24) * G, t.call(&"local_2lane"))
	r.add_road(Vector2(8, 24) * G, Vector2(8, 16) * G, t.call(&"local_2lane"))
	for x in [3, 8, 14]:
		sim.lots.add_lot(Vector2i(x, 7), Lot.Type.COMMERCIAL)
		sim.lots.add_lot(Vector2i(x, 8), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(19, 15), Lot.Type.INDUSTRIAL)
	for x in [3, 13]:
		sim.transit.add_stop(Vector2(x, 16) * G + Vector2(0, 10), true)
		sim.transit.add_stop(Vector2(x, 16) * G + Vector2(0, -10))
	var stops: Array[int] = []
	stops.assign(sim.transit.stops.keys())
	sim.transit.add_line(stops)


func _export_layout(name: String, built: Simulation) -> void:
	var save := built.to_dict()
	built.dispose()
	# Dump the reloaded save: Rust starts from the same save, so the ids must be the ones
	# from_dict assigns.
	var sim := Simulation.from_dict(JSON.parse_string(JSON.stringify(save, "", false, true)))
	var data := {
		"save": save,
		"roads": _dump_network(sim.roads),
		"lots": _dump_lots(sim),
		"walk": _dump_walk(sim),
		"transit": _dump_transit(sim),
		"routes": _sample_routes(sim),
		"lot_routes": _sample_lot_routes(sim),
	}
	_write(name, data)
	sim.dispose()


func _export_edits() -> void:
	var roads := RoadNetwork.new()
	var steps: Array = []
	var last_change := []
	var record := func(node_ids: Array, segment_ids: Array) -> void:
		last_change.clear()
		last_change.append(node_ids.duplicate())
		last_change.append(segment_ids.duplicate())
	roads.changed.connect(record)
	var t := func(id: StringName) -> RoadType: return RoadType.load_by_id(id)
	# Each entry: [command, args (as saved), callable]
	var commands: Array = [
		["add_road", [[0, 4], [12, 4], "local_2lane"], func() -> Variant: return roads.add_road(Vector2(0, 4) * G, Vector2(12, 4) * G, t.call(&"local_2lane"))],
		["add_road", [[4, 0], [4, 10], "local_2lane"], func() -> Variant: return roads.add_road(Vector2(4, 0) * G, Vector2(4, 10) * G, t.call(&"local_2lane"))],
		["add_road", [[8, 4], [8, 10], "local_2lane"], func() -> Variant: return roads.add_road(Vector2(8, 4) * G, Vector2(8, 10) * G, t.call(&"local_2lane"))],
		["add_road_pieces", [[0, 10], [12, 10], "local_2lane"], func() -> Variant: return roads.add_road_pieces(Vector2(0, 10) * G, Vector2(12, 10) * G, t.call(&"local_2lane"))],
		["add_road_pieces", [[2, 4], [10, 4], "avenue_4lane"], func() -> Variant: return roads.add_road_pieces(Vector2(2, 4) * G, Vector2(10, 4) * G, t.call(&"avenue_4lane"))],
		["add_road_pieces", [[12, 0], [12, 12], "one_way_2lane"], func() -> Variant: return roads.add_road_pieces(Vector2(12, 0) * G, Vector2(12, 12) * G, t.call(&"one_way_2lane"))],
		["add_road_pieces", [[12, 12], [12, 0], "one_way_2lane"], func() -> Variant: return roads.add_road_pieces(Vector2(12, 12) * G, Vector2(12, 0) * G, t.call(&"one_way_2lane"))],
		["set_node_control", [[4, 10], 1], func() -> Variant: return roads.set_node_control(roads.find_node_at(Vector2(4, 10) * G), RoadNode.JunctionControl.TRAFFIC_LIGHT)],
		["add_curved_road", [[0, 14], [6, 14], [6, 8], "local_2lane"], func() -> Variant: return roads.add_curved_road(Vector2(0, 14) * G, Vector2(6, 14) * G, Vector2(6, 8) * G, t.call(&"local_2lane"))],
		["add_road_pieces", [[0, 12], [10, 12], "local_2lane"], func() -> Variant: return roads.add_road_pieces(Vector2(0, 12) * G, Vector2(10, 12) * G, t.call(&"local_2lane"))],
		["remove_segment_at", [[8, 7]], func() -> Variant: return roads.remove_segment(roads.find_segment_at(Vector2(8, 7) * G, 4.0))],
		["remove_segment_at", [[4, 2]], func() -> Variant: return roads.remove_segment(roads.find_segment_at(Vector2(4, 2) * G, 4.0))],
		["add_road", [[20, 0], [20, 1], "local_2lane"], func() -> Variant: return roads.add_road(Vector2(20, 0) * G, Vector2(20, 1) * G, t.call(&"local_2lane"))],
		["add_road", [[9, 0], [9, 12], "avenue_4lane"], func() -> Variant: return roads.add_road(Vector2(9, 0) * G, Vector2(9, 12) * G, t.call(&"avenue_4lane"))],
	]
	for command: Array in commands:
		last_change.clear()
		var result: Variant = command[2].call()
		steps.append({
			"command": command[0],
			"args": command[1],
			"result": _j(result),
			"changed": _j(last_change.duplicate()),
			"roads": _dump_network(roads),
		})
	_write("edits", steps)
	roads.dispose()


func _dump_network(roads: RoadNetwork) -> Dictionary:
	var nodes: Array = []
	for node: RoadNode in roads.nodes.values():
		nodes.append({
			"id": node.id, "position": _j(node.position), "elevation_layer": node.elevation_layer,
			"control": node.control, "signal_phase_count": node.signal_phase_count,
			"segment_ids": _j(node.segment_ids), "connector_lane_ids": _j(node.connector_lane_ids),
		})
	var segments: Array = []
	for segment: RoadSegment in roads.segments.values():
		segments.append({
			"id": segment.id, "start_node_id": segment.start_node_id, "end_node_id": segment.end_node_id,
			"road_type": String(segment.road_type.id), "elevation_layer": segment.elevation_layer,
			"lane_ids": _j(segment.lane_ids), "polyline": _j(segment.get_polyline()),
			"length": segment.get_length(), "centerline": _dump_curve(segment.centerline),
		})
	var lanes: Array = []
	for lane: Lane in roads.lanes.values():
		var zones := {}
		for other: int in lane.conflict_zones:
			zones[str(other)] = _j(lane.conflict_zones[other])
		lanes.append({
			"id": lane.id, "segment_id": lane.segment_id, "from_node_id": lane.from_node_id,
			"to_node_id": lane.to_node_id, "index_from_center": lane.index_from_center,
			"turn": lane.turn, "speed_limit": lane.speed_limit, "bus_only": lane.bus_only,
			"feeder_lane_id": lane.feeder_lane_id, "signal_phase": lane.signal_phase,
			"is_straight": lane.is_straight, "length": lane.length,
			"outgoing_lane_ids": _j(lane.outgoing_lane_ids),
			"conflicting_lane_ids": _j(lane.conflicting_lane_ids), "conflict_zones": zones,
			"curve": _dump_curve(lane.curve),
		})
	return { "nodes": nodes, "segments": segments, "lanes": lanes }


func _dump_curve(curve: Curve2D) -> Dictionary:
	var points: Array = []
	var ins: Array = []
	var outs: Array = []
	for i in curve.point_count:
		points.append(_j(curve.get_point_position(i)))
		ins.append(_j(curve.get_point_in(i)))
		outs.append(_j(curve.get_point_out(i)))
	var length := curve.get_baked_length()
	var samples: Array = []
	for k in 9:
		var offset := length * k / 8.0
		var pose := curve.sample_baked_with_rotation(offset)
		samples.append([offset, pose.origin.x, pose.origin.y, pose.get_rotation()])
	return {
		"points": points, "in": ins, "out": outs, "bake_interval": curve.bake_interval,
		"baked_points": _j(curve.get_baked_points()), "baked_length": length, "samples": samples,
	}


func _dump_lots(sim: Simulation) -> Array:
	var out: Array = []
	for lot: Lot in sim.lots.lots.values():
		out.append({
			"id": lot.id, "cell": _j(lot.cell), "type": lot.type, "plot": _j(lot.plot),
			"access": _j(sim.lots.get_access_points(lot.id)),
		})
	return out


func _dump_walk(sim: Simulation) -> Dictionary:
	var network := sim.pedestrians.network
	network._ensure_built()
	var points: Array = []
	for id in network.graph.get_point_ids():
		points.append({
			"id": id, "position": _j(network.graph.get_point_position(id)),
			"connections": _j(network.graph.get_point_connections(id)),
		})
	var paths: Array = []
	var rng := RandomNumberGenerator.new()
	rng.seed = 5
	var area := _bounds(sim.roads)
	for i in WALK_SAMPLES:
		var from := Vector2(rng.randf_range(area.position.x, area.end.x), rng.randf_range(area.position.y, area.end.y))
		var to := Vector2(rng.randf_range(area.position.x, area.end.x), rng.randf_range(area.position.y, area.end.y))
		var path := network.find_path(from, to)
		paths.append({ "from": _j(from), "to": _j(to), "path": _j(path) })
	return {
		"points": points, "crosswalk_edges": _j(network.crosswalk_edges),
		"sidewalks": _j(network.sidewalks), "paths": paths,
	}


func _dump_transit(sim: Simulation) -> Dictionary:
	var stops: Array = []
	for stop: BusStop in sim.transit.stops.values():
		stops.append({
			"id": stop.id, "position": _j(stop.position), "direction": _j(stop.direction),
			"lane_id": stop.lane_id, "distance": stop.distance, "bay": stop.bay,
		})
	var lines: Array = []
	for line: BusLine in sim.transit.lines.values():
		lines.append({
			"id": line.id, "stop_ids": _j(line.stop_ids), "color": _j(line.color),
			"path": _j(line.path), "free_flow_times": _j(line.free_flow_times),
			"routes": _j(line.routes),
		})
	return { "stops": stops, "lines": lines, "mid_block": _j(sim.crosswalks.mid_block) }


## Random road-lane pairs (a fixed seed, so the list is the same every run).
func _sample_routes(sim: Simulation) -> Array:
	var road_lanes: Array[Lane] = []
	var has_bus_lanes := false
	for lane: Lane in sim.roads.lanes.values():
		if not lane.is_connector():
			road_lanes.append(lane)
			has_bus_lanes = has_bus_lanes or lane.bus_only
	var rng := RandomNumberGenerator.new()
	rng.seed = 99
	var out: Array = []
	var samples := ROUTE_SAMPLES + (BUS_ROUTE_SAMPLES if has_bus_lanes else 0)
	for i in samples:
		var bus := i >= ROUTE_SAMPLES
		var from: Lane = road_lanes[rng.randi() % road_lanes.size()]
		var to: Lane = road_lanes[rng.randi() % road_lanes.size()]
		var from_distance := rng.randf() * from.length
		var to_distance := rng.randf() * to.length
		out.append({
			"from_lane": from.id, "from_distance": from_distance, "to_lane": to.id,
			"to_distance": to_distance, "bus": bus,
			"route": _route(sim, sim.router.find_route(from.id, from_distance, to.id, to_distance, bus)),
		})
	return out


func _sample_lot_routes(sim: Simulation) -> Array:
	var lot_ids: Array = sim.lots.lots.keys()
	if lot_ids.size() < 2:
		return []
	var rng := RandomNumberGenerator.new()
	rng.seed = 77
	var out: Array = []
	for i in LOT_ROUTE_SAMPLES:
		var from: int = lot_ids[rng.randi() % lot_ids.size()]
		var to: int = lot_ids[rng.randi() % lot_ids.size()]
		var route := sim.router.find_best_route(sim.lots.get_access_points(from), sim.lots.get_access_points(to))
		out.append({ "from_lot": from, "to_lot": to, "route": _route(sim, route) })
	return out


func _route(sim: Simulation, route: Route) -> Variant:
	if route == null:
		return null
	return {
		"lane_ids": _j(route.lane_ids), "start_distance": route.start_distance,
		"end_distance": route.end_distance, "travel_time": route.travel_time,
		"free_flow_time": sim.router.get_free_flow_time(route),
	}


func _bounds(roads: RoadNetwork) -> Rect2:
	var rect := Rect2()
	var first := true
	for node: RoadNode in roads.nodes.values():
		rect = Rect2(node.position, Vector2.ZERO) if first else rect.expand(node.position)
		first = false
	return rect


## Converts Godot values to JSON-friendly ones.
func _j(value: Variant) -> Variant:
	match typeof(value):
		TYPE_VECTOR2, TYPE_VECTOR2I:
			return [value.x, value.y]
		TYPE_RECT2:
			return [value.position.x, value.position.y, value.size.x, value.size.y]
		TYPE_COLOR:
			return [value.r, value.g, value.b, value.a]
		TYPE_PACKED_VECTOR2_ARRAY, TYPE_PACKED_FLOAT32_ARRAY, TYPE_PACKED_FLOAT64_ARRAY, \
				TYPE_PACKED_INT32_ARRAY, TYPE_PACKED_INT64_ARRAY, TYPE_ARRAY:
			var out: Array = []
			for item in value:
				out.append(_j(item))
			return out
		TYPE_DICTIONARY:
			var out := {}
			for key in value:
				var text: String = "%s,%s" % [key.x, key.y] if typeof(key) in [TYPE_VECTOR2, TYPE_VECTOR2I] else str(key)
				out[text] = _j(value[key])
			return out
		TYPE_OBJECT:
			if value is Route:
				return {
					"lane_ids": _j(value.lane_ids), "start_distance": value.start_distance,
					"end_distance": value.end_distance, "travel_time": value.travel_time,
				}
			return str(value)
		_:
			return value


func _write(name: String, data: Variant) -> void:
	var path := OUT_DIR + name + ".json"
	var file := FileAccess.open(path, FileAccess.WRITE)
	file.store_string(JSON.stringify(data, "", false, true))
	file.close()
	print("wrote %s (%d KB)" % [path, FileAccess.get_file_as_bytes(path).size() / 1024])
