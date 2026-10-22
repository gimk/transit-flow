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
	for path in ["World/GridRenderer", "World/ZoneRenderer", "World/NatureRenderer", "World/RoadRenderer", "World/IntersectionRenderer", "World/LotRenderer",
			"World/VehicleRenderer", "World/PedestrianRenderer", "World/TransitRenderer", "World/HeatmapOverlay", "World/DayNight", "World/ExternalRenderer",
			"World/LaneOverlay", "World/Tools", "MapCamera", "HUD", "HUD/ToolPalette"]:
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
	var expected := [RoadNode.JunctionControl.TRAFFIC_LIGHT, RoadNode.JunctionControl.TRAFFIC_LIGHT_PROTECTED_LEFT,
		RoadNode.JunctionControl.NONE]
	for control in expected:
		await _mouse_left(_grid_point(0, 0) + Vector2(5, 5), true)
		await _mouse_left(_grid_point(0, 0) + Vector2(5, 5), false)
		assert_eq(node.control, control)


func _click(world: Vector2, button := MOUSE_BUTTON_LEFT) -> void:
	await _mouse_motion(world)
	for pressed in [true, false]:
		var event := InputEventMouseButton.new()
		event.button_index = button
		event.pressed = pressed
		event.position = _to_screen(world)
		_viewport.push_input(event)
		await wait_process_frames(1)


func test_bus_line_tool_places_stops_and_opens_a_line() -> void:
	var roads := Game.simulation.roads
	roads.add_road(_grid_point(-6, 0), _grid_point(6, 0), RoadType.load_by_id(&"local_2lane"))
	await _press_action(&"tool_bus")
	assert_eq(_active_tool_id(), &"BusLineTool")
	var south := Vector2(0, 10)
	var north := Vector2(0, -10)
	for point in [_grid_point(-4, 0) + south, _grid_point(4, 0) + south, _grid_point(4, 0) + north, _grid_point(-4, 0) + north]:
		await _click(point)
	assert_eq(Game.simulation.transit.stops.size(), 4, "a stop on each side at each end")
	assert_eq(Game.simulation.transit.lines.size(), 0, "not open yet")
	await _click(_grid_point(-4, 0) + south)  # The first stop again.
	var transit := Game.simulation.transit
	assert_eq(transit.lines.size(), 1)
	assert_eq(transit.lines.values()[0].stop_ids.size(), 4)
	# Right-clicking a stop removes it; the line goes on through the others.
	await _click(_grid_point(4, 0) + north, MOUSE_BUTTON_RIGHT)
	assert_eq(transit.stops.size(), 3)
	assert_eq(transit.lines.values()[0].stop_ids.size(), 3)


func test_road_tool_builds_a_curve() -> void:
	await _press_action(&"tool_road")
	await _press_action(&"road_curve_mode")
	var tool := _main.get_node("World/Tools/RoadTool") as RoadTool
	assert_true(tool.curved)
	# Drag from the start to the bend point, then click the end: a quarter turn.
	await _mouse_left(_grid_point(-2, -2), true)
	await _mouse_motion(_grid_point(1, -2))
	await _mouse_left(_grid_point(1, -2), false)
	await _click(_grid_point(1, 1))
	var roads := Game.simulation.roads
	assert_eq(roads.segments.size(), 1)
	var segment: RoadSegment = roads.segments.values()[0]
	assert_gt(segment.centerline.point_count, 2, "curved")
	assert_ne(roads.find_node_at(_grid_point(-2, -2)), -1)
	assert_ne(roads.find_node_at(_grid_point(1, 1)), -1)
	assert_eq(roads.find_node_at(_grid_point(1, -2)), -1, "the bend point is no node")
	await _press_action(&"road_curve_mode")
	assert_false(tool.curved, "V again: straight roads")


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
	_debug_button("Town").pressed.emit()
	assert_eq(Game.simulation.externals.get_active_points().size(), 2, "the Town scenario, with its two highways")
	_debug_button("New empty map").pressed.emit()
	assert_eq(Game.simulation.roads.segments.size(), 1, "only the highway stub of a new map")
	assert_eq(Game.simulation.externals.points.size(), 1)


func _palette() -> ToolPalette:
	return _main.get_node("HUD/ToolPalette")


func _active_tool_id() -> StringName:
	var tool := (_main.get_node("World/Tools") as ToolManager).active_tool
	return tool.name if tool != null else &""


