class_name VehicleRenderer
extends MultiMeshInstance2D
## Draws every vehicle in one batch, interpolating between two tick snapshots so
## motion stays smooth at any frame rate and sim speed. Each instance is a quad scaled
## to the vehicle's length and width, drawn by vehicle.gdshader as a car, truck or bus
## seen from above, with its lamps: brake lights when slowing or stopped, indicators
## before turns and lane changes, headlights at night. Colours tell kinds apart: people's
## cars shades of green, trucks brown, buses in their line's colour. The lateness view
## (debug toggle, F2) colours every vehicle instead by how far behind its ideal trip time
## it is: green on time, through yellow, to red once its driver turns impatient
## (VehicleSystem.IMPATIENT_FACTOR).

const MIN_CAPACITY := 64
const OVERLAY_ID := &"lateness"
const CAR_COLOR := Color("66bb6a")
const TRUCK_COLOR := Color("8d6e63")
const ON_TIME_COLOR := Color("66bb6a")
const LATE_COLOR := Color("fdd835")
const VERY_LATE_COLOR := Color("ef5350")
## Lamp flags, passed to the shader.
const LAMP_BRAKE := 1
const LAMP_LEFT := 2
const LAMP_RIGHT := 4
## Drivers signal this far before a turn or their driveway (px).
const SIGNAL_DISTANCE := 50.0
## Brake lights: slowing harder than this (px/s²), or slower than STOPPED_SPEED (px/s).
const BRAKE_DECELERATION := 15.0
const STOPPED_SPEED := 3.0
## The quad reaches past the body (in body lengths / widths), for headlight beams in
## front and the brake glow behind; must cover vehicle.gdshader's BEAM_LENGTH.
const QUAD_FRONT := 1.6
const QUAD_REAR := 0.4
const QUAD_SIDE := 1.0

## The lateness view is on.
var show_lateness := false
## Day / night lighting is on (headlights only then).
var _day_night := true


func _ready() -> void:
	multimesh = MultiMesh.new()
	multimesh.transform_format = MultiMesh.TRANSFORM_2D
	multimesh.use_colors = true
	multimesh.use_custom_data = true
	multimesh.mesh = make_quad()
	var shader_material := ShaderMaterial.new()
	shader_material.shader = preload("res://view/vehicle.gdshader")
	material = shader_material
	Events.debug_overlay_toggle_requested.connect(_on_toggle_requested)
	Events.debug_overlay_changed.connect(_on_overlay_changed)


## A quad in body units (the body spans -0.5..0.5 both ways), with UV equal to the
## position so the shader knows where on the body it is.
static func make_quad() -> ArrayMesh:
	var a := Vector2(-0.5 - QUAD_REAR, -0.5 - QUAD_SIDE)
	var b := Vector2(0.5 + QUAD_FRONT, 0.5 + QUAD_SIDE)
	return make_quad_around(a, b)


## A quad from corner `a` to corner `b` with UV equal to the position, for sprites drawn
## by a shader from where they are on it (vehicles, people, trees).
static func make_quad_around(a: Vector2, b: Vector2) -> ArrayMesh:
	var vertices := PackedVector2Array([a, Vector2(b.x, a.y), b, a, b, Vector2(a.x, b.y)])
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = vertices
	arrays[Mesh.ARRAY_TEX_UV] = vertices
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return mesh


func _unhandled_input(event: InputEvent) -> void:
	if event.is_action_pressed("toggle_lateness"):
		toggle_lateness()
		get_viewport().set_input_as_handled()


func toggle_lateness() -> void:
	show_lateness = not show_lateness
	Events.debug_overlay_changed.emit(OVERLAY_ID, show_lateness)


func _on_toggle_requested(overlay_id: StringName) -> void:
	if overlay_id == OVERLAY_ID:
		toggle_lateness()


func _on_overlay_changed(overlay_id: StringName, shown: bool) -> void:
	if overlay_id == DayNight.OVERLAY_ID:
		_day_night = shown


