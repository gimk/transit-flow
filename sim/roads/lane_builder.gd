class_name LaneBuilder
extends RefCounted
## Builds lane curves from a segment's centerline.
## Lanes are offset *polylines*, not offset Bézier control points: an offset Bézier
## is not a Bézier, while sampling keeps straight and curved roads on one code path.
## Traffic drives on the right: lanes sit to the right of their direction of travel.

const DUPLICATE_POINT_EPSILON := 0.01
## Caps how far a corner point may be pushed out, so sharp bends don't produce spikes.
const MIN_MITRE_COSINE := 0.25


## Returns one entry per lane: { "forward": bool, "index": int, "curve": Curve2D }.
## "forward" lanes run from the centerline's first point to its last.
static func build_lane_specs(centerline: Curve2D, road_type: RoadType) -> Array[Dictionary]:
	var forward_points := get_polyline(centerline)
	var backward_points := forward_points.duplicate()
	backward_points.reverse()
	var specs: Array[Dictionary] = []
	for index in road_type.lanes_per_direction:
		var offset := (index + 0.5) * road_type.lane_width
		specs.append({
			"forward": true,
			"index": index,
			"curve": polyline_to_curve(offset_polyline(forward_points, offset)),
		})
		specs.append({
			"forward": false,
			"index": index,
			"curve": polyline_to_curve(offset_polyline(backward_points, offset)),
		})
	return specs


## The curve's shape as a polyline: its control points when it has no handles
## (exact for grid roads), its baked points otherwise.
static func get_polyline(curve: Curve2D) -> PackedVector2Array:
	var source: PackedVector2Array
	if _has_handles(curve):
		source = curve.get_baked_points()
	else:
		for i in curve.point_count:
			source.append(curve.get_point_position(i))
	var points := PackedVector2Array()
	for point in source:
		if points.is_empty() or points[-1].distance_to(point) > DUPLICATE_POINT_EPSILON:
			points.append(point)
	return points


## Shifts every point sideways by `offset`; positive = right of the direction of travel.
static func offset_polyline(points: PackedVector2Array, offset: float) -> PackedVector2Array:
	var result := PackedVector2Array()
	var count := points.size()
	for i in count:
		var normal: Vector2
		if i == 0:
			normal = _right_normal(points[0], points[1])
		elif i == count - 1:
			normal = _right_normal(points[i - 1], points[i])
		else:
			var normal_in := _right_normal(points[i - 1], points[i])
			var normal_out := _right_normal(points[i], points[i + 1])
			var mitre := (normal_in + normal_out).normalized()
			normal = mitre / maxf(mitre.dot(normal_in), MIN_MITRE_COSINE)
		result.append(points[i] + normal * offset)
	return result


static func polyline_to_curve(points: PackedVector2Array) -> Curve2D:
	var curve := Curve2D.new()
	for point in points:
		curve.add_point(point)
	return curve


static func distance_to_polyline(point: Vector2, polyline: PackedVector2Array) -> float:
	var best := INF
	for i in polyline.size() - 1:
		var closest := Geometry2D.get_closest_point_to_segment(point, polyline[i], polyline[i + 1])
		best = minf(best, closest.distance_to(point))
	return best


static func _has_handles(curve: Curve2D) -> bool:
	for i in curve.point_count:
		if curve.get_point_in(i) != Vector2.ZERO or curve.get_point_out(i) != Vector2.ZERO:
			return true
	return false


## In y-down screen space, the right-hand side of travel direction d is (-d.y, d.x).
static func _right_normal(from: Vector2, to: Vector2) -> Vector2:
	var direction := (to - from).normalized()
	return Vector2(-direction.y, direction.x)
