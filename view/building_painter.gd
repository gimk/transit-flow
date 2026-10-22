class_name BuildingPainter
extends RefCounted
## Draws one building seen from above, in a style that tells its zone at a glance:
## houses (pitched roofs in a garden with a tree), shops (flat roofs with a striped
## awning over the street side and rooftop units) and factories (sawtooth or ribbed
## roofs, loading doors, sometimes a silo). The style is picked from a per-lot seed, so a
## lot keeps its look across rebuilds and reloads. The sun is in the north-west: lit
## faces are those facing up and left, and shadows fall down and right.

## Shadow offset per unit of building height (world px).
const SHADOW_DIRECTION := Vector2(0.55, 0.75)
const SHADOW_COLOR := Color(0, 0, 0, 0.3)
## Heights (world px, only for shadows).
const HOUSE_HEIGHT := 3.0
const SHOP_HEIGHT := 5.0
const FACTORY_HEIGHT := 6.0

const GARDEN_COLOR := Color("5b8c3e")
const TREE_COLOR := Color("2e6b2a")
const ROOF_COLORS: Array[Color] = [Color("b5523b"), Color("8e3b2f"), Color("6d6f78"), Color("8d6e63"), Color("a0603a")]
const SHOP_ROOF_COLORS: Array[Color] = [Color("c9ced3"), Color("b8c2c8"), Color("d5cfc4")]
const AWNING_COLORS: Array[Color] = [Color("1e88e5"), Color("42a5f5"), Color("26a69a"), Color("5c6bc0")]
const FACTORY_ROOF_COLORS: Array[Color] = [Color("9a9a96"), Color("a39383"), Color("8f979b")]
const FACTORY_ACCENT := Color("f9a825")
const AWNING_DEPTH := 3.0
const STRIPE := 3.0


## Paints the building filling `rect`, whose street side faces `front` (a unit axis vector).
static func paint(canvas: CanvasItem, type: Lot.Type, rect: Rect2, front: Vector2, seed: int) -> void:
	var rng := RandomNumberGenerator.new()
	rng.seed = seed
	match type:
		Lot.Type.RESIDENTIAL:
			_paint_house(canvas, rect, front, rng)
		Lot.Type.COMMERCIAL:
			_paint_shop(canvas, rect, front, rng)
		_:
			_paint_factory(canvas, rect, front, rng)


static func _paint_house(canvas: CanvasItem, rect: Rect2, front: Vector2, rng: RandomNumberGenerator) -> void:
	canvas.draw_rect(rect, GARDEN_COLOR)
	var length := _length(rect, front)
	var depth := _depth(rect, front)
	# The house stands at the front of its garden, somewhere along it; the rest is lawn
	# with a tree when there's room for one.
	var house_length := minf(length, maxf(14.0, length * rng.randf_range(0.5, 0.75)))
	var house_depth := minf(depth, maxf(10.0, depth * rng.randf_range(0.7, 0.95)))
	var start := rng.randf_range(0.0, length - house_length)
	var house := _sub(rect, front, start, 0.0, start + house_length, house_depth)
	var room_left := start
	var room_right := length - start - house_length
	if maxf(room_left, room_right) >= 9.0:
		var gap_start := 0.0 if room_left > room_right else start + house_length
		var gap := maxf(room_left, room_right)
		var spot := _sub(rect, front, gap_start, depth * 0.35, gap_start + gap, depth * 0.85).get_center()
		_paint_tree(canvas, spot, minf(gap, depth) * 0.3)
	_shadow(canvas, house, HOUSE_HEIGHT)
	var color: Color = ROOF_COLORS[rng.randi() % ROOF_COLORS.size()]
	# Ridge along the street, or across it on deep plots; hipped or gabled.
	var ridge_along_x := house.size.x >= house.size.y
	if rng.randf() < 0.25:
		ridge_along_x = not ridge_along_x
	_paint_pitched_roof(canvas, house, ridge_along_x, rng.randf() < 0.5, color)
	if rng.randf() < 0.6:
		# A chimney, on the back slope.
		var chimney := Vector2(2.5, 2.5)
		var at := house.get_center() + Vector2(house.size.x * 0.25, house.size.y * 0.2)
		canvas.draw_rect(Rect2(at - chimney * 0.5, chimney), color.darkened(0.45))


