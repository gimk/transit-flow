class_name RoadMarkings
extends Node2D
## Paint on one road segment: the center line between directions (dashed on local
## streets, solid on wider roads), dashed lines between lanes going the same way,
## arrows along one-way roads and zebra crossings at junctions. Stops short of
## junctions, where the connectors are.

const LINE_COLOR := Color(1, 1, 1, 0.45)
const CENTER_COLOR := Color(1.0, 0.85, 0.35, 0.6)
const ARROW_COLOR := Color(1, 1, 1, 0.5)
const LINE_WIDTH := 1.5
const DASH := 6.0
const ARROW_SPACING := 96.0
const ARROW_SIZE := 4.0
const ZEBRA_COLOR := Color(1, 1, 1, 0.35)
const ZEBRA_LENGTH := 6.0
const ZEBRA_WIDTH := 2.5
const ZEBRA_SPACING := 5.0

var _points: PackedVector2Array
var _road_type: RoadType
var _start_trim := 0.0
var _end_trim := 0.0
## Crosswalks (at junctions) across the road at its start / end.
var _zebra_at_start := false
var _zebra_at_end := false


func setup(points: PackedVector2Array, road_type: RoadType, start_trim: float, end_trim: float,
		zebra_at_start := false, zebra_at_end := false) -> void:
	_points = points
	_road_type = road_type
	_start_trim = start_trim
	_end_trim = end_trim
	_zebra_at_start = zebra_at_start
	_zebra_at_end = zebra_at_end
	queue_redraw()


func _draw() -> void:
	if _zebra_at_start:
		_draw_zebra(_points, _start_trim)
	if _zebra_at_end:
		var reversed := _points.duplicate()
		reversed.reverse()
		_draw_zebra(reversed, _end_trim)
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


## Zebra stripes across the road, `at` along `points` from its first point (where
## pedestrians cross: see WalkNetwork).
func _draw_zebra(points: PackedVector2Array, at: float) -> void:
	var center := _point_along(points, at)
	var along := (_point_along(points, at + 1.0) - center).normalized()
	var across := along.orthogonal()
	var half_width := _road_type.get_total_width() * 0.5
	var offset := -half_width + ZEBRA_SPACING * 0.5
	while offset < half_width:
		var stripe := center + across * offset
		draw_line(stripe - along * ZEBRA_LENGTH * 0.5, stripe + along * ZEBRA_LENGTH * 0.5, ZEBRA_COLOR, ZEBRA_WIDTH)
		offset += ZEBRA_SPACING


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
