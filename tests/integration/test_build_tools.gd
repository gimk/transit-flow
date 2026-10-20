extends GutTest
## Drives the real main scene with synthetic mouse/keyboard input.
## The scene runs inside its own SubViewport and input is pushed straight into it:
## going through the root window would let GUT's on-screen output panel swallow clicks.

const MAIN_SCENE := preload("res://scenes/main.tscn")

var _viewport: SubViewport
var _main: Node


func before_each() -> void:
	Game.new_simulation()
	_viewport = SubViewport.new()
	_viewport.size = Vector2i(1152, 648)
	add_child_autofree(_viewport)
	_main = MAIN_SCENE.instantiate()
	_viewport.add_child(_main)
	await wait_process_frames(2)


func _to_screen(world: Vector2) -> Vector2:
	return _viewport.get_canvas_transform() * world


func _press_action(action: StringName) -> void:
	var event := InputEventAction.new()
	event.action = action
	event.pressed = true
	_viewport.push_input(event)
	await wait_process_frames(1)


func _mouse_motion(world: Vector2) -> void:
	var event := InputEventMouseMotion.new()
	event.position = _to_screen(world)
	_viewport.push_input(event)
	await wait_process_frames(2)


func _mouse_left(world: Vector2, pressed: bool) -> void:
	var event := InputEventMouseButton.new()
	event.button_index = MOUSE_BUTTON_LEFT
	event.pressed = pressed
	event.position = _to_screen(world)
	_viewport.push_input(event)
	await wait_process_frames(2)


func _drag(world_from: Vector2, world_to: Vector2) -> void:
	await _mouse_motion(world_from)
	await _mouse_left(world_from, true)
	await _mouse_motion(world_to)
	await _mouse_left(world_to, false)


## A grid point a few cells away from the camera's start position (the map center).
func _grid_point(dx: int, dy: int) -> Vector2:
	return Grid.snap_to_point(Grid.map_rect().get_center()) + Vector2(dx, dy) * Grid.CELL_SIZE


func test_road_tool_builds_axis_locked_road_from_drag() -> void:
	await _press_action(&"tool_road")
	# Slightly off-axis and off-grid drag: should snap and lock to a horizontal road.
	await _drag(_grid_point(-2, 0) + Vector2(5, -3), _grid_point(2, 0) + Vector2(-4, 20))
	await wait_process_frames(10)
	var roads := Game.simulation.roads
	assert_eq(roads.segments.size(), 1)
	assert_ne(roads.find_node_at(_grid_point(-2, 0)), -1)
	assert_ne(roads.find_node_at(_grid_point(2, 0)), -1)


func test_road_tool_rejects_crossing_drag() -> void:
	Game.simulation.roads.add_road(_grid_point(-2, 0), _grid_point(2, 0), RoadType.load_by_id(&"local_2lane"))
	await _press_action(&"tool_road")
	await _drag(_grid_point(0, -2), _grid_point(0, 2))
	await wait_process_frames(10)
	assert_eq(Game.simulation.roads.segments.size(), 1)


func test_bulldoze_tool_removes_clicked_road() -> void:
	Game.simulation.roads.add_road(_grid_point(-2, 0), _grid_point(2, 0), RoadType.load_by_id(&"local_2lane"))
	await _press_action(&"tool_bulldoze")
	await _drag(_grid_point(0, 0), _grid_point(0, 0))
	await wait_process_frames(10)
	assert_eq(Game.simulation.roads.segments.size(), 0)
	assert_eq(Game.simulation.roads.nodes.size(), 0)


func test_renderer_follows_network_changes() -> void:
	var renderer: RoadRenderer = _main.get_node("World/RoadRenderer")
	var roads := Game.simulation.roads
	var id := roads.add_road(_grid_point(-2, 0), _grid_point(2, 0), RoadType.load_by_id(&"local_2lane"))
	await wait_process_frames(2)
	assert_eq(renderer.get_node("Surfaces").get_child_count(), 1)
	roads.remove_segment(id)
	await wait_process_frames(2)
	assert_eq(renderer.get_node("Surfaces").get_child_count(), 0)


func test_quick_save_and_load_restores_roads() -> void:
	var path := "user://test_save.json"
	Game.simulation.roads.add_road(_grid_point(-2, 0), _grid_point(2, 0), RoadType.load_by_id(&"local_2lane"))
	assert_eq(Game.save_game(path), OK)
	Game.new_simulation()
	assert_eq(Game.simulation.roads.segments.size(), 0)
	assert_eq(Game.load_game(path), OK)
	assert_eq(Game.simulation.roads.segments.size(), 1)
	await wait_process_frames(2)
	var renderer: RoadRenderer = _main.get_node("World/RoadRenderer")
	assert_eq(renderer.get_node("Surfaces").get_child_count(), 1, "renderer rebuilt after load")
	DirAccess.remove_absolute(ProjectSettings.globalize_path(path))
