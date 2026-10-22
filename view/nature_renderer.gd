class_name NatureRenderer
extends Node2D
## Greenery on land nobody has zoned: trees, clustered into woods by a slow noise over
## the map, and here and there a small park beside a road (mown grass, footpaths, a pond
## or a bench, trees round the edge). Purely decoration, derived from the map: it keeps
## to the part of each cell roads and sidewalks leave free (LotSystem.compute_plot) and is
## laid out again when roads, zones or lots change. The same cell always gets the same
## trees. Trees are one batch (tree.gdshader); parks are drawn here.

## Canopy radius range (px).
const TREE_RADIUS := Vector2(3.5, 6.0)
## The tree quad's width in canopy radii (room for the shadow): tree.gdshader's QUAD_SCALE.
const QUAD_SCALE := 3.2
## Woods: cells further than this into a wood (noise 0..1) get trees, more the deeper.
const WOODS_START := 0.42
const MAX_TREES_PER_CELL := 10
## A tree now and then even in open country.
const LONE_TREE_CHANCE := 0.12
## Share of unzoned cells beside a road that are parks.
const PARK_CHANCE := 0.1
## Size of a wood, in cells.
const WOODS_SCALE := 7.0
const TREE_COLORS: Array[Color] = [
	Color("2e6b2a"), Color("3b7d34"), Color("356e2e"), Color("4a7f35"), Color("2f5e2a"), Color("566f2c"),
]
const LAWN_COLOR := Color(0.42, 0.58, 0.3)
const PATH_COLOR := Color(0.74, 0.67, 0.5)
const POND_COLOR := Color(0.3, 0.5, 0.62)
const BENCH_COLOR := Color(0.45, 0.32, 0.2)
const PATH_WIDTH := 3.0

## Parks to draw: { "plot" (Rect2), "diagonal" (paths corner to corner), "pond" (bool) }.
var _parks: Array[Dictionary] = []
var _trees := MultiMeshInstance2D.new()
## Cell -> its greenery, were it unzoned (see lay_out_wild): depends only on the roads.
var _wild: Dictionary = {}
## Roads changed: lay out the wild greenery again. Zones or lots changed: only filter it.
var _roads_dirty := true
var _dirty := true


func _ready() -> void:
	var trees := MultiMesh.new()
	trees.transform_format = MultiMesh.TRANSFORM_2D
	trees.use_colors = true
	trees.mesh = VehicleRenderer.make_quad_around(Vector2(-0.5, -0.5), Vector2(0.5, 0.5))
	_trees.multimesh = trees
	var shader_material := ShaderMaterial.new()
	shader_material.shader = preload("res://view/tree.gdshader")
	_trees.material = shader_material
	_trees.name = "Trees"
	add_child(_trees)
	Events.simulation_reset.connect(_mark_roads_dirty)
	Events.zones_changed.connect(_mark_dirty)
	Events.lots_changed.connect(_mark_dirty)
	Events.road_network_changed.connect(func(_nodes: Array, _segments: Array) -> void: _mark_roads_dirty())


func _mark_dirty() -> void:
	_dirty = true


func _mark_roads_dirty() -> void:
	_roads_dirty = true
	_dirty = true


func _process(_delta: float) -> void:
	if _dirty:
		_dirty = false
		_rebuild()


func _rebuild() -> void:
	var sim := Game.simulation
	if _roads_dirty:
		_roads_dirty = false
		_wild = lay_out_wild(sim.roads)
	var layout := keep_unzoned(_wild, sim.zones.zones, sim.lots)
	_parks = layout.parks
	var trees: Array[Vector3] = layout.trees
	var multimesh := _trees.multimesh
	multimesh.instance_count = trees.size()
	for i in trees.size():
		var tree := trees[i]
		var at := Vector2(tree.x, tree.y)
		multimesh.set_instance_transform_2d(i, Transform2D(0.0, Vector2.ONE * tree.z * QUAD_SCALE, 0.0, at))
		multimesh.set_instance_color(i, TREE_COLORS[hash(at) % TREE_COLORS.size()])
	queue_redraw()


## Where greenery goes on the map: { "trees": Array[Vector3] (x, y, canopy radius),
## "parks": Array[Dictionary] (see _parks) }. Static and deterministic, for tests.
static func lay_out(roads: RoadNetwork, zones: Dictionary, lots: LotSystem) -> Dictionary:
	return keep_unzoned(lay_out_wild(roads), zones, lots)


## The greenery of a lay_out_wild() map on the cells that are neither zoned nor built on.
static func keep_unzoned(wild: Dictionary, zones: Dictionary, lots: LotSystem) -> Dictionary:
	var trees: Array[Vector3] = []
	var parks: Array[Dictionary] = []
	for cell: Vector2i in wild:
		if zones.has(cell) or lots.get_lot_at_cell(cell) != -1:
			continue
		var greenery: Dictionary = wild[cell]
		trees.append_array(greenery.trees)
		if greenery.has("park"):
			parks.append(greenery.park)
	return { "trees": trees, "parks": parks }


