class_name MapCamera
extends Camera2D
## Top-down map camera: keyboard pan, middle-mouse drag, wheel zoom toward the cursor,
## and trackpad pan/pinch. Runs on real time, so it works while the sim is paused.
## Zooms in steps (ZOOM_LEVELS) that keep the dot display's grid at 3 or 4 screen px per
## dot (DotDisplay.cell_size).

## World px to screen px: each one is a power-of-two dot cell times 3 or 4.
const ZOOM_LEVELS: Array[float] = [0.25, 0.375, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0]
## A pinch this much bigger (or smaller) moves one zoom level.
const PINCH_STEP := 1.15

@export var pan_speed := 900.0  ## Screen pixels per second.

var _dragging := false
## How far the current pinch has gone since the last zoom level.
var _pinch := 1.0


func _ready() -> void:
	position = Grid.map_rect().get_center()
	Events.camera_focus_requested.connect(_on_focus_requested)


func _on_focus_requested(world_position: Vector2) -> void:
	position = world_position
	_clamp_to_map()


func _process(delta: float) -> void:
	var direction := Input.get_vector("camera_pan_left", "camera_pan_right", "camera_pan_up", "camera_pan_down")
	if direction != Vector2.ZERO:
		_move_by(direction * pan_speed * delta / zoom.x)


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		match event.button_index:
			MOUSE_BUTTON_MIDDLE:
				_dragging = event.pressed
			MOUSE_BUTTON_WHEEL_UP:
				if event.pressed:
					zoom_by_levels(1, event.position)
			MOUSE_BUTTON_WHEEL_DOWN:
				if event.pressed:
					zoom_by_levels(-1, event.position)
	elif event is InputEventMouseMotion and _dragging:
		_move_by(-event.relative / zoom.x)
	elif event is InputEventMagnifyGesture:
		_pinch *= event.factor
		while _pinch > PINCH_STEP:
			_pinch /= PINCH_STEP
			zoom_by_levels(1, event.position)
		while _pinch < 1.0 / PINCH_STEP:
			_pinch *= PINCH_STEP
			zoom_by_levels(-1, event.position)
	elif event is InputEventPanGesture:
		_move_by(event.delta * 16.0 / zoom.x)


## Moves `steps` zoom levels in (out if negative), keeping the world point under
## `screen_pos` fixed on screen.
func zoom_by_levels(steps: int, screen_pos: Vector2) -> void:
	var old_zoom := zoom.x
	var level := clampi(nearest_level(old_zoom) + steps, 0, ZOOM_LEVELS.size() - 1)
	var new_zoom := ZOOM_LEVELS[level]
	if is_equal_approx(new_zoom, old_zoom):
		return
	var offset_from_center := screen_pos - get_viewport_rect().size / 2.0
	var world_under_cursor := position + offset_from_center / old_zoom
	zoom = Vector2.ONE * new_zoom
	position = world_under_cursor - offset_from_center / new_zoom
	_clamp_to_map()


## The index of the zoom level closest to `value`.
static func nearest_level(value: float) -> int:
	var best := 0
	for i in ZOOM_LEVELS.size():
		if absf(log(ZOOM_LEVELS[i] / value)) < absf(log(ZOOM_LEVELS[best] / value)):
			best = i
	return best


func _move_by(world_delta: Vector2) -> void:
	position += world_delta
	_clamp_to_map()


func _clamp_to_map() -> void:
	var rect := Grid.map_rect()
	position = position.clamp(rect.position, rect.end)
