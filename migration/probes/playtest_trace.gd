extends Node
## R4 playtest trace: the playtest of docs/IMPLEMENTATION_PLAN.md played through the real
## main scene with synthetic input, as tests/integration/test_build_tools.gd does (main.tscn in
## a SubViewport, events pushed into it; the keys the Game autoload handles are sent to
## `Game._unhandled_input`, which the SubViewport doesn't reach). The sim is stepped by hand
## (`Game.set_speed(0)`, then `Game.simulation.step()`), so every run is the same.
## Run from the project folder:
##   /Applications/Godot.app/Contents/MacOS/Godot --headless res://migration/probes/playtest_trace.tscn
## (a scene, not `--script`: the autoloads (`Game`, `Events`) are not there when a script is compiled
## that way, and main.tscn needs them)
## Writes migration/golden/playtest_trace.json:
## { "steps": [{ "name", "events": [["action", name] | ["game", name] |
##     ["mouse", button (0 motion, 1 left, 2 right), pressed, x, y]] (x, y: the world position
##     the tools saw), "ticks", "tool": [id, variant], "save": Simulation.to_dict(),
##     "metrics": { vehicles, pedestrians, waiting, arrivals, failed, flow (* 1e6, null: none),
##     trips, time } }], "vehicles": [[id, lane id, distance, speed]] at the end }
## The Rust test (crates/tf_game/tests/playtest_parity.rs) replays the events.

const MAIN_SCENE := preload("res://scenes/main.tscn")
## Keys the Game autoload handles (not the tool manager).
const GAME_ACTIONS: Array[StringName] = [&"sim_toggle_pause", &"debug_load_scenario", &"debug_stress_traffic",
	&"debug_load_town", &"debug_rush_hour", &"debug_bus_lanes", &"debug_new_map"]

var _vp: SubViewport
var _main: Node
var _tools: ToolManager
var _log: Array = []
var _steps: Array = []
var _stats := { "arrivals": 0, "failed": 0 }
var _origin: Vector2i


func _ready() -> void:
	_run()


