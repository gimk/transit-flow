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


func test_road_tool_crossing_drag_creates_a_junction() -> void:
	Game.simulation.roads.add_road(_grid_point(-2, 0), _grid_point(2, 0), RoadType.load_by_id(&"local_2lane"))
	await _press_action(&"tool_road")
	await _drag(_grid_point(0, -2), _grid_point(0, 2))
	await wait_process_frames(10)
	assert_eq(Game.simulation.roads.segments.size(), 4)
	var center := Game.simulation.roads.find_node_at(_grid_point(0, 0))
	assert_eq(Game.simulation.roads.nodes[center].get_degree(), 4)


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


func test_route_tool_shows_route_between_two_clicked_roads() -> void:
	var t := RoadType.load_by_id(&"local_2lane")
	var roads := Game.simulation.roads
	roads.add_road(_grid_point(-3, 0), _grid_point(0, 0), t)
	roads.add_road(_grid_point(0, 0), _grid_point(0, 3), t)
	await _press_action(&"tool_route")
	var tool: RouteTool = _main.get_node("World/Tools/RouteTool")
	# South side of the east-west road = eastbound lane; west side of the north-south road = southbound.
	await _mouse_left(_grid_point(-2, 0) + Vector2(0, 7), true)
	await _mouse_left(_grid_point(0, 2) + Vector2(-7, 0), true)
	assert_not_null(tool._route)
	assert_eq(tool._route.lane_ids.size(), 3)
	assert_eq(roads.lanes[tool._route.lane_ids[1]].turn, Lane.Turn.RIGHT)
	# Bulldozing a road on the route clears it rather than leaving a stale drawing.
	roads.remove_segment(roads.find_segment_at(_grid_point(0, 2), 4.0))
	await wait_process_frames(2)
	assert_null(tool._route)


func test_zone_tools_paint_areas_that_develop_next_to_roads() -> void:
	var sim := Game.simulation
	sim.roads.add_road(_grid_point(-4, 0), _grid_point(4, 0), RoadType.load_by_id(&"local_2lane"))
	var row := Grid.world_to_cell(_grid_point(0, -1) + Vector2(32, 32)).y
	var first := Vector2i(Grid.world_to_cell(_grid_point(-3, 0)).x, row)
	var last := first + Vector2i(2, 0)
	await _press_action(&"tool_residential")
	await _mouse_left(Grid.cell_center(first), true)
	await _mouse_motion(Grid.cell_center(last))
	await _mouse_left(Grid.cell_center(last), false)
	for x in range(first.x, last.x + 1):
		assert_eq(sim.zones.zones.get(Vector2i(x, row)), Lot.Type.RESIDENTIAL)
	# A cell two rows away from any road is zoned but can't develop.
	var far := first + Vector2i(0, -3)
	await _press_action(&"tool_commercial")
	await _mouse_left(Grid.cell_center(far), true)
	await _mouse_left(Grid.cell_center(far), false)

	sim.zones.step(1000.0)
	for x in range(first.x, last.x + 1):
		assert_ne(sim.lots.get_lot_at_cell(Vector2i(x, row)), -1, "developed next to the road")
	assert_eq(sim.lots.get_lot_at_cell(far), -1, "no road, no building")
	assert_true(sim.zones.is_waiting_for_road(far))

	await _press_action(&"tool_dezone")
	await _mouse_left(Grid.cell_center(last), true)
	await _mouse_left(Grid.cell_center(last), false)
	assert_false(sim.zones.zones.has(last))
	assert_eq(sim.lots.get_lot_at_cell(last), -1, "dezoning removes the building")

	await _press_action(&"tool_bulldoze")
	await _mouse_motion(Grid.cell_center(first))
	await _mouse_left(Grid.cell_center(first), true)
	await _mouse_left(Grid.cell_center(first), false)
	assert_eq(sim.lots.get_lot_at_cell(first), -1)
	assert_false(sim.zones.zones.has(first), "bulldozing a building also dezones it")


