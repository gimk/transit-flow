class_name NatureRenderer
extends Node2D
## Trees on land nobody has zoned, clustered into woods by a slow noise over the map, with
## a lone tree now and then elsewhere. Purely decoration, derived from the map: they keep
## to the part of each cell roads and sidewalks leave free (LotSystem.compute_plot) and are
## laid out again when roads, zones or lots change. The same cell always gets the same
## trees. One batch (tree.gdshader).

## Canopy radius range (px).
const TREE_RADIUS := Vector2(3.5, 6.0)
## The tree quad's width in canopy radii (room for the shadow): tree.gdshader's QUAD_SCALE.
const QUAD_SCALE := 3.2
## Floats per tree in the MultiMesh buffer: a 2D transform and a colour.
const FLOATS_PER_TREE := 12
const REGION_CELLS := 8
## Woods: cells further than this into a wood (noise 0..1) get trees, more the deeper.
const WOODS_START := 0.42
const MAX_TREES_PER_CELL := 10
## A tree now and then even in open country.
const LONE_TREE_CHANCE := 0.12
## Size of a wood, in cells.
const WOODS_SCALE := 7.0
const TREE_COLORS: Array[Color] = [
	Color("2e6b2a"), Color("3b7d34"), Color("356e2e"), Color("4a7f35"), Color("2f5e2a"), Color("566f2c"),
]

var _trees := MultiMeshInstance2D.new()
## Cell -> its trees, were it unzoned (cells without trees left out): depends only on the
## roads near it, so a road edit only lays out again the cells near the roads it changed.
var _wild: Dictionary = {}
## Road segment id -> the cells it passes near; cell -> the segments passing near it (as a
## set): which cells a road edit touches, and which roads to keep clear of in a cell.
var _segment_cells: Dictionary = {}
var _cell_segments: Dictionary = {}
## Cell -> its trees as MultiMesh instance data (_instance_data), ready to join.
var _chunks: Dictionary = {}
## Trees are joined per square region of REGION_CELLS cells (region -> its unzoned cells'
## data), so a change only re-joins the regions it touches; and the cells zoned or built
## on, as last seen, to find which those are when zones or lots change.
var _region_data: Dictionary = {}
var _blocked: Dictionary = {}
## Segments changed since the last frame (all of them after a reset: _reset_pending).
var _changed_segments: Dictionary = {}
var _reset_pending := true
## Zones or lots changed (or trees moved): fill the tree batch again.
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
	Events.simulation_reset.connect(_on_simulation_reset)
	Events.zones_changed.connect(_mark_dirty)
	Events.lots_changed.connect(_mark_dirty)
	Events.road_network_changed.connect(_on_road_network_changed)


func _mark_dirty() -> void:
	_dirty = true


func _on_simulation_reset() -> void:
	_reset_pending = true
	_dirty = true


func _on_road_network_changed(_node_ids: Array, segment_ids: Array) -> void:
	for segment_id: int in segment_ids:
		_changed_segments[segment_id] = true
	_dirty = true


func _process(_delta: float) -> void:
	if _dirty:
		_dirty = false
		_rebuild()