## Greenery per cell as if nothing were zoned, from the roads alone (the slow part, about
## 70 ms for the whole map): cell -> { "trees": Array[Vector3], "park" (if a park) }.
static func lay_out_wild(roads: RoadNetwork) -> Dictionary:
	var near_roads := _cells_near_roads(roads)
	var wild: Dictionary = {}
	for x in Grid.MAP_SIZE_CELLS.x:
		for y in Grid.MAP_SIZE_CELLS.y:
			var cell := Vector2i(x, y)
			var trees: Array[Vector3] = []
			var full := Rect2(Vector2(cell) * Grid.CELL_SIZE, Vector2.ONE * Grid.CELL_SIZE)
			var plot := LotSystem.compute_plot(roads, cell) if near_roads.has(cell) else full
			if not plot.has_area():
				continue
			if plot != full and _random(cell, 1) < PARK_CHANCE:
				_add_park_trees(trees, plot, cell, near_roads.get(cell, []))
				wild[cell] = { "trees": trees,
					"park": { "plot": plot, "diagonal": _random(cell, 2) < 0.5, "pond": _random(cell, 3) < 0.3 } }
				continue
			var woods := _woods(cell)
			var count := 0
			if woods > WOODS_START:
				count = ceili((woods - WOODS_START) / (1.0 - WOODS_START) * MAX_TREES_PER_CELL)
			elif _random(cell, 4) < LONE_TREE_CHANCE:
				count = 1
			for k in count:
				var radius := lerpf(TREE_RADIUS.x, TREE_RADIUS.y, _random(cell, 10 + k * 3))
				var room := plot.grow(-radius)
				if not room.has_area():
					continue
				var at := room.position + room.size * Vector2(_random(cell, 11 + k * 3), _random(cell, 12 + k * 3))
				if _clear_of(near_roads.get(cell, []), at, radius):
					trees.append(Vector3(at.x, at.y, radius))
			if not trees.is_empty():
				wild[cell] = { "trees": trees }
	return wild


## Trees round the edge of a park, clear of its paths.
static func _add_park_trees(trees: Array[Vector3], plot: Rect2, cell: Vector2i, pieces: Array) -> void:
	var inset := plot.grow(-TREE_RADIUS.y)
	if not inset.has_area():
		return
	var corners := [inset.position, Vector2(inset.end.x, inset.position.y), inset.end, Vector2(inset.position.x, inset.end.y)]
	for k in corners.size():
		var radius := lerpf(TREE_RADIUS.x, TREE_RADIUS.y, _random(cell, 30 + k))
		if _random(cell, 20 + k) < 0.75 and _clear_of(pieces, corners[k], radius):
			trees.append(Vector3(corners[k].x, corners[k].y, radius))


## Cells a road passes close enough to take part of (the rest are wholly free), each with
## the road pieces near it: [from, to, how far greenery keeps from the centerline].
static func _cells_near_roads(roads: RoadNetwork) -> Dictionary:
	var near: Dictionary = {}
	for segment: RoadSegment in roads.segments.values():
		var points := LaneBuilder.get_polyline(segment.centerline)
		var clear := segment.road_type.get_total_width() * 0.5 + LotSystem.ROAD_CLEARANCE
		for i in points.size() - 1:
			var a := points[i]
			var b := points[i + 1]
			var steps := maxi(1, ceili(a.distance_to(b) / (Grid.CELL_SIZE * 0.5)))
			for s in steps + 1:
				var cell := Grid.world_to_cell(a.lerp(b, float(s) / steps))
				for dx in [-1, 0, 1]:
					for dy in [-1, 0, 1]:
						var pieces: Array = near.get_or_add(cell + Vector2i(dx, dy), [])
						if pieces.is_empty() or pieces[-1][0] != a:
							pieces.append([a, b, clear])
	return near


## Whether a tree of `radius` at `at` keeps clear of these road pieces and their sidewalks
## (past a road's end too, where compute_plot doesn't look).
static func _clear_of(pieces: Array, at: Vector2, radius: float) -> bool:
	for piece: Array in pieces:
		var closest := Geometry2D.get_closest_point_to_segment(at, piece[0], piece[1])
		if at.distance_to(closest) < piece[2] + radius:
			return false
	return true


## How deep into a wood a cell is, 0..1: smooth value noise over cells.
static func _woods(cell: Vector2i) -> float:
	var p := Vector2(cell) / WOODS_SCALE
	var i := Vector2i(p.floor())
	var f := p - Vector2(i)
	var u := f * f * (Vector2(3, 3) - 2.0 * f)
	var top := lerpf(_random(i, 0), _random(i + Vector2i(1, 0), 0), u.x)
	var bottom := lerpf(_random(i + Vector2i(0, 1), 0), _random(i + Vector2i(1, 1), 0), u.x)
	return lerpf(top, bottom, u.y)


## A repeatable random number in 0..1 for a cell and a purpose (`salt`).
static func _random(cell: Vector2i, salt: int) -> float:
	return float(hash(Vector3i(cell.x, cell.y, salt)) % 10007) / 10006.0


func _draw() -> void:
	for park in _parks:
		var plot: Rect2 = park.plot
		draw_rect(plot, LAWN_COLOR)
		var center := plot.get_center()
		if park.diagonal:
			draw_line(plot.position, plot.end, PATH_COLOR, PATH_WIDTH)
			draw_line(Vector2(plot.end.x, plot.position.y), Vector2(plot.position.x, plot.end.y), PATH_COLOR, PATH_WIDTH)
		else:
			draw_line(Vector2(plot.position.x, center.y), Vector2(plot.end.x, center.y), PATH_COLOR, PATH_WIDTH)
			draw_line(Vector2(center.x, plot.position.y), Vector2(center.x, plot.end.y), PATH_COLOR, PATH_WIDTH)
		if park.pond:
			var radius := minf(plot.size.x, plot.size.y) * 0.18
			draw_circle(center, radius + 1.5, PATH_COLOR)
			draw_circle(center, radius, POND_COLOR)
		else:
			draw_circle(center, 3.5, PATH_COLOR)
			var bench := Rect2(center + Vector2(5.0, -6.5), Vector2(5.0, 1.8))
			draw_rect(bench, BENCH_COLOR)
