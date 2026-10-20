extends Node
## Owns the running Simulation and drives it with a fixed-tick accumulator (autoload "Game").
## Pausing sets the speed to 0 instead of pausing the SceneTree,
## so the camera, tools and UI keep working while the sim is frozen.

const SPEED_MULTIPLIERS: Array[int] = [0, 1, 2, 4]
## Upper bound on catch-up ticks in a single frame. Past this the backlog is dropped,
## so a slow frame makes the sim run slower instead of spiralling.
const MAX_TICKS_PER_FRAME := 16

var simulation: Simulation
var speed_index := 1
## How far (0–1) the current frame sits between the last tick and the next.
## Renderers use it to interpolate moving things smoothly.
var interpolation_alpha := 0.0

var _last_running_speed_index := 1
var _accumulator := 0.0


func _ready() -> void:
	new_simulation()


func new_simulation() -> void:
	simulation = Simulation.new()
	_accumulator = 0.0
	interpolation_alpha = 0.0
	simulation.clock.hour_changed.connect(Events.hour_changed.emit)
	simulation.clock.day_started.connect(Events.day_started.emit)
	Events.simulation_reset.emit()


func _process(delta: float) -> void:
	_accumulator += delta * SPEED_MULTIPLIERS[speed_index]
	var ticks := 0
	while _accumulator >= Simulation.TICK_DT and ticks < MAX_TICKS_PER_FRAME:
		simulation.step()
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
	else:
		return
	get_viewport().set_input_as_handled()
