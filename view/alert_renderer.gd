class_name AlertRenderer
extends Node2D
## Problems, under the dot display, at every zoom: what the player should look at. On the
## information layer (Blinking: over everything; it blinks by dimming, which the dot
## display shows as dimmer dots) and, close up, on
## the ground (Marks: square marks, under the vehicles):
## - jams: lanes at least JAM slower than free flow (CongestionMonitor, every 5 s), in red
##   under their queue up close (DETAIL, STREET), blinking from further out;
## - vehicles stuck for ViewSnapshot.STUCK_SECONDS: a blinking ring round each group of
##   them (_stuck_clusters);
## - stops with CROWDED_STOP riders or more waiting: a blinking dot on the sign;
## - zones waiting for a road or for room: a blinking outline (ZoneRenderer's dashed one
##   in the old look).
## Marks far out are at least two dot cells across, so they are found from the city
## view. Hidden in the old look (the congestion heatmap, F4, is there).

## Congestion (extra travel time / free-flow time) from which a lane is jammed.
const JAM := 1.5
const COLOR := Color(1.0, 0.22, 0.12)
## A jammed lane under its queue, up close: a faint tint (DotDisplay's square marks are
## as strong as they're opaque).
const JAM_MARK_ALPHA := 0.6
const JAM_MARK_WIDTH := 6.0
const STUCK_RADIUS := 9.0
const CROWDED_STOP := 8
const REDRAW_INTERVAL := 0.5  ## Real seconds.
## Blinks this many times a second, lit this share of the time, else dimmed to BLINK_DIM.
const BLINK_RATE := 1.25
const BLINK_ON := 0.6
const BLINK_DIM := Color(0.6, 0.6, 0.6)

var _blinking := Node2D.new()
var _marks := Node2D.new()
var _since_redraw := 0.0


func _ready() -> void:
	_blinking.name = "Blinking"
	_blinking.draw.connect(_draw_blinking)
	add_child(_blinking)
	_marks.name = "Marks"
	_marks.draw.connect(_draw_marks)
	add_child(_marks)
	Events.dot_band_changed.connect(_on_band_changed)


func _on_band_changed(_band: int) -> void:
	_redraw()


func _process(delta: float) -> void:
	visible = DotDisplay.current_cell > 0.0
	var lit := fmod(Time.get_ticks_msec() / 1000.0 * BLINK_RATE, 1.0) < BLINK_ON
	_blinking.modulate = Color.WHITE if lit else BLINK_DIM
	_since_redraw += delta
	if visible and _since_redraw >= REDRAW_INTERVAL:
		_redraw()


func _redraw() -> void:
	_since_redraw = 0.0
	_blinking.queue_redraw()
	_marks.queue_redraw()


## Lanes jammed now, as polylines.
func _jammed_lanes() -> Array[PackedVector2Array]:
	var lanes: Array[PackedVector2Array] = []
	var roads := Game.roads
	var congestion := Game.view_to.congestion
	for lane_id: int in congestion:
		if congestion[lane_id] < JAM:
			continue
		var lane: Lane = roads.lanes.get(lane_id)
		if lane != null:
			lanes.append(LaneBuilder.get_polyline(lane.curve))
	return lanes


## Stuck vehicles grouped by squares of `size` px: one ring per group, at its middle. A
## gridlock is a few rings, not hundreds (which would also overflow one canvas item).
static func _stuck_clusters(spots: PackedVector2Array, size: float) -> PackedVector2Array:
	var sums: Dictionary[Vector2i, Vector3] = {}
	for spot in spots:
		var key := Vector2i((spot / size).floor())
		var sum: Vector3 = sums.get(key, Vector3.ZERO)
		sums[key] = sum + Vector3(spot.x, spot.y, 1.0)
	var clusters := PackedVector2Array()
	for sum: Vector3 in sums.values():
		clusters.append(Vector2(sum.x, sum.y) / sum.z)
	return clusters


func _close_up() -> bool:
	return DotDisplay.current_band <= DotDisplay.Band.STREET


func _draw_marks() -> void:
	if not _close_up():
		return
	var width := maxf(JAM_MARK_WIDTH, DotDisplay.current_cell)
	for lane in _jammed_lanes():
		_marks.draw_polyline(lane, Color(COLOR, JAM_MARK_ALPHA), width)


func _draw_blinking() -> void:
	var cell := DotDisplay.current_cell
	var width := maxf(1.5, cell)
	if not _close_up():
		for lane in _jammed_lanes():
			_blinking.draw_polyline(lane, COLOR, 2.0 * cell)
	var view := Game.view_to
	var radius := maxf(STUCK_RADIUS, 2.0 * cell)
	for spot in _stuck_clusters(view.stuck_positions, 3.0 * radius):
		_blinking.draw_arc(spot, radius, 0.0, TAU, 24, COLOR, width)
	for i in view.stop_positions.size():
		if view.stop_waiting[i] >= CROWDED_STOP:
			_blinking.draw_circle(TransitRenderer.sign_spot(view, i), maxf(4.0, cell), COLOR)
	var zones := Game.simulation.zones
	for cell_id: Vector2i in zones.zones:
		if zones.is_waiting_for_road(cell_id):
			var rect := Rect2(Vector2(cell_id) * Grid.CELL_SIZE, Vector2.ONE * Grid.CELL_SIZE).grow(-6.0)
			_blinking.draw_rect(rect, COLOR, false, width)
