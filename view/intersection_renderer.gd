class_name IntersectionRenderer
extends Node2D
## Draws traffic lights: a bar at each stop line of a light-controlled junction, coloured
## by the best light any of that lane's turns has (with protected left turns, the left
## lane of a road can be green for going straight and red for turning).
## At least a dot cell thick under the dot display, so they read at any zoom.

const LIGHT_COLORS := {
	TrafficLights.Light.GREEN: Color("43a047"),
	TrafficLights.Light.AMBER: Color("fdd835"),
	TrafficLights.Light.RED: Color("e53935"),
}
const BAR_WIDTH := 3.0

var _has_lights := false


func _ready() -> void:
	Events.simulation_reset.connect(_refresh)
	Events.road_network_changed.connect(func(_nodes: Array, _segments: Array) -> void: _refresh())
	_refresh()


func _refresh() -> void:
	_has_lights = false
	for node: RoadNode in Game.roads.nodes.values():
		if node.has_lights():
			_has_lights = true
	queue_redraw()


func _process(_delta: float) -> void:
	# Lights change with sim time, not only with the network.
	if _has_lights:
		queue_redraw()


func _draw() -> void:
	var roads := Game.roads
	for node: RoadNode in roads.nodes.values():
		if not node.has_lights():
			continue
		for segment_id in node.segment_ids:
			var segment := roads.segments[segment_id]
			for lane_id in segment.lane_ids:
				var lane := roads.lanes[lane_id]
				if lane.to_node_id != node.id:
					continue
				var color: Color = LIGHT_COLORS[_lane_light(node, lane)]
				var pose := lane.sample_pose(lane.length)
				var across := pose.y.normalized() * segment.road_type.lane_width * 0.5
				draw_line(pose.origin - across, pose.origin + across, color, maxf(BAR_WIDTH, DotDisplay.current_cell))


func _lane_light(node: RoadNode, lane: Lane) -> TrafficLights.Light:
	var best := TrafficLights.Light.RED
	for connector in lane.outgoing_lanes:
		best = mini(best, Game.view_to.get_light(node.id, connector.signal_phase)) as TrafficLights.Light
	return best
