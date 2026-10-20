class_name IntersectionRenderer
extends Node2D
## Draws traffic lights: a bar at each stop line of a light-controlled junction, coloured
## by the light that lane currently has.

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
	for node: RoadNode in Game.simulation.roads.nodes.values():
		if node.control == RoadNode.JunctionControl.TRAFFIC_LIGHT:
			_has_lights = true
	queue_redraw()


func _process(_delta: float) -> void:
	# Lights change with sim time, not only with the network.
	if _has_lights:
		queue_redraw()


func _draw() -> void:
	var roads := Game.simulation.roads
	for node: RoadNode in roads.nodes.values():
		if node.control == RoadNode.JunctionControl.NONE:
			continue
		for segment_id in node.segment_ids:
			var segment := roads.segments[segment_id]
			for lane_id in segment.lane_ids:
				var lane := roads.lanes[lane_id]
				if lane.to_node_id != node.id:
					continue
				var color: Color = LIGHT_COLORS[Game.simulation.signals.get_light(node, lane.signal_phase)]
				var pose := lane.sample_pose(lane.length)
				var across := pose.y.normalized() * segment.road_type.lane_width * 0.5
				draw_line(pose.origin - across, pose.origin + across, color, BAR_WIDTH)
