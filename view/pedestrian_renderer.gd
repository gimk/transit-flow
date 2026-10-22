class_name PedestrianRenderer
extends MultiMeshInstance2D
## Draws every person on foot in one batch, as tiny figures seen from above
## (pedestrian.gdshader: clothes in a colour per person, a dark outline), facing the way
## they walk and interpolated between two tick snapshots like the vehicles. Riders
## waiting at bus stops stand in a row on the sidewalk facing the road
## (TransitRenderer.rider_spot); when some board (ViewSnapshot.stop_boarded goes up),
## they are drawn walking one after the other to the bus door, a BOARD_TIME apart.
## Riders getting off need nothing here: their walk starts at the bus door.

const MIN_CAPACITY := 64
## Length (the way they face) x width (shoulders) of a figure, px: fills pedestrian.gdshader.
const FIGURE_SIZE := Vector2(3.2, 4.8)
const CLOTHES: Array[Color] = [
	Color("e53935"), Color("1e88e5"), Color("fdd835"), Color("8e24aa"), Color("43a047"),
	Color("fb8c00"), Color("eceff1"), Color("6d4c41"), Color("00acc1"), Color("3949ab"),
]
## How long a boarding rider takes from the queue to the door (sim seconds).
const BOARD_WALK_TIME := 0.6

## Pedestrian id -> the way they last faced (radians), kept while they stand still.
var _headings: Dictionary[int, float] = {}
## Stop id -> riders boarded there so far, as last seen.
var _boarded: Dictionary[int, int] = {}
## Riders walking to a bus door: { "from", "to" (Vector2), "start" (sim time), "color" }.
var _boarding: Array[Dictionary] = []


func _ready() -> void:
	multimesh = MultiMesh.new()
	multimesh.transform_format = MultiMesh.TRANSFORM_2D
	multimesh.use_colors = true
	multimesh.mesh = _make_quad()
	var shader_material := ShaderMaterial.new()
	shader_material.shader = preload("res://view/pedestrian.gdshader")
	material = shader_material
	Events.simulation_reset.connect(_on_simulation_reset)


## A unit quad with UV equal to the position (-0.5..0.5), for the shader.
static func _make_quad() -> ArrayMesh:
	var a := Vector2(-0.5, -0.5)
	var b := Vector2(0.5, 0.5)
	var vertices := PackedVector2Array([a, Vector2(b.x, a.y), b, a, b, Vector2(a.x, b.y)])
	var arrays := []
	arrays.resize(Mesh.ARRAY_MAX)
	arrays[Mesh.ARRAY_VERTEX] = vertices
	arrays[Mesh.ARRAY_TEX_UV] = vertices
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arrays)
	return mesh


func _on_simulation_reset() -> void:
	_headings.clear()
	_boarded.clear()
	_boarding.clear()


func _process(_delta: float) -> void:
	var from := Game.view_from
	var to := Game.view_to
	var alpha := Game.view_alpha
	var now := lerpf(from.time, to.time, alpha)
	_notice_boardings(to)
	var walking := to.pedestrian_positions.size()
	var waiting := 0
	for i in to.stop_positions.size():
		waiting += mini(to.stop_waiting[i], TransitRenderer.MAX_DRAWN_RIDERS)
	var count := walking + waiting + _boarding.size()
	if multimesh.instance_count < count:
		multimesh.instance_count = maxi(count, maxi(multimesh.instance_count * 2, MIN_CAPACITY))
	if _headings.size() > 2 * walking + MIN_CAPACITY:
		# Forget people who have arrived.
		for pedestrian_id: int in _headings.keys():
			if not to.pedestrian_index.has(pedestrian_id):
				_headings.erase(pedestrian_id)
	var n := 0
	for pedestrian_id: int in to.pedestrian_index:
		var i: int = to.pedestrian_index[pedestrian_id]
		var position_now := to.pedestrian_positions[i]
		var j: int = from.pedestrian_index.get(pedestrian_id, -1)
		var heading: float = _headings.get(pedestrian_id, 0.0)
		if j != -1:
			var before := from.pedestrian_positions[j]
			if before.distance_squared_to(position_now) > 0.0001:
				heading = (position_now - before).angle()
				_headings[pedestrian_id] = heading
			position_now = before.lerp(position_now, alpha)
		_set_figure(n, position_now, heading, CLOTHES[hash(pedestrian_id) % CLOTHES.size()])
		n += 1
	for i in to.stop_positions.size():
		var at_stop := mini(to.stop_waiting[i], TransitRenderer.MAX_DRAWN_RIDERS)
		var along := to.stop_directions[i]
		var facing := Vector2(along.y, -along.x).angle()  # Toward the road.
		for k in at_stop:
			_set_figure(n, TransitRenderer.rider_spot(to, i, k, at_stop), facing,
				CLOTHES[hash(to.stop_ids[i] * 97 + k) % CLOTHES.size()])
			n += 1
	var still_boarding: Array[Dictionary] = []
	for rider in _boarding:
		var t: float = (now - rider.start) / BOARD_WALK_TIME
		if t >= 1.0:
			continue
		still_boarding.append(rider)
		var start: Vector2 = rider.from
		var door: Vector2 = rider.to
		_set_figure(n, start.lerp(door, clampf(t, 0.0, 1.0)), (door - start).angle(), rider.color)
		n += 1
	_boarding = still_boarding
	multimesh.visible_instance_count = n


func _set_figure(index: int, at: Vector2, heading: float, color: Color) -> void:
	multimesh.set_instance_transform_2d(index, Transform2D(heading, FIGURE_SIZE, 0.0, at))
	multimesh.set_instance_color(index, color)


## Starts drawing riders walking to the bus at stops where more have boarded since last
## seen: from the front of the queue (the end nearest the door), one after the other.
func _notice_boardings(view: ViewSnapshot) -> void:
	for i in view.stop_ids.size():
		var stop_id := view.stop_ids[i]
		var boarded := view.stop_boarded[i]
		var seen: int = _boarded.get(stop_id, boarded)
		_boarded[stop_id] = boarded
		var count := mini(boarded - seen, TransitRenderer.MAX_DRAWN_RIDERS)
		if count <= 0:
			continue
		# The queue as it stood before they left: those boarding, then those still waiting.
		var queue := mini(view.stop_waiting[i] + count, TransitRenderer.MAX_DRAWN_RIDERS)
		var door := TransitRenderer.door_spot(view, i)
		for k in count:
			var slot := queue - 1 - k  # The row runs along the traffic; the door is at its front.
			_boarding.append({
				"from": TransitRenderer.rider_spot(view, i, slot, queue),
				"to": door,
				"start": view.time + k * TransitSystem.BOARD_TIME,
				"color": CLOTHES[hash(stop_id * 97 + slot) % CLOTHES.size()],
			})
