extends Node
## Owns the running Simulation and drives it with a fixed-tick accumulator (autoload "Game").
## Pausing sets the speed to 0 instead of pausing the SceneTree,
## so the camera, tools and UI keep working while the sim is frozen.

const SPEED_MULTIPLIERS: Array[int] = [0, 1, 2, 4]
## Upper bound on catch-up ticks in a single frame. Past this the backlog is dropped,
## so a slow frame makes the sim run slower instead of spiralling.
const MAX_TICKS_PER_FRAME := 16
const QUICKSAVE_PATH := "user://quicksave.json"
## Vehicle counts the F6 debug key cycles through.
const STRESS_TRAFFIC_STEPS: Array[int] = [0, 250, 500, 1000, 2000]

var simulation: Simulation
var speed_index := 1
## How far (0–1) the current frame sits between the last tick and the next.
## Renderers use it to interpolate moving things smoothly.
var interpolation_alpha := 0.0
## Smoothed cost of one simulation tick, for the debug readout.
var average_tick_ms := 0.0

var _last_running_speed_index := 1
var _accumulator := 0.0


func _ready() -> void:
	new_simulation()


## Replaces the running simulation (a fresh one if `sim` is null) and tells everyone to resync.
func new_simulation(sim: Simulation = null) -> void:
	if simulation != null:
		simulation.dispose()
	simulation = sim if sim != null else Simulation.new()
	_accumulator = 0.0
	interpolation_alpha = 0.0
	simulation.clock.hour_changed.connect(Events.hour_changed.emit)
	simulation.clock.day_started.connect(Events.day_started.emit)
	simulation.roads.changed.connect(Events.road_network_changed.emit)
	simulation.lots.changed.connect(Events.lots_changed.emit)
	Events.simulation_reset.emit()


## Replaces the game with the "Bottleneck" playtest scenario (F7) and centers on it.
func load_bottleneck_scenario() -> void:
	new_simulation()
	var info := Scenarios.build_bottleneck(simulation, Grid.world_to_cell(Grid.map_rect().get_center()))
	Events.camera_focus_requested.emit(info.junction)
	Events.notification_posted.emit("Scenario: Bottleneck. Relieve the stop-sign junction (F4 shows congestion)")


func save_game(path := QUICKSAVE_PATH) -> Error:
	var file := FileAccess.open(path, FileAccess.WRITE)
	if file == null:
		return FileAccess.get_open_error()
	file.store_string(JSON.stringify(simulation.to_dict(), "\t", true, true))
	return OK


func load_game(path := QUICKSAVE_PATH) -> Error:
	if not FileAccess.file_exists(path):
		return ERR_FILE_NOT_FOUND
	var data: Variant = JSON.parse_string(FileAccess.get_file_as_string(path))
	if not data is Dictionary:
		return ERR_PARSE_ERROR
	var sim := Simulation.from_dict(data)
	if sim == null:
		return ERR_INVALID_DATA
	new_simulation(sim)
	return OK


func _process(delta: float) -> void:
	_accumulator += delta * SPEED_MULTIPLIERS[speed_index]
	var ticks := 0
	while _accumulator >= Simulation.TICK_DT and ticks < MAX_TICKS_PER_FRAME:
		var started := Time.get_ticks_usec()
		simulation.step()
		var tick_ms := (Time.get_ticks_usec() - started) / 1000.0
		average_tick_ms = lerpf(average_tick_ms, tick_ms, 0.05)
		_accumulator -= Simulation.TICK_DT
		ticks += 1
	if ticks == MAX_TICKS_PER_FRAME:
		_accumulator = 0.0
	interpolation_alpha = _accumulator / Simulation.TICK_DT


func set_speed(index: int) -> void:
	speed_index = clampi(index, 0, SPEED_MULTIPLIERS.size() - 1)
	if speed_index > 0:
		_last_running_speed_index = speed_index
	Events.speed_changed.emit(speed_index, SPEED_MULTIPLIERS[speed_index])


func toggle_pause() -> void:
	set_speed(_last_running_speed_index if is_paused() else 0)


func is_paused() -> bool:
	return speed_index == 0


func _unhandled_input(event: InputEvent) -> void:
	if event.is_action_pressed("sim_toggle_pause"):
		toggle_pause()
	elif event.is_action_pressed("sim_speed_1"):
		set_speed(1)
	elif event.is_action_pressed("sim_speed_2"):
		set_speed(2)
	elif event.is_action_pressed("sim_speed_3"):
		set_speed(3)
	elif event.is_action_pressed("quick_save"):
		var error := save_game()
		Events.notification_posted.emit("Game saved" if error == OK else "Save failed (%s)" % error_string(error))
	elif event.is_action_pressed("quick_load"):
		var error := load_game()
		Events.notification_posted.emit("Game loaded" if error == OK else "Load failed (%s)" % error_string(error))
	elif event.is_action_pressed("debug_load_scenario"):
		load_bottleneck_scenario()
	elif event.is_action_pressed("debug_stress_traffic"):
		var stress := simulation.stress
		var step := (STRESS_TRAFFIC_STEPS.find(stress.target_count) + 1) % STRESS_TRAFFIC_STEPS.size()
		stress.target_count = STRESS_TRAFFIC_STEPS[step]
		Events.notification_posted.emit("Stress traffic: %d vehicles" % stress.target_count)
	else:
		return
	get_viewport().set_input_as_handled()


func _exit_tree() -> void:
	if simulation != null:
		simulation.dispose()
