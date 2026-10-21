extends Node
## Owns the running Simulation and drives it at a fixed tick (autoload "Game").
## Pausing sets the speed to 0 instead of pausing the SceneTree,
## so the camera, tools and UI keep working while the sim is frozen.
##
## Threading. Outside headless runs the simulation steps on a worker thread, in small
## batches that keep it a couple of ticks ahead of what is on screen, so a tick's cost
## spreads over the frames of its tick period instead of landing on one frame. After
## each tick the worker captures a ViewSnapshot; per-frame views draw from the two
## snapshots around the render clock (view_from / view_to / view_alpha). Rules:
## - `simulation` waits for the worker before returning it: anything may use it, at the
##   cost of a short wait if a batch is running (so per-frame code should use the view).
## - `roads` returns the road network without waiting: only the main thread changes
##   roads (through `simulation`), and the worker only reads them.
## - Simulation signals the views listen to are relayed to Events on the main thread.
## If the worker can't keep up, the render clock waits for it: the sim runs slower than
## asked (sim_lagging) instead of frames stalling. Headless (tests) steps inline.

const SPEED_MULTIPLIERS: Array[int] = [0, 1, 2, 4]
## The simulation runs this far ahead of the screen (in ticks): the time a tick has to
## compute in the background before it's needed on screen.
const LEAD_TICKS := 2
## Most ticks the worker runs in one go.
const MAX_TICKS_PER_BATCH := 4
## sim_lagging: the simulation runs below this fraction of the chosen speed (smoothed).
const LAG_THRESHOLD := 0.9
const SPEED_SMOOTHING := 0.02
const QUICKSAVE_PATH := "user://quicksave.json"
## Vehicle counts the F6 debug key cycles through.
const STRESS_TRAFFIC_STEPS: Array[int] = [0, 250, 500, 1000, 2000]

var simulation: Simulation:
	get:
		_wait_for_worker()
		return _simulation
## The road network, readable without waiting for the worker (see the class doc).
var roads: RoadNetwork:
	get:
		return _simulation.roads
## Debug stress traffic level (set on the main thread only, so no need to wait).
var stress_target: int:
	get:
		return _simulation.stress.target_count
var speed_index := 1
## The snapshots just before and after the render clock, and how far (0–1) between them:
## per-frame views draw from these, interpolating moving things.
var view_from: ViewSnapshot
var view_to: ViewSnapshot
var view_alpha := 0.0
## How fast the simulation actually runs (sim seconds per real second, smoothed), and
## whether that is well below the chosen speed (it couldn't keep up, so it ran slower).
var effective_speed := 1.0
var sim_lagging := false
## Smoothed cost of one simulation tick, for the debug readout.
var average_tick_ms := 0.0

var _simulation: Simulation
var _last_running_speed_index := 1
var _threaded := false
## The worker thread, its wake-up and done signals, the batch it runs, and whether one is
## running (main thread side).
var _worker: Thread
var _work_ready := Semaphore.new()
var _work_done := Semaphore.new()
var _batch_size := 0
var _busy := false
var _quitting := false
## Snapshots ready for the views, oldest first, and those the running batch produced.
var _snapshots: Array[ViewSnapshot] = []
var _batch_snapshots: Array[ViewSnapshot] = []
## Sim time on screen.
var _render_time := 0.0

## Simulation signals waiting to be relayed to Events (set from the worker thread).
var _relay_hours: Array[int] = []
var _relay_days: Array[int] = []
var _relay_lots := false
var _relay_zones := false


func _ready() -> void:
	_threaded = DisplayServer.get_name() != "headless" and OS.get_processor_count() > 1
	if _threaded:
		_worker = Thread.new()
		_worker.start(_worker_loop, Thread.PRIORITY_HIGH)
	new_simulation()
	Scenarios.add_highway(simulation)


## Replaces the running simulation (a fresh one if `sim` is null) and tells everyone to resync.
func new_simulation(sim: Simulation = null) -> void:
	_wait_for_worker()
	if _simulation != null:
		_simulation.dispose()
	_simulation = sim if sim != null else Simulation.new()
	_simulation.clock.hour_changed.connect(_on_hour_changed)
	_simulation.clock.day_started.connect(_on_day_started)
	_simulation.roads.changed.connect(Events.road_network_changed.emit)
	_simulation.lots.changed.connect(_on_lots_changed)
	_simulation.zones.changed.connect(_on_zones_changed)
	_simulation.externals.changed.connect(Events.externals_changed.emit)
	_snapshots = [ViewSnapshot.capture(_simulation)]
	_render_time = _snapshots[0].time
	_update_view()
	Events.simulation_reset.emit()


