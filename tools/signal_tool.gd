class_name SignalTool
extends BuildTool
## Click a junction where 3+ roads meet to toggle traffic lights on or off (without them
## traffic takes turns first come, first served).

const PICK_DISTANCE := 24.0
const HIGHLIGHT_COLOR := Color(1, 1, 1, 0.9)
const CONTROL_NAMES := {
	RoadNode.JunctionControl.NONE: "No control (first come, first served)",
	RoadNode.JunctionControl.TRAFFIC_LIGHT: "Traffic lights (directions alternate)",
}
const IDLE_HINT := "Click a junction (3+ roads) to toggle traffic lights"

var _hovered_node_id := -1


func activate() -> void:
	super()
	_hovered_node_id = -1
	_update_hint()


func _process(_delta: float) -> void:
	var node_id := _find_junction(get_pointer_position())
	if node_id != _hovered_node_id:
		_hovered_node_id = node_id
		_update_hint()
		queue_redraw()


func handle_input(event: InputEvent) -> bool:
	var mouse_button := event as InputEventMouseButton
	if mouse_button == null or not mouse_button.pressed or mouse_button.button_index != MOUSE_BUTTON_LEFT:
		return false
	var roads := Game.simulation.roads
	var node: RoadNode = roads.nodes.get(_find_junction(get_pointer_position()))
	if node != null:
		var next := ((node.control + 1) % RoadNode.JunctionControl.size()) as RoadNode.JunctionControl
		roads.set_node_control(node.id, next)
		_update_hint()
	return true


func _find_junction(position: Vector2) -> int:
	var best_id := -1
	var best_distance := PICK_DISTANCE
	for node: RoadNode in Game.simulation.roads.nodes.values():
		var distance := node.position.distance_to(position)
		if node.get_degree() >= 3 and distance <= best_distance:
			best_id = node.id
			best_distance = distance
	return best_id


func _update_hint() -> void:
	var node: RoadNode = Game.simulation.roads.nodes.get(_hovered_node_id)
	if node == null:
		Events.tool_hint_changed.emit(IDLE_HINT)
	else:
		Events.tool_hint_changed.emit("%s · click to change" % CONTROL_NAMES[node.control])


func _draw() -> void:
	var node: RoadNode = Game.simulation.roads.nodes.get(_hovered_node_id)
	if node != null:
		draw_arc(node.position, PICK_DISTANCE, 0.0, TAU, 32, HIGHLIGHT_COLOR, 2.0)