func _rebuild() -> void:
	var sim := Game.simulation
	var dirty_regions: Dictionary = {}
	if _reset_pending:
		_reset_pending = false
		_region_data.clear()
		_blocked.clear()
		for cell: Vector2i in _all_cells():
			dirty_regions[_region_of(cell)] = true
		_changed_segments.clear()
		_segment_cells.clear()
		_cell_segments.clear()
		for segment: RoadSegment in sim.roads.segments.values():
			_index_segment(segment)
		_wild = lay_out_cells(sim.roads, _all_cells(), _cell_segments)
		_chunks.clear()
		for cell: Vector2i in _wild:
			_chunks[cell] = _instance_data(_wild[cell])
	elif not _changed_segments.is_empty():
		# Cells near the changed roads, before and after the change.
		var touched: Dictionary = {}
		for segment_id: int in _changed_segments:
			for cell: Vector2i in _segment_cells.get(segment_id, []):
				touched[cell] = true
				var near: Dictionary = _cell_segments[cell]
				near.erase(segment_id)
				if near.is_empty():
					_cell_segments.erase(cell)
			_segment_cells.erase(segment_id)
			var segment: RoadSegment = sim.roads.segments.get(segment_id)
			if segment != null:
				for cell: Vector2i in _index_segment(segment):
					touched[cell] = true
		_changed_segments.clear()
		var fresh := lay_out_cells(sim.roads, touched.keys(), _cell_segments)
		for cell: Vector2i in touched:
			dirty_regions[_region_of(cell)] = true
			if fresh.has(cell):
				_wild[cell] = fresh[cell]
				_chunks[cell] = _instance_data(fresh[cell])
			else:
				_wild.erase(cell)
				_chunks.erase(cell)
	# Zones and lots change often as the city grows: find the cells that became (or
	# stopped being) zoned or built on, re-join only their regions, then join the regions.
	var blocked: Dictionary = {}
	for cell: Vector2i in sim.zones.zones:
		blocked[cell] = true
	for lot: Lot in sim.lots.lots.values():
		blocked[lot.cell] = true
	for cell: Vector2i in blocked:
		if not _blocked.has(cell):
			dirty_regions[_region_of(cell)] = true
	for cell: Vector2i in _blocked:
		if not blocked.has(cell):
			dirty_regions[_region_of(cell)] = true
	_blocked = blocked
	if dirty_regions.is_empty():
		return  # Nothing visible changed (e.g. a lot on a cell already zoned).
	for region: Vector2i in dirty_regions:
		var region_data := PackedFloat32Array()
		for x in REGION_CELLS:
			for y in REGION_CELLS:
				var cell := region * REGION_CELLS + Vector2i(x, y)
				if _chunks.has(cell) and not blocked.has(cell):
					region_data.append_array(_chunks[cell])
		if region_data.is_empty():
			_region_data.erase(region)
		else:
			_region_data[region] = region_data
	var data := PackedFloat32Array()
	for region_data: PackedFloat32Array in _region_data.values():
		data.append_array(region_data)
	var multimesh := _trees.multimesh
	multimesh.instance_count = data.size() / FLOATS_PER_TREE
	if not data.is_empty():
		multimesh.buffer = data


static func _region_of(cell: Vector2i) -> Vector2i:
	return Vector2i(floori(cell.x / float(REGION_CELLS)), floori(cell.y / float(REGION_CELLS)))


## MultiMesh buffer data for these trees: per tree a 2D transform (2 rows of 4 floats)
## then its colour.
static func _instance_data(trees: Array[Vector3]) -> PackedFloat32Array:
	var data := PackedFloat32Array()
	for tree in trees:
		var size := tree.z * QUAD_SCALE
		var color := TREE_COLORS[hash(Vector2(tree.x, tree.y)) % TREE_COLORS.size()]
		data.append_array([size, 0.0, 0.0, tree.x, 0.0, size, 0.0, tree.y, color.r, color.g, color.b, color.a])
	return data


## Records which cells a segment passes near, both ways; returns them.
func _index_segment(segment: RoadSegment) -> Array:
	var cells := cells_near(segment)
	_segment_cells[segment.id] = cells
	for cell: Vector2i in cells:
		(_cell_segments.get_or_add(cell, {}) as Dictionary)[segment.id] = true
	return cells


## Where trees go on the map: (x, y, canopy radius) each. Static and deterministic, for
## tests.
static func lay_out(roads: RoadNetwork, zones: Dictionary, lots: LotSystem) -> Array[Vector3]:
	return keep_unzoned(lay_out_wild(roads), zones, lots)


