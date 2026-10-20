class_name LaneOverlay
extends Node2D
## Debug view (toggle: F3): every lane as a thin line with direction arrows,
## and every node as a dot. Forward lanes (start -> end of their segment) are cyan,
## backward lanes orange.

const FORWARD_COLOR := Color(0.3, 0.9, 1.0, 0.9)
const BACKWARD_COLOR := Color(1.0, 0.65, 0.2, 0.9)
const NODE_COLOR := Color(1, 1, 1, 0.8)
const ARROW_SPACING := 32.0
const ARROW_SIZE := 4.0


func _ready() -> void:
	visible = false
	Events.simulation_reset.connect(queue_redraw)
	Events.road_network_changed.connect(func(_nodes: Array, _segments: Array) -> void: queue_redraw())


func _unhandled_input(event: InputEvent) -> void:
	if event.is_action_pressed("toggle_lane_overlay"):
		visible = not visible
		queue_redraw()
		get_viewport().set_input_as_handled()


func _draw() -> void:
	var roads := Game.simulation.roads
	for lane: Lane in roads.lanes.values():
		var segment := roads.segments[lane.segment_id]
		var color := FORWARD_COLOR if lane.from_node_id == segment.start_node_id else BACKWARD_COLOR
		draw_polyline(LaneBuilder.get_polyline(lane.curve), color, 1.0, true)
		var distance := ARROW_SPACING * 0.5
		while distance < lane.length:
			var t := lane.curve.sample_baked_with_rotation(distance)
			var forward := t.x.normalized() * ARROW_SIZE
			var side := t.y.normalized() * ARROW_SIZE
			draw_colored_polygon(PackedVector2Array([
				t.origin + forward,
				t.origin - forward + side,
				t.origin - forward - side,
			]), color)
			distance += ARROW_SPACING
	for node: RoadNode in roads.nodes.values():
		draw_circle(node.position, 3.0, NODE_COLOR)
