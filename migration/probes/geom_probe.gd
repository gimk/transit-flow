extends SceneTree
## Prints Geometry2D results for fixed and random segments as JSON (for tf_sim tests).
func _v(v: Vector2) -> Array:
	return [v.x, v.y]
func _init() -> void:
	var rng := RandomNumberGenerator.new()
	rng.seed = 42
	var cases: Array[Array] = [
		[Vector2(0, 0), Vector2(10, 0), Vector2(4, -5), Vector2(4, 5)],
		[Vector2(0, 0), Vector2(10, 0), Vector2(12, -5), Vector2(12, 5)],
		[Vector2(0, 0), Vector2(10, 0), Vector2(0, 1), Vector2(10, 1)],
		[Vector2(0, 0), Vector2(10, 0), Vector2(0, 0), Vector2(10, 0)],
		[Vector2(0, 0), Vector2(10, 0), Vector2(10, 0), Vector2(10, 5)],
		[Vector2(0, 0), Vector2(10, 0), Vector2(5, 0), Vector2(5, 5)],
		[Vector2(3, 3), Vector2(3, 3), Vector2(0, 0), Vector2(5, 5)],
		[Vector2(0, 0), Vector2(64, 64), Vector2(64, 0), Vector2(0, 64)],
	]
	for i in 40:
		var c: Array = []
		for j in 4:
			c.append(Vector2(rng.randf_range(-100, 100), rng.randf_range(-100, 100)))
		cases.append(c)
	var out: Array = []
	for c in cases:
		var hit: Variant = Geometry2D.segment_intersects_segment(c[0], c[1], c[2], c[3])
		var pts := Geometry2D.get_closest_points_between_segments(c[0], c[1], c[2], c[3])
		out.append({
			"segments": [_v(c[0]), _v(c[1]), _v(c[2]), _v(c[3])],
			"intersection": null if hit == null else _v(hit),
			"closest_points": [_v(pts[0]), _v(pts[1])],
			"closest_to_segment": _v(Geometry2D.get_closest_point_to_segment(c[2], c[0], c[1])),
		})
	print("JSON:" + JSON.stringify(out))
	quit()
