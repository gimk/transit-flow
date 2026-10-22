class_name NightLights
extends CanvasLayer
## Light at night: street lamps along every road and lit windows on the street side of
## buildings, as soft additive glows (light_glow.gdshader). On a canvas layer of its own
## that follows the camera, so DayNight's tint, which darkens the world, doesn't darken
## the lights: they brighten what's under them instead. They fade in with the dusk and are
## out soon after 6:00 (light_amount), and are off when day / night lighting is.
##
## Windows switch on and off through the evening, each at its own time (seeded per
## building and window): homes from dusk until 21:30-01:00, and again early in the
## morning until 6:15; shops until 20:00-23:00; factories until 19:00-21:00, except a third of their
## windows, lit all night for the night shift.

const LAMP_SPACING := 56.0
## Lamps stand this far out from the road's edge (on the sidewalk, by the kerb).
const LAMP_SETBACK := 3.0
const LAMP_RADIUS := 26.0
const LAMP_COLOR := Color(1.0, 0.84, 0.55, 0.8)
const WINDOW_RADIUS := 7.0
const WINDOW_SPACING := 9.0
const WINDOW_COLOR := Color(1.0, 0.78, 0.42, 0.9)
## In the morning the lights go out by then (hours), well before the dawn tint is gone.
const LIGHTS_OUT := 6.25

var _lamps := MultiMeshInstance2D.new()
var _windows := MultiMeshInstance2D.new()
var _glow := ShaderMaterial.new()
var _day_night := true
var _roads_dirty := true
var _lots_dirty := true
## Per window: [position, kind (Lot.Type), seed]; which are lit is worked out by the hour.
var _window_spots: Array[Array] = []
## The tenth of an hour the windows were last lit for.
var _lit_for := -1


func _ready() -> void:
	follow_viewport_enabled = true
	_glow.shader = preload("res://view/light_glow.gdshader")
	for layer: MultiMeshInstance2D in [_lamps, _windows]:
		var multimesh := MultiMesh.new()
		multimesh.transform_format = MultiMesh.TRANSFORM_2D
		multimesh.use_colors = true
		multimesh.mesh = VehicleRenderer.make_quad_around(Vector2(-0.5, -0.5), Vector2(0.5, 0.5))
		layer.multimesh = multimesh
		layer.material = _glow
		add_child(layer)
	_lamps.name = "Lamps"
	_windows.name = "Windows"
	Events.simulation_reset.connect(_on_reset)
	Events.road_network_changed.connect(func(_nodes: Array, _segments: Array) -> void: _on_reset())
	Events.lots_changed.connect(_on_lots_changed)
	Events.debug_overlay_changed.connect(_on_overlay_changed)


func _on_reset() -> void:
	_roads_dirty = true
	_lots_dirty = true


func _on_lots_changed() -> void:
	_lots_dirty = true


func _on_overlay_changed(overlay_id: StringName, shown: bool) -> void:
	if overlay_id == DayNight.OVERLAY_ID:
		_day_night = shown


func _process(_delta: float) -> void:
	var hours := Game.view_to.hours
	var night := light_amount(hours) if _day_night else 0.0
	visible = night > 0.0
	_glow.set_shader_parameter("night", night)
	if _roads_dirty:
		_roads_dirty = false
		_place_lamps()
	if _lots_dirty:
		# LotRenderer lays lots out in its own _process, earlier in the tree: same frame.
		_lots_dirty = false
		_find_windows()
		_lit_for = -1
	var tenth := floori(hours * 10.0)
	if visible and tenth != _lit_for:
		_lit_for = tenth
		_light_windows(hours)


