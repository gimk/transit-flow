class_name VehicleRenderer
extends MultiMeshInstance2D
## Draws every vehicle in one batch, interpolating between the last two sim ticks so
## motion stays smooth at any frame rate and sim speed.

const CAR_SIZE := Vector2(Vehicle.LENGTH, Vehicle.WIDTH)
const MIN_CAPACITY := 64


func _ready() -> void:
	var mesh := QuadMesh.new()
	mesh.size = CAR_SIZE
	multimesh = MultiMesh.new()
	multimesh.transform_format = MultiMesh.TRANSFORM_2D
	multimesh.use_colors = true
	multimesh.mesh = mesh


func _process(_delta: float) -> void:
	var vehicles := Game.simulation.vehicles.vehicles
	var count := vehicles.size()
	if multimesh.instance_count < count:
		# Resizing clears instance data; every instance is rewritten below anyway.
		multimesh.instance_count = maxi(count, maxi(multimesh.instance_count * 2, MIN_CAPACITY))
	multimesh.visible_instance_count = count
	var alpha := Game.interpolation_alpha
	var i := 0
	for vehicle: Vehicle in vehicles.values():
		var position_now := vehicle.previous_position.lerp(vehicle.position, alpha)
		var rotation_now := lerp_angle(vehicle.previous_rotation, vehicle.rotation, alpha)
		multimesh.set_instance_transform_2d(i, Transform2D(rotation_now, position_now))
		multimesh.set_instance_color(i, color_for(vehicle.owner_id if vehicle.owner_id >= 0 else vehicle.id))
		i += 1


## A stable, distinct pastel per owner, so you can follow one commuter around
## (per vehicle for ownerless debug traffic).
static func color_for(owner_id: int) -> Color:
	return Color.from_hsv(fmod(owner_id * 0.618034, 1.0), 0.45, 1.0)
