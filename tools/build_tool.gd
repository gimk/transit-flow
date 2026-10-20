class_name BuildTool
extends Node2D
## Base class for player build tools. Lives in world space so previews can draw with
## _draw(). Only the active tool processes and is visible; ToolManager feeds it input.

var is_active := false


func _ready() -> void:
	deactivate()


func activate() -> void:
	is_active = true
	visible = true
	set_process(true)


func deactivate() -> void:
	cancel()
	is_active = false
	visible = false
	set_process(false)
	Events.tool_hint_changed.emit("")


## Return true if the event was consumed.
func handle_input(_event: InputEvent) -> bool:
	return false


## Abort any in-progress action. Return true if there was one to abort.
func cancel() -> bool:
	return false


## Pointer position in world space. Tools must use this instead of get_global_mouse_position().
func get_pointer_position() -> Vector2:
	return (get_parent() as ToolManager).get_pointer_world_position()


## Pointer position snapped to the grid and kept inside the map.
func get_snapped_pointer_position() -> Vector2:
	var rect := Grid.map_rect()
	return Grid.snap_to_point(get_pointer_position()).clamp(rect.position, rect.end)
