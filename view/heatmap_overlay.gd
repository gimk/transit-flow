class_name HeatmapOverlay
extends Node2D
## Congestion view (toggle: F4): road lanes coloured by how much slower than free flow
## traffic moves there, from yellow (slightly slow) to deep red (FULL_RED or worse).
## Free-flowing lanes are not drawn. Data comes from the CongestionMonitor (every 5 s).
## Sits under the vehicles in the scene so queues stay visible.

const REDRAW_INTERVAL := 0.5  ## Real seconds.
## Congestion value (extra travel time / free-flow time) drawn fully red.
const FULL_RED := 4.0
const WIDTH := 6.0

var _since_redraw := 0.0


func _ready() -> void:
	visible = false


func _unhandled_input(event: InputEvent) -> void:
	if event.is_action_pressed("toggle_heatmap"):
		visible = not visible
		queue_redraw()
		get_viewport().set_input_as_handled()


func _process(delta: float) -> void:
	if not visible:
		return
	_since_redraw += delta
	if _since_redraw >= REDRAW_INTERVAL:
		_since_redraw = 0.0
		queue_redraw()


func _draw() -> void:
	var sim := Game.simulation
	for lane_id: int in sim.congestion.congestion:
		var lane: Lane = sim.roads.lanes.get(lane_id)
		if lane == null:
			continue
		var heat := clampf(sim.congestion.congestion[lane_id] / FULL_RED, 0.0, 1.0)
		var color := Color(1.0, 0.85, 0.2).lerp(Color(0.85, 0.1, 0.1), heat)
		color.a = 0.6
		draw_polyline(LaneBuilder.get_polyline(lane.curve), color, WIDTH)