## A roof with two long slopes meeting at a ridge; hipped roofs slope at the ends too.
static func _paint_pitched_roof(canvas: CanvasItem, rect: Rect2, ridge_along_x: bool, hipped: bool, color: Color) -> void:
	var lit := color.lightened(0.18)
	var shaded := color.darkened(0.22)
	var a := rect.position
	var b := rect.end
	if ridge_along_x:
		var mid := rect.get_center().y
		var inset := minf(rect.size.x, rect.size.y) * 0.5 if hipped else 0.0
		var r0 := Vector2(a.x + inset, mid)
		var r1 := Vector2(b.x - inset, mid)
		canvas.draw_colored_polygon(PackedVector2Array([a, Vector2(b.x, a.y), r1, r0]), lit)
		canvas.draw_colored_polygon(PackedVector2Array([Vector2(a.x, b.y), r0, r1, b]), shaded)
		if hipped:
			canvas.draw_colored_polygon(PackedVector2Array([a, r0, Vector2(a.x, b.y)]), color)
			canvas.draw_colored_polygon(PackedVector2Array([Vector2(b.x, a.y), b, r1]), shaded.darkened(0.1))
		canvas.draw_line(r0, r1, color.darkened(0.35), 1.0)
	else:
		var mid := rect.get_center().x
		var inset := minf(rect.size.x, rect.size.y) * 0.5 if hipped else 0.0
		var r0 := Vector2(mid, a.y + inset)
		var r1 := Vector2(mid, b.y - inset)
		canvas.draw_colored_polygon(PackedVector2Array([a, r0, r1, Vector2(a.x, b.y)]), lit)
		canvas.draw_colored_polygon(PackedVector2Array([Vector2(b.x, a.y), b, r1, r0]), shaded)
		if hipped:
			canvas.draw_colored_polygon(PackedVector2Array([a, Vector2(b.x, a.y), r0]), color)
			canvas.draw_colored_polygon(PackedVector2Array([Vector2(a.x, b.y), r1, b]), shaded.darkened(0.1))
		canvas.draw_line(r0, r1, color.darkened(0.35), 1.0)


static func _paint_tree(canvas: CanvasItem, at: Vector2, radius: float) -> void:
	radius = clampf(radius, 3.0, 6.0)
	canvas.draw_circle(at + SHADOW_DIRECTION * 3.0, radius, SHADOW_COLOR)
	canvas.draw_circle(at, radius, TREE_COLOR)
	canvas.draw_circle(at - Vector2(radius, radius) * 0.3, radius * 0.55, TREE_COLOR.lightened(0.15))


static func _paint_shop(canvas: CanvasItem, rect: Rect2, front: Vector2, rng: RandomNumberGenerator) -> void:
	var length := _length(rect, front)
	var depth := _depth(rect, front)
	_shadow(canvas, rect, SHOP_HEIGHT)
	var roof: Color = SHOP_ROOF_COLORS[rng.randi() % SHOP_ROOF_COLORS.size()]
	# Parapet round a flat roof.
	canvas.draw_rect(rect, roof.darkened(0.3))
	canvas.draw_rect(rect.grow(-1.5), roof)
	# Wide lots hold two shops side by side.
	var shops := 2 if length >= 40.0 and rng.randf() < 0.5 else 1
	var shop_length := length / shops
	for i in shops:
		var awning_color: Color = AWNING_COLORS[rng.randi() % AWNING_COLORS.size()]
		var awning_start := i * shop_length + 2.0
		var awning_end := (i + 1) * shop_length - 2.0
		var along := awning_start
		var stripe := 0
		while along < awning_end:
			var piece := _sub(rect, front, along, 0.0, minf(along + STRIPE, awning_end), AWNING_DEPTH)
			canvas.draw_rect(piece, awning_color if stripe % 2 == 0 else Color(0.95, 0.95, 0.92))
			along += STRIPE
			stripe += 1
		if i > 0:
			var wall := _sub(rect, front, i * shop_length - 0.5, 0.0, i * shop_length + 0.5, depth)
			canvas.draw_rect(wall, roof.darkened(0.3))
		# Rooftop units toward the back.
		var units := rng.randi_range(1, 2)
		for u in units:
			var spot := _sub(rect, front, i * shop_length + shop_length * (u + 1) / (units + 1) - 2.0, depth * 0.55,
				i * shop_length + shop_length * (u + 1) / (units + 1) + 2.0, depth * 0.55 + 4.0)
			if spot.size.x < 1.0 or spot.size.y < 1.0 or depth < 12.0:
				continue
			canvas.draw_rect(spot.grow(0.5), roof.darkened(0.45))
			canvas.draw_rect(spot, roof.lightened(0.35))


