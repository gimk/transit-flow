class_name RouteTool
extends BuildTool
## Debug inspector: click a road to set an origin, click another to set a destination,
## and the quickest route between them is drawn. Clicking again starts over.
## Positions snap to the nearest lane, so clicking either side of a road picks its direction.

const PICK_DISTANCE := 18.0
const ROUTE_COLOR := Color(1.0, 0.85, 0.2, 0.85)
const ORIGIN_COLOR := Color(0.35, 0.9, 0.45)
const DESTINATION_COLOR := Color(1.0, 0.4, 0.35)
const IDLE_HINT := "Click a road to set the route origin"

var _origin := {}
var _destination := {}
var _route: Route


func activate() -> void:
	super()
	_clear()


func handle_input(event: InputEvent) -> bool:
	var mouse_button := event as InputEventMouseButton
	if mouse_button == null or not mouse_button.pressed:
		return false
	if mouse_button.button_index == MOUSE_BUTTON_RIGHT:
		return cancel()
	if mouse_button.button_index != MOUSE_BUTTON_LEFT:
		return false

	var picked := Game.simulation.roads.find_lane_position(get_pointer_position(), PICK_DISTANCE)
	if picked.is_empty():
		return true
	if _origin.is_empty() or not _destination.is_empty():
		_clear()
		_origin = picked
		Events.tool_hint_changed.emit("Click a road to set the destination")
	else:
		_destination = picked
		_compute_route()
	queue_redraw()
	return true


func cancel() -> bool:
	if _origin.is_empty():
		return false
	_clear()
	return true


func _process(_delta: float) -> void:
	# The network may have changed under the displayed route; recompute it if so.
	if _route != null and not _route_still_valid():
		_compute_route()
		queue_redraw()


func _compute_route() -> void:
	var roads := Game.simulation.roads
	if not (roads.lanes.has(_origin.lane_id) and roads.lanes.has(_destination.lane_id)):
		_clear()
		return
	_route = Game.simulation.router.find_route(_origin.lane_id, _origin.distance, _destination.lane_id, _destination.distance)
	if _route == null:
		Events.tool_hint_changed.emit("No route between these points")
	else:
		Events.tool_hint_changed.emit("Route: %d lanes · %.1f s at free flow" % [_route.lane_ids.size(), _route.travel_time])


func _route_still_valid() -> bool:
	for lane_id in _route.lane_ids:
		if not Game.simulation.roads.lanes.has(lane_id):
			return false
	return true


func _clear() -> void:
	_origin = {}
	_destination = {}
	_route = null
	Events.tool_hint_changed.emit(IDLE_HINT)
	queue_redraw()


func _draw() -> void:
	var roads := Game.simulation.roads
	if _route != null:
		draw_polyline(_route.get_polyline(roads), ROUTE_COLOR, 5.0, true)
	for entry: Array in [[_origin, ORIGIN_COLOR], [_destination, DESTINATION_COLOR]]:
		var picked: Dictionary = entry[0]
		if not picked.is_empty() and roads.lanes.has(picked.lane_id):
			draw_circle(roads.lanes[picked.lane_id].curve.sample_baked(picked.distance), 6.0, entry[1])