func _process(_delta: float) -> void:
	# Drawn from the snapshots around the render clock (see Game), not from the live
	# simulation, which may be stepping on the worker thread.
	var from := Game.view_from
	var to := Game.view_to
	var alpha := Game.view_alpha
	var count := to.positions.size()
	if multimesh.instance_count < count:
		# Resizing clears instance data; every instance is rewritten below anyway.
		multimesh.instance_count = maxi(count, maxi(multimesh.instance_count * 2, MIN_CAPACITY))
	multimesh.visible_instance_count = count
	var dt := to.time - from.time
	(material as ShaderMaterial).set_shader_parameter("night", night_amount(to.hours) if _day_night else 0.0)
	for vehicle_id: int in to.vehicle_index:
		var i: int = to.vehicle_index[vehicle_id]
		var position_now := to.positions[i]
		var rotation_now := to.rotations[i]
		var speed := to.speeds[i]
		var braking := speed < STOPPED_SPEED
		var j: int = from.vehicle_index.get(vehicle_id, -1)
		if j != -1:
			position_now = from.positions[j].lerp(position_now, alpha)
			rotation_now = lerp_angle(from.rotations[j], rotation_now, alpha)
			if dt > 0.0 and (speed - from.speeds[j]) / dt < -BRAKE_DECELERATION:
				braking = true
		var size := to.sizes[i]
		multimesh.set_instance_transform_2d(i, Transform2D(rotation_now, size, 0.0, position_now))
		multimesh.set_instance_color(i, lateness_color(to.lateness[i]) if show_lateness else to.colors[i])
		var lamps := to.indicators[i] | (LAMP_BRAKE if braking else 0)
		multimesh.set_instance_custom_data(i, Color(to.kinds[i], lamps, size.x, size.y))


## How dark it is (0 day, 1 night), from the day / night tint.
static func night_amount(hours: float) -> float:
	var night_tint: Color = DayNight.KEYFRAMES[0][1]
	return clampf((1.0 - DayNight.tint_at(hours).get_luminance()) / (1.0 - night_tint.get_luminance()), 0.0, 1.0)


## Normal colour of a vehicle that isn't a bus (buses take their line's colour): cars in
## shades of green, so they still read as people's cars but don't all look alike.
static func color_for_vehicle(vehicle: Vehicle) -> Color:
	if vehicle.kind == Vehicle.Kind.TRUCK:
		return TRUCK_COLOR
	return car_color(vehicle.id)


## A car's colour, a shade of green picked by `seed`.
static func car_color(seed: int) -> Color:
	var shade := hash(seed)
	var hue := CAR_COLOR.h + ((shade % 64) / 63.0 - 0.5) * 0.08
	var value := CAR_COLOR.v * (0.75 + ((shade >> 6) % 64) / 63.0 * 0.35)
	return Color.from_hsv(hue, CAR_COLOR.s * 0.9, clampf(value, 0.0, 1.0))


## Which indicator a driver has on (LAMP_LEFT, LAMP_RIGHT or 0): before a turn at a
## junction or into its driveway, while waiting for or making a lane change, and pulling
## out of a bus bay. Reads the simulation (called when capturing a snapshot).
static func indicator_for(vehicle: Vehicle) -> int:
	if vehicle.in_bay:
		return LAMP_LEFT if vehicle.leaving_bay_since != INF else 0
	# Still sliding across into a new lane (offset: how far right of it the vehicle is).
	if vehicle.lateral_offset > 0.5:
		return LAMP_LEFT
	if vehicle.lateral_offset < -0.5:
		return LAMP_RIGHT
	var lane := vehicle.lane
	if lane == null:
		return 0
	if lane.is_connector():
		return _turn_lamp(lane.turn)
	if vehicle.next_lanes.is_empty():
		if vehicle.destination_lot_id == -1 or vehicle.end_distance - vehicle.distance > SIGNAL_DISTANCE:
			return 0
		# Into the driveway: across the road if it is on the far side.
		return LAMP_LEFT if vehicle.arrival_checked and not vehicle.crossing_lanes.is_empty() else LAMP_RIGHT
	var next := vehicle.next_lanes[0]
	if lane.is_beside(next):
		# Looking for a gap to change lanes, wherever along the lane.
		var pose := lane.sample_pose(vehicle.distance)
		var toward := next.sample_pose(minf(vehicle.distance, next.length)).origin - pose.origin
		return LAMP_RIGHT if pose.x.cross(toward) > 0.0 else LAMP_LEFT
	if next.is_connector() and lane.length - vehicle.distance <= SIGNAL_DISTANCE:
		return _turn_lamp(next.turn)
	return 0


static func _turn_lamp(turn: Lane.Turn) -> int:
	match turn:
		Lane.Turn.RIGHT:
			return LAMP_RIGHT
		Lane.Turn.LEFT, Lane.Turn.U_TURN:
			return LAMP_LEFT
	return 0


## How late a vehicle is, 0 (on time) to 1 (impatient): see ViewSnapshot.lateness.
static func lateness_color(lateness: float) -> Color:
	if lateness < 0.5:
		return ON_TIME_COLOR.lerp(LATE_COLOR, lateness * 2.0)
	return LATE_COLOR.lerp(VERY_LATE_COLOR, (lateness - 0.5) * 2.0)