## Saves to the quicksave slot and tells the player how it went.
func quick_save() -> void:
	var error := save_game()
	Events.notification_posted.emit("Game saved" if error == OK else "Save failed (%s)" % error_string(error))


func quick_load() -> void:
	var error := load_game()
	Events.notification_posted.emit("Game loaded" if error == OK else "Load failed (%s)" % error_string(error))


## Starts over on an empty map (with its highway connection).
func new_empty_game() -> void:
	new_simulation()
	Scenarios.add_highway(simulation)
	Events.camera_focus_requested.emit(simulation.externals.points[0])
	Events.notification_posted.emit("New empty map")


## Debug: everyone at home with a job leaves for work right away.
func rush_hour_now() -> void:
	simulation.citizens.depart_everyone_now()
	Events.notification_posted.emit("Rush hour: everyone at home leaves for work")


## Steps the debug stress traffic through STRESS_TRAFFIC_STEPS (0, 250, 500 …).
func cycle_stress_traffic() -> void:
	var stress := simulation.stress
	var step := (STRESS_TRAFFIC_STEPS.find(stress.target_count) + 1) % STRESS_TRAFFIC_STEPS.size()
	stress.target_count = STRESS_TRAFFIC_STEPS[step]
	Events.notification_posted.emit("Stress traffic: %d vehicles" % stress.target_count)


## Replaces the game with the "Bottleneck" playtest scenario (F7) and centers on it.
func load_bottleneck_scenario() -> void:
	new_simulation()
	var info := Scenarios.build_bottleneck(simulation, Grid.world_to_cell(Grid.map_rect().get_center()))
	Events.camera_focus_requested.emit(info.junction)
	Events.notification_posted.emit("Scenario: Bottleneck. Relieve the jammed main road (F4 shows congestion)")


## The Bottleneck with its main road given bus lanes and a bus line along it (debug menu).
func load_bottleneck_bus_lanes_scenario() -> void:
	new_simulation()
	var origin := Grid.world_to_cell(Grid.map_rect().get_center())
	var info := Scenarios.build_bottleneck(simulation, origin)
	Scenarios.add_bottleneck_bus_lanes(simulation, origin)
	Events.camera_focus_requested.emit(info.junction)
	Events.notification_posted.emit("Scenario: Bottleneck with bus lanes. Compare Flow with plain F7")


## Replaces the game with the "Town" scenario (F8): a complete small city that lives
## through a whole day, starting just before the morning rush.
func load_town_scenario() -> void:
	new_simulation()
	var info := Scenarios.build_town(simulation)
	Events.camera_focus_requested.emit(info.center)
	Events.notification_posted.emit("Scenario: Town. Watch a full day; F4 shows congestion, L toggles lights")


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
	if _busy and _work_done.try_wait():
		_collect_batch()
	var multiplier := SPEED_MULTIPLIERS[speed_index]
	var before := _render_time
	_render_time += delta * multiplier
	var newest: float = _snapshots.back().time
	if _render_time > newest:
		# The simulation is behind: wait for it rather than run ahead of it.
		_render_time = newest
	if multiplier > 0 and delta > 0.0:
		effective_speed = lerpf(effective_speed, maxf(_render_time - before, 0.0) / delta, SPEED_SMOOTHING)
		sim_lagging = effective_speed < multiplier * LAG_THRESHOLD
	else:
		sim_lagging = false
	# Relay the signals of the ticks just collected while the worker is idle (once the
	# next batch starts, the worker may be adding to them).
	if not _busy:
		_flush_relays()
	if multiplier > 0:
		_start_batch()
	_update_view()


