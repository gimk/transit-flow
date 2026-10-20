class_name LaneBuilder
extends RefCounted
## Lane geometry: road lanes from a segment's centerline, and connector curves
## through junctions.
## Road lanes are offset *polylines*, not offset Bézier control points: an offset Bézier
## is not a Bézier, while sampling keeps straight and curved roads on one code path.
## Traffic drives on the right: lanes sit to the right of their direction of travel.

const DUPLICATE_POINT_EPSILON := 0.01
## Caps how far a corner point may be pushed out, so sharp bends don't produce spikes.
const MIN_MITRE_COSINE := 0.25
## Trimming never shortens a lane below this, even on very short segments.
const MIN_TRIMMED_LENGTH := 4.0


## Returns one entry per lane: { "forward": bool, "index": int, "curve": Curve2D }.
## "forward" lanes run from the centerline's first point to its last.
## `start_trim` / `end_trim` cut lanes back from the segment's start / end node,
## leaving room for the junction's connector lanes.
static func build_lane_specs(centerline: Curve2D, road_type: RoadType,
		start_trim := 0.0, end_trim := 0.0) -> Array[Dictionary]:
	var forward_points := get_polyline(centerline)
	var backward_points := forward_points.duplicate()
	backward_points.reverse()
	var specs: Array[Dictionary] = []
	for index in road_type.lanes_per_direction:
		var offset := (index + 0.5) * road_type.lane_width
		var forward := trim_polyline(offset_polyline(forward_points, offset), start_trim, end_trim)
		var backward := trim_polyline(offset_polyline(backward_points, offset), end_trim, start_trim)
		specs.append({ "forward": true, "index": index, "curve": polyline_to_curve(forward) })
		specs.append({ "forward": false, "index": index, "curve": polyline_to_curve(backward) })
	return specs


## Smooth curve from the end of `from_curve` to the start of `to_curve`, leaving and
## arriving tangent to both. Turns aim both handles at the point where the two lanes'
## lines meet (a quadratic curve expressed as a cubic); straight continuations stay
## straight; U-turns become a near-semicircle.
static func build_connector_curve(from_curve: Curve2D, to_curve: Curve2D) -> Curve2D:
	var from_points := get_polyline(from_curve)
	var to_points := get_polyline(to_curve)
	var p0 := from_points[-1]
	var p1 := to_points[0]
	var d0 := (from_points[-1] - from_points[-2]).normalized()
	var d1 := (to_points[1] - to_points[0]).normalized()
	var chord := p0.distance_to(p1)

	var h0 := chord * 0.5
	var h1 := chord * 0.5
	var denominator := d0.cross(d1)
	if absf(denominator) > 0.0001:
		# Solve p0 + t·d0 = p1 - s·d1 for the corner point of the turn.
		var w := p1 - p0
		var t := w.cross(d1) / denominator
		var s := -w.cross(d0) / denominator
		if t > 0.0 and s > 0.0:
			h0 = t * 2.0 / 3.0
			h1 = s * 2.0 / 3.0
	elif d0.dot(d1) > 0.0:
		h0 = 0.0
		h1 = 0.0
	else:
		# Cubic approximation of a semicircle of radius chord / 2.
		h0 = chord * 2.0 / 3.0
		h1 = h0

	var curve := Curve2D.new()
	curve.add_point(p0, Vector2.ZERO, d0 * h0)
	curve.add_point(p1, -d1 * h1, Vector2.ZERO)
	return curve


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


## Removes `start_trim` px from the start and `end_trim` px from the end. If the
## polyline is too short for both, the trims shrink proportionally.
static func trim_polyline(points: PackedVector2Array, start_trim: float, end_trim: float) -> PackedVector2Array:
	var length := polyline_length(points)
	var available := maxf(length - MIN_TRIMMED_LENGTH, 0.0)
	if start_trim + end_trim > available:
		var scale := available / (start_trim + end_trim)
		start_trim *= scale
		end_trim *= scale
	return sub_polyline(points, start_trim, length - end_trim)


## The part of the polyline between two distances measured along it.
static func sub_polyline(points: PackedVector2Array, from_distance: float, to_distance: float) -> PackedVector2Array:
	var result := PackedVector2Array()
	var walked := 0.0
	for i in points.size() - 1:
		var a := points[i]
		var b := points[i + 1]
		var piece := a.distance_to(b)
		var next_walked := walked + piece
		if next_walked >= from_distance and walked <= to_distance and piece > 0.0:
			if result.is_empty():
				result.append(a.lerp(b, clampf((from_distance - walked) / piece, 0.0, 1.0)))
			if next_walked >= to_distance:
				result.append(a.lerp(b, clampf((to_distance - walked) / piece, 0.0, 1.0)))
				break
			result.append(b)
		walked = next_walked
	return result


static func polyline_length(points: PackedVector2Array) -> float:
	var length := 0.0
	for i in points.size() - 1:
		length += points[i].distance_to(points[i + 1])
	return length


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


## Unit direction of travel at the start / end of a lane curve.
static func start_direction(curve: Curve2D) -> Vector2:
	var points := get_polyline(curve)
	return (points[1] - points[0]).normalized()


static func end_direction(curve: Curve2D) -> Vector2:
	var points := get_polyline(curve)
	return (points[-1] - points[-2]).normalized()


static func _has_handles(curve: Curve2D) -> bool:
	for i in curve.point_count:
		if curve.get_point_in(i) != Vector2.ZERO or curve.get_point_out(i) != Vector2.ZERO:
			return true
	return false


## In y-down screen space, the right-hand side of travel direction d is (-d.y, d.x).
static func _right_normal(from: Vector2, to: Vector2) -> Vector2:
	var direction := (to - from).normalized()
	return Vector2(-direction.y, direction.x)