## The trees of a lay_out_wild() map on the cells that are neither zoned nor built on.
static func keep_unzoned(wild: Dictionary, zones: Dictionary, lots: LotSystem) -> Array[Vector3]:
	var trees: Array[Vector3] = []
	for cell: Vector2i in wild:
		if not zones.has(cell) and lots.get_lot_at_cell(cell) == -1:
			trees.append_array(wild[cell])
	return trees


## Trees per cell as if nothing were zoned, for the whole map: cell -> Array[Vector3]
## (cells without trees left out). The renderer only redoes the cells a road edit touches.
static func lay_out_wild(roads: RoadNetwork) -> Dictionary:
	var cell_segments: Dictionary = {}
	for segment: RoadSegment in roads.segments.values():
		for cell: Vector2i in cells_near(segment):
			(cell_segments.get_or_add(cell, {}) as Dictionary)[segment.id] = true
	return lay_out_cells(roads, _all_cells(), cell_segments)


## Trees for `cells`, given the segments passing near each cell (`cell_segments`: cell ->
## set of segment ids): cell -> Array[Vector3], cells without trees left out.
static func lay_out_cells(roads: RoadNetwork, cells: Array, cell_segments: Dictionary) -> Dictionary:
	var wild: Dictionary = {}
	for cell: Vector2i in cells:
		var near: Dictionary = cell_segments.get(cell, {})
		var pieces := _pieces(roads, near)
		var full := Rect2(Vector2(cell) * Grid.CELL_SIZE, Vector2.ONE * Grid.CELL_SIZE)
		var plot := LotSystem.compute_plot(roads, cell) if not near.is_empty() else full
		if not plot.has_area():
			continue
		var woods := _woods(cell)
		var count := 0
		if woods > WOODS_START:
			count = ceili((woods - WOODS_START) / (1.0 - WOODS_START) * MAX_TREES_PER_CELL)
		elif _random(cell, 4) < LONE_TREE_CHANCE:
			count = 1
		var trees: Array[Vector3] = []
		for k in count:
			var radius := lerpf(TREE_RADIUS.x, TREE_RADIUS.y, _random(cell, 10 + k * 3))
			var room := plot.grow(-radius)
			if not room.has_area():
				continue
			var at := room.position + room.size * Vector2(_random(cell, 11 + k * 3), _random(cell, 12 + k * 3))
			if _clear_of(pieces, at, radius):
				trees.append(Vector3(at.x, at.y, radius))
		if not trees.is_empty():
			wild[cell] = trees
	return wild


static func _all_cells() -> Array:
	var cells: Array = []
	for x in Grid.MAP_SIZE_CELLS.x:
		for y in Grid.MAP_SIZE_CELLS.y:
			cells.append(Vector2i(x, y))
	return cells


## Cells a road passes close enough to take part of (the rest are wholly free).
static func cells_near(segment: RoadSegment) -> Array:
	var cells: Dictionary = {}
	var points := segment.get_polyline()
	for i in points.size() - 1:
		var a := points[i]
		var b := points[i + 1]
		var steps := maxi(1, ceili(a.distance_to(b) / (Grid.CELL_SIZE * 0.5)))
		for s in steps + 1:
			var cell := Grid.world_to_cell(a.lerp(b, float(s) / steps))
			for dx in [-1, 0, 1]:
				for dy in [-1, 0, 1]:
					cells[cell + Vector2i(dx, dy)] = true
	return cells.keys()


## The road pieces of these segments: [from, to, how far trees keep from the centerline].
static func _pieces(roads: RoadNetwork, segment_ids: Dictionary) -> Array:
	var pieces: Array = []
	for segment_id: int in segment_ids:
		var segment: RoadSegment = roads.segments.get(segment_id)
		if segment == null:
			continue
		var points := segment.get_polyline()
		var clear := segment.road_type.get_total_width() * 0.5 + LotSystem.ROAD_CLEARANCE
		for i in points.size() - 1:
			pieces.append([points[i], points[i + 1], clear])
	return pieces


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
