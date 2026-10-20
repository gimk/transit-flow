class_name MapCamera
extends Camera2D
## Top-down map camera: keyboard pan, middle-mouse drag, wheel zoom toward the cursor,
## and trackpad pan/pinch. Runs on real time, so it works while the sim is paused.

@export var pan_speed := 900.0  ## Screen pixels per second.
@export var min_zoom := 0.25
@export var max_zoom := 3.0
@export var zoom_step := 1.15

var _dragging := false


func _ready() -> void:
	position = Grid.map_rect().get_center()


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
					_zoom_at(zoom_step, event.position)
			MOUSE_BUTTON_WHEEL_DOWN:
				if event.pressed:
					_zoom_at(1.0 / zoom_step, event.position)
	elif event is InputEventMouseMotion and _dragging:
		_move_by(-event.relative / zoom.x)
	elif event is InputEventMagnifyGesture:
		_zoom_at(event.factor, event.position)
	elif event is InputEventPanGesture:
		_move_by(event.delta * 16.0 / zoom.x)


## Zooms while keeping the world point under `screen_pos` fixed on screen.
func _zoom_at(factor: float, screen_pos: Vector2) -> void:
	var old_zoom := zoom.x
	var new_zoom := clampf(old_zoom * factor, min_zoom, max_zoom)
	if is_equal_approx(new_zoom, old_zoom):
		return
	var offset_from_center := screen_pos - get_viewport_rect().size / 2.0
	var world_under_cursor := position + offset_from_center / old_zoom
	zoom = Vector2.ONE * new_zoom
	position = world_under_cursor - offset_from_center / new_zoom
	_clamp_to_map()


func _move_by(world_delta: Vector2) -> void:
	position += world_delta
	_clamp_to_map()


func _clamp_to_map() -> void:
	var rect := Grid.map_rect()
	position = position.clamp(rect.position, rect.end)
