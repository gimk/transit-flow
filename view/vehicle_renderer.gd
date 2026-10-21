class_name VehicleRenderer
extends MultiMeshInstance2D
## Draws every vehicle in one batch, interpolating between two tick snapshots so
## motion stays smooth at any frame rate and sim speed. The mesh is a unit square scaled
## per instance to the vehicle's length and width, so cars, trucks and buses share one
## batch. Colours tell kinds apart: people's cars green, trucks brown, buses in their
## line's colour. The lateness view (debug toggle, F2) colours every vehicle instead by
## how far behind its ideal trip time it is: green on time, through yellow, to red once
## its driver turns impatient (VehicleSystem.IMPATIENT_FACTOR).

const MIN_CAPACITY := 64
const OVERLAY_ID := &"lateness"
const CAR_COLOR := Color("66bb6a")
const TRUCK_COLOR := Color("8d6e63")
const ON_TIME_COLOR := Color("66bb6a")
const LATE_COLOR := Color("fdd835")
const VERY_LATE_COLOR := Color("ef5350")

## The lateness view is on.
var show_lateness := false


func _ready() -> void:
	var mesh := QuadMesh.new()
	mesh.size = Vector2.ONE
	multimesh = MultiMesh.new()
	multimesh.transform_format = MultiMesh.TRANSFORM_2D
	multimesh.use_colors = true
	multimesh.mesh = mesh
	Events.debug_overlay_toggle_requested.connect(_on_toggle_requested)


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
	for vehicle_id: int in to.vehicle_index:
		var i: int = to.vehicle_index[vehicle_id]
		var position_now := to.positions[i]
		var rotation_now := to.rotations[i]
		var j: int = from.vehicle_index.get(vehicle_id, -1)
		if j != -1:
			position_now = from.positions[j].lerp(position_now, alpha)
			rotation_now = lerp_angle(from.rotations[j], rotation_now, alpha)
		multimesh.set_instance_transform_2d(i, Transform2D(rotation_now, to.sizes[i], 0.0, position_now))
		multimesh.set_instance_color(i, lateness_color(to.lateness[i]) if show_lateness else to.colors[i])


## Normal colour of a vehicle that isn't a bus (buses take their line's colour).
static func color_for_vehicle(vehicle: Vehicle) -> Color:
	return TRUCK_COLOR if vehicle.kind == Vehicle.Kind.TRUCK else CAR_COLOR


## How late a vehicle is, 0 (on time) to 1 (impatient): see ViewSnapshot.lateness.
static func lateness_color(lateness: float) -> Color:
	if lateness < 0.5:
		return ON_TIME_COLOR.lerp(LATE_COLOR, lateness * 2.0)
	return LATE_COLOR.lerp(VERY_LATE_COLOR, (lateness - 0.5) * 2.0)
