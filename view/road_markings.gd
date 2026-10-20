class_name RoadMarkings
extends Node2D
## Paint on one road segment: the center line between directions (dashed on local
## streets, solid on wider roads), dashed lines between lanes going the same way, and
## arrows along one-way roads. Stops short of junctions, where the connectors are.

const LINE_COLOR := Color(1, 1, 1, 0.45)
const CENTER_COLOR := Color(1.0, 0.85, 0.35, 0.6)
const ARROW_COLOR := Color(1, 1, 1, 0.5)
const LINE_WIDTH := 1.5
const DASH := 6.0
const ARROW_SPACING := 96.0
const ARROW_SIZE := 4.0

var _points: PackedVector2Array
var _road_type: RoadType
var _start_trim := 0.0
var _end_trim := 0.0


func setup(points: PackedVector2Array, road_type: RoadType, start_trim: float, end_trim: float) -> void:
	_points = points
	_road_type = road_type
	_start_trim = start_trim
	_end_trim = end_trim
	queue_redraw()


func _draw() -> void:
	var lane_width := _road_type.lane_width
	if _road_type.one_way:
		var half_width := _road_type.get_total_width() * 0.5
		for k in range(1, _road_type.lanes_per_direction):
			_dashed(_line_at(k * lane_width - half_width), LINE_COLOR)
		for index in _road_type.lanes_per_direction:
			draw_arrows(self, _line_at((index + 0.5) * lane_width - half_width), ARROW_COLOR)
		return
	var center := _line_at(0.0)
	if _road_type.lanes_per_direction == 1:
		_dashed(center, CENTER_COLOR)
	elif center.size() >= 2:
		draw_polyline(center, CENTER_COLOR, LINE_WIDTH)
	for k in range(1, _road_type.lanes_per_direction):
		_dashed(_line_at(k * lane_width), LINE_COLOR)
		_dashed(_line_at(-k * lane_width), LINE_COLOR)


## The centerline shifted `offset` to the right of the segment's direction, trimmed.
func _line_at(offset: float) -> PackedVector2Array:
	return LaneBuilder.trim_polyline(LaneBuilder.offset_polyline(_points, offset), _start_trim, _end_trim)


func _dashed(points: PackedVector2Array, color: Color) -> void:
	for i in points.size() - 1:
		draw_dashed_line(points[i], points[i + 1], color, LINE_WIDTH, DASH)


## Chevrons pointing along `points`, every ARROW_SPACING (also used by the road tool's
## preview of a one-way road).
static func draw_arrows(canvas: CanvasItem, points: PackedVector2Array, color: Color) -> void:
	var length := LaneBuilder.polyline_length(points)
	if length <= 0.0:
		return
	var count := maxi(1, floori(length / ARROW_SPACING))
	var step := length / count
	for i in count:
		var at := step * (i + 0.5)
		var position := _point_along(points, at)
		var direction := (_point_along(points, at + 1.0) - position).normalized()
		if direction == Vector2.ZERO:
			direction = (_point_along(points, at) - _point_along(points, at - 1.0)).normalized()
		var side := direction.orthogonal() * ARROW_SIZE
		var tip := position + direction * ARROW_SIZE
		canvas.draw_polyline(PackedVector2Array([tip - direction * ARROW_SIZE * 2.0 + side, tip, tip - direction * ARROW_SIZE * 2.0 - side]), color, LINE_WIDTH)


static func _point_along(points: PackedVector2Array, distance: float) -> Vector2:
	distance = clampf(distance, 0.0, LaneBuilder.polyline_length(points))
	for i in points.size() - 1:
		var piece := points[i].distance_to(points[i + 1])
		if distance <= piece:
			return points[i].lerp(points[i + 1], distance / piece if piece > 0.0 else 0.0)
		distance -= piece
	return points[-1]