## Runs (or, threaded, starts running) the ticks that keep the simulation LEAD_TICKS
## ahead of the screen.
func _start_batch() -> void:
	if _busy:
		return
	var behind := _render_time + LEAD_TICKS * Simulation.TICK_DT - _simulation.get_sim_time()
	var count := mini(ceili(behind / Simulation.TICK_DT - 0.001), MAX_TICKS_PER_BATCH)
	if count <= 0:
		return
	if _threaded:
		_batch_size = count
		_busy = true
		_work_ready.post()
	else:
		_run_ticks(count)
		_collect_batch()


## The worker thread: one long-lived thread (the OS learns it is busy and gives it a fast
## core, which a fresh thread per batch doesn't get), running a batch whenever asked.
func _worker_loop() -> void:
	while true:
		_work_ready.wait()
		if _quitting:
			return
		_run_ticks(_batch_size)
		_work_done.post()


## Steps the simulation and snapshots each tick for the views (on the worker if threaded).
func _run_ticks(count: int) -> void:
	for i in count:
		var started := Time.get_ticks_usec()
		_simulation.step()
		average_tick_ms = lerpf(average_tick_ms, (Time.get_ticks_usec() - started) / 1000.0, 0.05)
		_batch_snapshots.append(ViewSnapshot.capture(_simulation))


## The batch is over: its snapshots become available to the views.
func _collect_batch() -> void:
	_busy = false
	_snapshots.append_array(_batch_snapshots)
	_batch_snapshots.clear()


## Lets a running batch finish (so the simulation can be used from the main thread).
func _wait_for_worker() -> void:
	if _busy:
		_work_done.wait()
		_collect_batch()


## Picks the snapshots on either side of the render clock, dropping older ones.
func _update_view() -> void:
	while _snapshots.size() > 2 and _snapshots[1].time <= _render_time:
		_snapshots.pop_front()
	view_from = _snapshots[0]
	view_to = _snapshots[1] if _snapshots.size() > 1 else _snapshots[0]
	var span := view_to.time - view_from.time
	view_alpha = clampf((_render_time - view_from.time) / span, 0.0, 1.0) if span > 0.0 else 0.0


func _flush_relays() -> void:
	for hour in _relay_hours:
		Events.hour_changed.emit(hour)
	for day in _relay_days:
		Events.day_started.emit(day)
	_relay_hours.clear()
	_relay_days.clear()
	if _relay_lots:
		_relay_lots = false
		Events.lots_changed.emit()
	if _relay_zones:
		_relay_zones = false
		Events.zones_changed.emit()


# Simulation signals, possibly on the worker thread: noted, relayed by _flush_relays.
func _on_hour_changed(hour: int) -> void:
	_relay_hours.append(hour)


func _on_day_started(day: int) -> void:
	_relay_days.append(day)


func _on_lots_changed() -> void:
	_relay_lots = true


func _on_zones_changed() -> void:
	_relay_zones = true


func set_speed(index: int) -> void:
	speed_index = clampi(index, 0, SPEED_MULTIPLIERS.size() - 1)
	effective_speed = SPEED_MULTIPLIERS[speed_index]
	if speed_index > 0:
		_last_running_speed_index = speed_index
	Events.speed_changed.emit(speed_index, SPEED_MULTIPLIERS[speed_index])


## Switches between a window and exclusive fullscreen (F11). Exclusive fullscreen
## presents straight to the screen, bypassing the desktop compositor, which can avoid
## tearing and hitches on laptops with two GPUs.
func toggle_fullscreen() -> void:
	var fullscreen := DisplayServer.window_get_mode() == DisplayServer.WINDOW_MODE_EXCLUSIVE_FULLSCREEN
	DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_WINDOWED if fullscreen else DisplayServer.WINDOW_MODE_EXCLUSIVE_FULLSCREEN)


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
		quick_save()
	elif event.is_action_pressed("quick_load"):
		quick_load()
	elif event.is_action_pressed("debug_load_scenario"):
		load_bottleneck_scenario()
	elif event.is_action_pressed("debug_load_town"):
		load_town_scenario()
	elif event.is_action_pressed("toggle_fullscreen"):
		toggle_fullscreen()
	elif event.is_action_pressed("debug_stress_traffic"):
		cycle_stress_traffic()
	else:
		return
	get_viewport().set_input_as_handled()


func _exit_tree() -> void:
	_wait_for_worker()
	if _worker != null:
		_quitting = true
		_work_ready.post()
		_worker.wait_to_finish()
	if _simulation != null:
		_simulation.dispose()