func test_main_scene_has_its_key_nodes() -> void:
	for path in ["World/GridRenderer", "World/ZoneRenderer", "World/RoadRenderer", "World/IntersectionRenderer", "World/LotRenderer",
			"World/VehicleRenderer", "World/HeatmapOverlay", "World/DayNight", "World/ExternalRenderer",
			"World/LaneOverlay", "World/Tools", "MapCamera", "HUD"]:
		assert_not_null(_main.get_node_or_null(path), path)
	for tool_id: StringName in ToolManager.TOOL_SHORTCUTS.values():
		assert_not_null(_main.get_node_or_null("World/Tools/" + tool_id), "tool " + tool_id)


func test_signal_tool_cycles_junction_control() -> void:
	var t := RoadType.load_by_id(&"local_2lane")
	var roads := Game.simulation.roads
	roads.add_road(_grid_point(-2, 0), _grid_point(2, 0), t)
	roads.add_road(_grid_point(0, -2), _grid_point(0, 2), t)
	var node := roads.nodes[roads.find_node_at(_grid_point(0, 0))]
	await _press_action(&"tool_signals")
	await _mouse_motion(_grid_point(0, 0) + Vector2(5, 5))
	var expected := [RoadNode.JunctionControl.TRAFFIC_LIGHT, RoadNode.JunctionControl.NONE]
	for control in expected:
		await _mouse_left(_grid_point(0, 0) + Vector2(5, 5), true)
		await _mouse_left(_grid_point(0, 0) + Vector2(5, 5), false)
		assert_eq(node.control, control)


func test_bottleneck_scenario_loads() -> void:
	Game.load_bottleneck_scenario()
	await wait_process_frames(2)
	assert_eq(Game.simulation.citizens.citizens.size(), 56)
	var controlled := Game.simulation.roads.nodes.values().filter(
		func(n: RoadNode) -> bool: return n.control == RoadNode.JunctionControl.TRAFFIC_LIGHT)
	assert_eq(controlled.size(), 1)


func _debug_button(prefix: String) -> Button:
	for button: Button in _main.get_node("HUD/DebugMenu").find_children("*", "Button", true, false):
		if button.text.begins_with(prefix):
			return button
	return null


func test_debug_menu_buttons_drive_the_debug_functions() -> void:
	var heatmap: HeatmapOverlay = _main.get_node("World/HeatmapOverlay")
	var heatmap_button := _debug_button("Congestion heatmap")
	assert_not_null(heatmap_button)
	heatmap_button.pressed.emit()
	assert_true(heatmap.visible, "button shows the heatmap")
	heatmap.toggle()  # As if F4 was pressed.
	assert_false(heatmap_button.button_pressed, "button follows the keyboard toggle")

	_debug_button("Stress traffic").pressed.emit()
	assert_eq(Game.simulation.stress.target_count, Game.STRESS_TRAFFIC_STEPS[1])
	await wait_process_frames(20)
	assert_string_contains(_debug_button("Stress traffic").text, str(Game.STRESS_TRAFFIC_STEPS[1]))

	_debug_button("Bottleneck").pressed.emit()
	assert_eq(Game.simulation.citizens.citizens.size(), 56)
	_debug_button("New empty map").pressed.emit()
	assert_eq(Game.simulation.roads.segments.size(), 1, "only the highway stub of a new map")
	assert_eq(Game.simulation.externals.points.size(), 1)


func test_debug_menu_folds_away() -> void:
	var menu: DebugMenu = _main.get_node("HUD/DebugMenu")
	var body: Control = menu.get_child(0).get_child(0).get_child(1)
	assert_true(body.visible)
	await _press_action(&"toggle_debug_menu")
	assert_false(body.visible)
	await _press_action(&"toggle_debug_menu")
	assert_true(body.visible)