func test_palette_zones_family_opens_its_variants() -> void:
	var palette := _palette()
	var submenu := palette.get_submenu("Zones")
	assert_false(submenu.visible, "closed until the family is picked")
	palette.get_family_button("Zones").pressed.emit()
	assert_eq(_active_tool_id(), &"ResidentialZoneTool", "the first zone type by default")
	assert_true(submenu.visible)
	assert_true(palette.get_entry_button("Zones", "Residential").button_pressed)
	palette.get_entry_button("Zones", "Industrial").pressed.emit()
	assert_eq(_active_tool_id(), &"IndustrialZoneTool")
	assert_true(palette.get_entry_button("Zones", "Industrial").button_pressed)
	assert_false(palette.get_entry_button("Zones", "Residential").button_pressed)
	# Another family closes the sub-menu; coming back picks the last-used zone type.
	palette.get_family_button("Bulldoze").pressed.emit()
	assert_false(submenu.visible)
	palette.get_family_button("Zones").pressed.emit()
	assert_eq(_active_tool_id(), &"IndustrialZoneTool")
	# Pressing the open family again puts the tool away.
	palette.get_family_button("Zones").pressed.emit()
	assert_eq(_active_tool_id(), &"")


func test_palette_follows_keyboard_shortcuts() -> void:
	var palette := _palette()
	await _press_action(&"tool_commercial")
	assert_true(palette.get_submenu("Zones").visible)
	assert_true(palette.get_entry_button("Zones", "Commercial").button_pressed)
	assert_true(palette.get_family_button("Zones").button_pressed)
	await _press_action(&"tool_road")
	assert_false(palette.get_submenu("Zones").visible)
	assert_true(palette.get_family_button("Roads").button_pressed)


func test_palette_builds_roads_of_the_picked_type() -> void:
	var palette := _palette()
	var types := RoadType.list_all()
	assert_false(types.is_empty())
	palette.get_family_button("Roads").pressed.emit()
	assert_true(palette.get_submenu("Roads").visible, "road types to pick from")
	var picked: RoadType = types.back()
	palette.get_entry_button("Roads", picked.display_name).pressed.emit()
	assert_eq((_main.get_node("World/Tools/RoadTool") as RoadTool).road_type, picked)
	await _drag(_grid_point(-2, 0), _grid_point(2, 0))
	var segments := Game.simulation.roads.segments.values()
	assert_eq(segments.size(), 1)
	assert_eq(segments[0].road_type, picked)


func test_road_shortcut_again_cycles_road_types() -> void:
	var palette := _palette()
	var tool: RoadTool = _main.get_node("World/Tools/RoadTool")
	var types := RoadType.list_all()
	await _press_action(&"tool_road")
	assert_eq(tool.road_type, types[0])
	await _press_action(&"tool_road")
	assert_eq(_active_tool_id(), &"RoadTool", "still building roads")
	assert_eq(tool.road_type, types[1], "next road type")
	assert_true(palette.get_entry_button("Roads", types[1].display_name).button_pressed)
	assert_string_contains(palette.get_node("HintLabel").text, types[1].display_name)
	await _press_action(&"tool_cancel")
	assert_eq(_active_tool_id(), &"")


func test_long_top_bar_text_keeps_the_speed_buttons_on_screen() -> void:
	var stats: Label = _main.get_node("HUD").find_child("StatsLabel", true, false)
	stats.text = "999 vehicles (+999 in driveways) · 999 stuck · tick 99.9 ms · sim at 1.2× (can't keep up) and more"
	await wait_process_frames(3)
	var buttons: Control = _main.get_node("HUD").find_child("SpeedButtons", true, false)
	assert_lte(buttons.get_global_rect().end.x, float(_viewport.size.x), "speed buttons inside the window")
	assert_true(stats.get_global_rect().end.x <= buttons.get_global_rect().position.x + 0.5, "the text gives way")


func test_hint_reads_above_the_toolbar() -> void:
	var palette := _palette()
	palette.get_family_button("Zones").pressed.emit()
	await wait_process_frames(3)
	var hint: Label = palette.get_node("HintLabel")
	assert_true(hint.visible)
	assert_ne(hint.text, "")
	var toolbar: Control = palette.get_node("Toolbar")
	var submenu := palette.get_submenu("Zones")
	assert_lte(submenu.get_global_rect().end.y, toolbar.get_global_rect().position.y + 0.5, "sub-menu above the toolbar")
	assert_lte(hint.get_global_rect().end.y, submenu.get_global_rect().position.y + 0.5, "hint above the sub-menu")
	assert_almost_eq(submenu.get_global_rect().position.x, palette.get_family_button("Zones").get_global_rect().position.x, 1.0,
		"sub-menu lined up with its family button")


func test_debug_menu_folds_away() -> void:
	var menu: DebugMenu = _main.get_node("HUD/DebugMenu")
	var body: Control = menu.get_child(0).get_child(0).get_child(1)
	assert_true(body.visible)
	await _press_action(&"toggle_debug_menu")
	assert_false(body.visible)
	await _press_action(&"toggle_debug_menu")
	assert_true(body.visible)
