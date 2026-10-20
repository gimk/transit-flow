class_name VehicleRenderer
extends MultiMeshInstance2D
## Draws every vehicle in one batch, interpolating between two tick snapshots so
## motion stays smooth at any frame rate and sim speed. The mesh is a unit square scaled
## per instance to the vehicle's length and width, so cars and trucks share one batch.

const MIN_CAPACITY := 64
const TRUCK_COLOR := Color("b08d6a")
## Drivers running far behind their ideal trip time.
const IMPATIENT_COLOR := Color("ff5a4f")


func _ready() -> void:
	var mesh := QuadMesh.new()
	mesh.size = Vector2.ONE
	multimesh = MultiMesh.new()
	multimesh.transform_format = MultiMesh.TRANSFORM_2D
	multimesh.use_colors = true
	multimesh.mesh = mesh


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
		multimesh.set_instance_color(i, to.colors[i])


static func color_for_vehicle(vehicle: Vehicle) -> Color:
	if vehicle.impatient:
		return IMPATIENT_COLOR
	if vehicle.kind == Vehicle.Kind.TRUCK:
		return TRUCK_COLOR
	return color_for(vehicle.owner_id if vehicle.owner_id >= 0 else vehicle.id)


## A stable, distinct pastel per owner, so you can follow one commuter around
## (per vehicle for ownerless debug traffic).
static func color_for(owner_id: int) -> Color:
	return Color.from_hsv(fmod(owner_id * 0.618034, 1.0), 0.45, 1.0)