func _run() -> void:
	_vp = SubViewport.new()
	# Big, so the HUD (anchored to the edges) never covers the playing field.
	_vp.size = Vector2i(3600, 2200)
	add_child(_vp)
	_main = MAIN_SCENE.instantiate()
	_vp.add_child(_main)
	_tools = _main.get_node("World/Tools")
	await _frames(3)
	_origin = Grid.world_to_cell(Grid.map_rect().get_center())

	await _action(&"sim_toggle_pause")
	assert(Game.is_paused())
	await _action(&"debug_load_scenario")
	_hook()
	await _finish("F7 Bottleneck", 200)

	# R: a bypass round the lights, three roads; R again picks the next road type each time.
	await _action(&"tool_road")
	await _drag(_at(-3, 0), _at(-3, -4))
	await _frames(2)
	await _finish("R: bypass 1", 60)
	await _action(&"tool_road")
	await _drag(_at(-3, -4), _at(15, -4))
	await _finish("R again: next type, bypass 2", 60)
	await _action(&"tool_road")
	await _drag(_at(15, -4), _at(15, 0))
	await _finish("R again: next type, bypass 3", 60)
	# An invalid drag (a zero-length one) builds nothing, a click on a road leaves it alone.
	await _drag(_at(5, 3), _at(5, 3))
	await _finish("empty drag", 20)

	# V: a curve from the end of the side street (drag to the bend, click the end).
	await _action(&"road_curve_mode")
	await _mouse(0, false, _at(0, 7))
	await _mouse(1, true, _at(0, 7))
	await _mouse(0, false, _at(0, 10))
	await _mouse(1, false, _at(0, 10))
	await _click(_at(4, 10))
	await _finish("V: curved road", 60)
	await _action(&"road_curve_mode")
	await _action(&"tool_cancel")
	await _finish("V again, Esc", 20)

	# R x3 more: the one-way types (drag the way traffic goes), then a different type dragged
	# over a road converts it; Esc and a right-click abort a drag.
	await _action(&"tool_road")
	await _action(&"tool_road")
	await _action(&"tool_road")
	await _drag(_at(12, 3), _at(4, 3))
	await _finish("R x3: one-way road, west-going", 60)
	await _action(&"tool_road")
	await _drag(_at(8, 3), _at(4, 3))
	await _finish("R again: another type over it", 60)
	await _mouse(0, false, _at(6, 6))
	await _mouse(1, true, _at(6, 6))
	await _mouse(0, false, _at(10, 6))
	await _action(&"tool_cancel")
	await _mouse(1, false, _at(10, 6))
	await _mouse(0, false, _at(6, 6))
	await _mouse(1, true, _at(6, 6))
	await _mouse(0, false, _at(10, 6))
	await _click(_at(10, 6), 2)
	await _mouse(1, false, _at(10, 6))
	await _finish("Esc and right-click abort drags", 20)
	await _action(&"tool_cancel")

	# L: lights at the Bottleneck junction cycle through the controls, the new bypass junction gets some.
	await _action(&"tool_signals")
	await _mouse(0, false, _at(0, 0) + Vector2(5, 5))
	await _click(_at(0, 0) + Vector2(5, 5))
	await _finish("L: protected lefts", 40)
	await _click(_at(0, 0) + Vector2(5, 5))
	await _finish("L: no lights", 40)
	await _click(_at(-3, 0) + Vector2(-4, 3))
	await _finish("L: lights at the bypass junction", 40)
	await _action(&"tool_cancel")

	# H / C / I: zones along the bypass, X takes some away.
	await _action(&"tool_residential")
	await _drag(Grid.cell_center(_cell(0, -5)), Grid.cell_center(_cell(3, -5)))
	await _action(&"tool_commercial")
	await _drag(Grid.cell_center(_cell(5, -5)), Grid.cell_center(_cell(8, -5)))
	await _action(&"tool_industrial")
	await _drag(Grid.cell_center(_cell(10, -5)), Grid.cell_center(_cell(13, -5)))
	await _action(&"tool_commercial")
	await _drag(Grid.cell_center(_cell(1, -4)), Grid.cell_center(_cell(4, -4)))
	await _finish("H, C, I: zones", 400)
	await _action(&"tool_dezone")
	await _drag(Grid.cell_center(_cell(6, -5)), Grid.cell_center(_cell(7, -5)))
	await _finish("X: dezone two cells", 100)

	# B: bulldoze a road segment (the curve), then a zoned lot.
	await _action(&"tool_bulldoze")
	await _mouse(0, false, _at(0, 7) + Vector2(0, 40))
	await _mouse(1, true, _at(0, 7) + Vector2(0, 40))
	await _mouse(1, false, _at(0, 7) + Vector2(0, 40))
	await _finish("B: bulldoze the curve", 40)
	await _mouse(0, false, Grid.cell_center(_cell(11, -5)))
	await _mouse(1, true, Grid.cell_center(_cell(11, -5)))
	await _mouse(1, false, Grid.cell_center(_cell(11, -5)))
	await _finish("B: bulldoze a lot", 40)
	await _action(&"tool_bulldoze")

	# B again: hold and sweep over the zoned row (each lot under the cursor goes, its zone with it).
	await _action(&"tool_bulldoze")
	await _mouse(0, false, Grid.cell_center(_cell(1, -4)))
	await _mouse(1, true, Grid.cell_center(_cell(1, -4)))
	for x in [2, 3]:
		await _mouse(0, false, Grid.cell_center(_cell(x, -4)))
	await _mouse(1, false, Grid.cell_center(_cell(3, -4)))
	await _finish("B: sweep", 40)
	await _action(&"tool_cancel")

	# U: a three-stop line with curbside stops, one with bays; right-click removes a stop.
	await _action(&"tool_bus")
	for x in [-10, -6, 6]:
		await _click(_at(x, 0) + Vector2(0, 10))
	await _click(_at(-10, 0) + Vector2(0, 10))
	await _finish("U: curbside line", 100)
	await _action(&"tool_bus")
	for x in [7, 10, 13]:
		await _click(_at(x, 0) + Vector2(0, -10))
	await _click(_at(7, 0) + Vector2(0, -10))
	await _finish("U again: bay line", 100)
	await _click(_at(10, 0) + Vector2(0, -10), 2)
	await _finish("U: right-click removes a stop", 100)
	# Right-click opens a line being drawn; a click away from roads places nothing.
	for x in [-9, -5, 4]:
		await _click(_at(x, 0) + Vector2(0, -10))
	await _click(_at(-9, 6))
	await _click(_at(0, 0), 2)
	await _finish("U: right-click opens a line", 100)
	await _action(&"tool_cancel")

	# F6: extra random traffic, then the day goes on.
	await _action(&"debug_stress_traffic")
	await _finish("F6 stress traffic", 600)
	await _finish("run on", 1200)

	# The debug functions that had only a button in Godot (keys F10, 0, F12 in the Bevy build), and the Town.
	await _action(&"debug_rush_hour")
	await _finish("F10 rush hour", 100)
	await _action(&"debug_bus_lanes")
	_hook()
	await _finish("0 Bottleneck with bus lanes", 600)
	await _action(&"debug_new_map")
	_hook()
	await _finish("F12 new empty map", 40)
	await _action(&"debug_load_town")
	_hook()
	await _finish("F8 Town", 1200)

	var sim := Game.simulation
	var vehicles := []
	for v: Vehicle in sim.vehicles.vehicles.values():
		vehicles.append([v.id, v.lane.id, snappedf(v.distance, 0.001), snappedf(v.speed, 0.001)])
	var file := FileAccess.open("res://migration/golden/playtest_trace.json", FileAccess.WRITE)
	file.store_string(JSON.stringify({ "steps": _steps, "vehicles": vehicles }, "", false))
	for step: Dictionary in _steps:
		var save: Dictionary = step.save
		print("%-36s roads %d lots %d zones %d stops %d lines %d | %s" % [step.name, save.roads.segments.size(),
			save.lots.size(), save.zones.size(), save.transit.stops.size(), save.transit.lines.size(), step.metrics])
	get_tree().quit()