func _place_lamps() -> void:
	var roads := Game.roads
	var spots := PackedVector2Array()
	for segment: RoadSegment in roads.segments.values():
		var centerline := LaneBuilder.get_polyline(segment.centerline)
		var out := segment.road_type.get_total_width() * 0.5 + LAMP_SETBACK
		var start_trim := roads.get_junction_radius(segment.start_node_id)
		var end_trim := roads.get_junction_radius(segment.end_node_id)
		for side in [-1, 1]:
			var line := LaneBuilder.trim_polyline(LaneBuilder.offset_polyline(centerline, side * out), start_trim, end_trim)
			var length := LaneBuilder.polyline_length(line)
			# Staggered: lamps on one side fall between those on the other.
			var along := LAMP_SPACING * (0.25 if side < 0 else 0.75)
			while along < length:
				spots.append(LaneBuilder.sub_polyline(line, along, along)[0])
				along += LAMP_SPACING
	var multimesh := _lamps.multimesh
	multimesh.instance_count = spots.size()
	for i in spots.size():
		multimesh.set_instance_transform_2d(i, Transform2D(0.0, Vector2.ONE * LAMP_RADIUS * 2.0, 0.0, spots[i]))
		multimesh.set_instance_color(i, LAMP_COLOR)


func _find_windows() -> void:
	_window_spots.clear()
	var lots := get_node_or_null(^"../LotRenderer") as LotRenderer
	if lots == null:
		return
	for entry in lots.get_layouts():
		if entry.door == Vector2.INF:
			continue
		var building: Rect2 = entry.building
		var front: Vector2 = entry.front
		if entry.type == Lot.Type.RESIDENTIAL:
			# The house, not its garden.
			building = BuildingPainter.house_rect(building, front, entry.seed)
		# Along the street side, just outside the wall.
		var a: Vector2
		var b: Vector2
		if front == Vector2.UP:
			a = building.position
			b = Vector2(building.end.x, building.position.y)
		elif front == Vector2.DOWN:
			a = Vector2(building.position.x, building.end.y)
			b = building.end
		elif front == Vector2.LEFT:
			a = building.position
			b = Vector2(building.position.x, building.end.y)
		else:
			a = Vector2(building.end.x, building.position.y)
			b = building.end
		var count := maxi(1, floori(a.distance_to(b) / WINDOW_SPACING))
		for k in count:
			var at := a.lerp(b, (k + 0.5) / count) + front
			_window_spots.append([at, entry.type, hash(Vector2i(entry.seed, k))])


func _light_windows(hours: float) -> void:
	var lit := PackedVector2Array()
	for spot in _window_spots:
		if is_window_lit(spot[1], spot[2], hours):
			lit.append(spot[0])
	var multimesh := _windows.multimesh
	multimesh.instance_count = lit.size()
	for i in lit.size():
		multimesh.set_instance_transform_2d(i, Transform2D(0.0, Vector2.ONE * WINDOW_RADIUS * 2.0, 0.0, lit[i]))
		multimesh.set_instance_color(i, WINDOW_COLOR)


## How strongly the lights shine at `hours`: with the dark (DayNight.night_amount) in the
## evening; in the morning they fade out over the half hour before LIGHTS_OUT.
static func light_amount(hours: float) -> float:
	var night := DayNight.night_amount(hours)
	if hours < 12.0:
		night *= 1.0 - smoothstep(LIGHTS_OUT - 0.5, LIGHTS_OUT, hours)
	return night


## Whether a window (of a building of `kind`, picked by `seed`) has its light on at `hours`.
static func is_window_lit(kind: Lot.Type, seed: int, hours: float) -> bool:
	var r1 := float(seed % 1000) / 999.0
	var r2 := float((seed / 1000) % 1000) / 999.0
	# Evening hours run on past midnight: 1 am is 25.
	var evening := hours + 24.0 if hours < 12.0 else hours
	match kind:
		Lot.Type.RESIDENTIAL:
			if hours >= 5.0 + r1 * 0.8 and hours < LIGHTS_OUT:
				return r2 < 0.6
			return evening >= 17.5 + r1 * 1.5 and evening < 21.5 + r2 * 3.5
		Lot.Type.COMMERCIAL:
			return evening >= 17.0 and evening < 20.0 + r1 * 3.0
		_:
			if r2 < 0.33:
				return true
			return evening >= 17.0 and evening < 19.0 + r1 * 2.0
