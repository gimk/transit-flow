class_name ToolManager
extends Node2D
## Routes world input to the active build tool. Tools are BuildTool children;
## a tool's id is its node name. Selecting the active tool again turns it off, unless
## it has variants to cycle through (pressing R again picks the next road type).

## Input action -> tool id.
const TOOL_SHORTCUTS := {
	&"tool_road": &"RoadTool",
	&"tool_bulldoze": &"BulldozeTool",
	&"tool_route": &"RouteTool",
	&"tool_residential": &"ResidentialZoneTool",
	&"tool_commercial": &"CommercialZoneTool",
	&"tool_industrial": &"IndustrialZoneTool",
	&"tool_dezone": &"DezoneTool",
	&"tool_signals": &"SignalTool",
}

var active_tool: BuildTool = null
## Last pointer position in viewport coordinates. Tracked from input events rather
## than queried from the OS, so tools also respond to synthetic input in tests.
var pointer_screen_position := Vector2.ZERO


func _ready() -> void:
	Events.tool_requested.connect(select_tool)
	Events.tool_variant_requested.connect(use_tool)


## Makes `tool_id` the active tool (without toggling it off) in `variant`, if given.
func use_tool(tool_id: StringName, variant: StringName = &"") -> void:
	var tool := get_node_or_null(NodePath(tool_id)) as BuildTool
	if tool == null:
		return
	if tool != active_tool:
		select_tool(tool_id)
	if variant != &"":
		tool.set_variant(variant)


func _input(event: InputEvent) -> void:
	if event is InputEventMouse:
		pointer_screen_position = event.position


func get_pointer_world_position() -> Vector2:
	return get_viewport().get_canvas_transform().affine_inverse() * pointer_screen_position


func select_tool(tool_id: StringName) -> void:
	var next: BuildTool = null
	if tool_id != &"":
		next = get_node_or_null(NodePath(tool_id)) as BuildTool
	if next == active_tool:
		next = null
	if active_tool != null:
		active_tool.deactivate()
	active_tool = next
	if active_tool != null:
		active_tool.activate()
	Events.tool_changed.emit(active_tool.name if active_tool != null else &"")


func _unhandled_input(event: InputEvent) -> void:
	for action: StringName in TOOL_SHORTCUTS:
		if event.is_action_pressed(action):
			# The active tool's shortcut cycles its variants (road types); otherwise it
			# toggles the tool.
			var tool_id: StringName = TOOL_SHORTCUTS[action]
			if active_tool == null or active_tool.name != tool_id or not active_tool.cycle_variant():
				select_tool(tool_id)
			get_viewport().set_input_as_handled()
			return
	if event.is_action_pressed("tool_cancel"):
		# First Escape aborts the current drag, the second one puts the tool away.
		if active_tool == null or not active_tool.cancel():
			select_tool(&"")
	elif active_tool == null or not active_tool.handle_input(event):
		return
	get_viewport().set_input_as_handled()