func _hook() -> void:
	var sim := Game.simulation
	sim.vehicles.vehicle_arrived.connect(func(_v: Vehicle) -> void: _stats.arrivals += 1)
	sim.vehicles.vehicle_failed.connect(func(_v: Vehicle) -> void: _stats.failed += 1)


func _at(x: int, y: int) -> Vector2:
	return Vector2(_origin + Vector2i(x, y)) * Grid.CELL_SIZE


func _cell(x: int, y: int) -> Vector2i:
	return _origin + Vector2i(x, y)


func _frames(n: int) -> void:
	for i in n:
		await get_tree().process_frame


## Ends a step: runs `ticks` ticks, records the state.
func _finish(step_name: String, ticks: int) -> void:
	await _frames(2)
	var sim := Game.simulation
	for i in ticks:
		sim.step()
	var now := sim.get_sim_time()
	var flow := sim.flow.get_flow_percent(now)
	var tool := _tools.active_tool
	_steps.append({
		"name": step_name,
		"events": _log,
		"ticks": ticks,
		"tool": [String(tool.name) if tool != null else "", String(tool.get_variant()) if tool != null else ""],
		"save": sim.to_dict(),
		"metrics": {
			"vehicles": sim.vehicles.vehicles.size(),
			"pedestrians": sim.pedestrians.pedestrians.size(),
			"waiting": sim.vehicles.get_waiting_departures().size(),
			"arrivals": _stats.arrivals,
			"failed": _stats.failed,
			"flow": null if flow < 0.0 else roundi(flow * 1e6),
			"trips": sim.flow.get_trip_count(now),
			"time": snappedf(now, 0.001),
		},
	})
	_log = []


func _action(action: StringName) -> void:
	var event := InputEventAction.new()
	event.action = action
	event.pressed = true
	_log.append(["game" if GAME_ACTIONS.has(action) else "action", String(action)])
	if action == &"debug_rush_hour":
		Game.rush_hour_now()
	elif action == &"debug_bus_lanes":
		Game.load_bottleneck_bus_lanes_scenario()
	elif action == &"debug_new_map":
		Game.new_empty_game()
	elif GAME_ACTIONS.has(action):
		Game._unhandled_input(event)
	else:
		_vp.push_input(event)
	await _frames(2)


## button: 0 motion, 1 left, 2 right.
func _mouse(button: int, pressed: bool, world: Vector2) -> void:
	var event: InputEvent
	if button == 0:
		event = InputEventMouseMotion.new()
	else:
		var click := InputEventMouseButton.new()
		click.button_index = MOUSE_BUTTON_LEFT if button == 1 else MOUSE_BUTTON_RIGHT
		click.pressed = pressed
		event = click
	(event as InputEventMouse).position = _vp.get_canvas_transform() * world
	_vp.push_input(event)
	var seen := _tools.get_pointer_world_position()
	_log.append(["mouse", button, pressed, seen.x, seen.y])
	await _frames(2)


func _drag(from: Vector2, to: Vector2) -> void:
	await _mouse(0, false, from)
	await _mouse(1, true, from)
	await _mouse(0, false, to)
	await _mouse(1, false, to)


func _click(world: Vector2, button := 1) -> void:
	await _mouse(0, false, world)
	await _mouse(button, true, world)
	await _mouse(button, false, world)
