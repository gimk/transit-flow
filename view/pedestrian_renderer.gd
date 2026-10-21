class_name PedestrianRenderer
extends MultiMeshInstance2D
## Draws every pedestrian as a small dot in one batch, interpolated between two tick
## snapshots like the vehicles.

const MIN_CAPACITY := 64
const SIZE := Vector2(3.0, 3.0)


func _ready() -> void:
	var mesh := QuadMesh.new()
	mesh.size = Vector2.ONE
	multimesh = MultiMesh.new()
	multimesh.transform_format = MultiMesh.TRANSFORM_2D
	multimesh.mesh = mesh
	modulate = Color("f2efe6")


func _process(_delta: float) -> void:
	var from := Game.view_from
	var to := Game.view_to
	var count := to.pedestrian_positions.size()
	if multimesh.instance_count < count:
		multimesh.instance_count = maxi(count, maxi(multimesh.instance_count * 2, MIN_CAPACITY))
	multimesh.visible_instance_count = count
	for pedestrian_id: int in to.pedestrian_index:
		var i: int = to.pedestrian_index[pedestrian_id]
		var position_now := to.pedestrian_positions[i]
		var j: int = from.pedestrian_index.get(pedestrian_id, -1)
		if j != -1:
			position_now = from.pedestrian_positions[j].lerp(position_now, Game.view_alpha)
		multimesh.set_instance_transform_2d(i, Transform2D(0.0, SIZE, 0.0, position_now))