static func _paint_factory(canvas: CanvasItem, rect: Rect2, front: Vector2, rng: RandomNumberGenerator) -> void:
	var length := _length(rect, front)
	var depth := _depth(rect, front)
	_shadow(canvas, rect, FACTORY_HEIGHT)
	var roof: Color = FACTORY_ROOF_COLORS[rng.randi() % FACTORY_ROOF_COLORS.size()]
	canvas.draw_rect(rect, roof)
	if rng.randf() < 0.5:
		# Sawtooth roof: rows of north lights across the depth, glass then slope.
		var row := 6.0
		var v := 1.5
		while v + row <= depth - 1.0:
			canvas.draw_rect(_sub(rect, front, 1.5, v, length - 1.5, v + 2.0), Color("8fb3c4"))
			canvas.draw_rect(_sub(rect, front, 1.5, v + 2.0, length - 1.5, v + row), roof.lightened(0.1))
			v += row
	else:
		# Ribbed metal roof, ribs running front to back.
		var along := 3.0
		while along < length - 1.0:
			canvas.draw_rect(_sub(rect, front, along, 1.0, along + 0.75, depth - 1.0), roof.darkened(0.15))
			along += 3.0
	canvas.draw_rect(rect, roof.darkened(0.35), false, 1.0)
	# Loading doors on the street side, edged in safety yellow.
	var doors := clampi(floori(length / 14.0), 1, 3)
	for i in doors:
		var mid := length * (i + 1) / (doors + 1)
		var door := _sub(rect, front, mid - 3.0, 0.0, mid + 3.0, 1.5)
		canvas.draw_rect(door, FACTORY_ACCENT)
		canvas.draw_rect(door.grow(-0.5), Color(0.2, 0.2, 0.22))
	if depth >= 16.0 and rng.randf() < 0.5:
		# A silo in a back corner.
		var radius := minf(5.0, depth * 0.22)
		var corner := length - radius - 2.0 if rng.randf() < 0.5 else radius + 2.0
		var at := _sub(rect, front, corner, depth - radius - 2.0, corner, depth - radius - 2.0).position
		canvas.draw_circle(at + SHADOW_DIRECTION * 4.0, radius, SHADOW_COLOR)
		canvas.draw_circle(at, radius, Color("b0b4b8"))
		canvas.draw_circle(at, radius * 0.6, Color("c8ccd0"))
		canvas.draw_arc(at, radius, 0.0, TAU, 16, Color("7c8084"), 0.75)


static func _shadow(canvas: CanvasItem, rect: Rect2, height: float) -> void:
	canvas.draw_rect(Rect2(rect.position + SHADOW_DIRECTION * height, rect.size), SHADOW_COLOR)


## Length of the building along the street.
static func _length(rect: Rect2, front: Vector2) -> float:
	return rect.size.y if front.x != 0.0 else rect.size.x


## Depth of the building away from the street.
static func _depth(rect: Rect2, front: Vector2) -> float:
	return rect.size.x if front.x != 0.0 else rect.size.y


## The part of `rect` between `along0` and `along1` along the street and `depth0` and
## `depth1` back from its street side.
static func _sub(rect: Rect2, front: Vector2, along0: float, depth0: float, along1: float, depth1: float) -> Rect2:
	var p0: Vector2
	var p1: Vector2
	if front == Vector2.UP:
		p0 = rect.position + Vector2(along0, depth0)
		p1 = rect.position + Vector2(along1, depth1)
	elif front == Vector2.DOWN:
		p0 = Vector2(rect.position.x + along0, rect.end.y - depth0)
		p1 = Vector2(rect.position.x + along1, rect.end.y - depth1)
	elif front == Vector2.LEFT:
		p0 = rect.position + Vector2(depth0, along0)
		p1 = rect.position + Vector2(depth1, along1)
	else:
		p0 = Vector2(rect.end.x - depth0, rect.position.y + along0)
		p1 = Vector2(rect.end.x - depth1, rect.position.y + along1)
	return Rect2(p0.min(p1), (p1 - p0).abs())
