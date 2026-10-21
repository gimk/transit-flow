class_name LotSystem
extends RefCounted
## Owns all lots and resolves each lot's road access: the nearest road-lane position
## within reach of the lot's center (the lane on the lot's side of the road), plus the
## matching positions on the road's lanes in the other direction.
## Access is cached and recomputed lazily after any road change.
## Lots stand on what the roads around their cell leave free (compute_plot): a road
## built so close that too little is left (e.g. an avenue one cell from another road)
## removes the lot, and no lot is placed there.

signal changed

## From a cell center, the near lane of a road on the cell's edge is ~25 px away and
## the far lane ~39 px; anything beyond a cell's reach doesn't count.
const ACCESS_DISTANCE := 48.0
## Expected extra time (sim seconds) to turn across the road into or out of a driveway,
## counted when choosing between the two sides (Router.find_best_route).
const FAR_SIDE_PENALTY := 4.0
## Room a road takes beyond its half width: the sidewalk, and a little verge.
const ROAD_CLEARANCE := WalkNetwork.SIDEWALK_OFFSET + 2.0
## A plot narrower than this (px) holds no building.
const MIN_PLOT := 16.0

var lots: Dictionary[int, Lot] = {}

var _roads: RoadNetwork
var _lot_by_cell: Dictionary[Vector2i, int] = {}
var _access_cache: Dictionary[int, Array] = {}
var _next_id := 1


func _init(roads: RoadNetwork) -> void:
	_roads = roads
	_roads.changed.connect(_on_roads_changed)


## Returns the new lot id, or -1 if the cell is taken, outside the map, or the roads
## leave no room for a building there.
func add_lot(cell: Vector2i, type: Lot.Type) -> int:
	if not can_place(cell):
		return -1
	var plot := compute_plot(_roads, cell)
	if not plot.has_area():
		return -1
	var lot := Lot.new(_next_id, cell, type)
	lot.plot = plot
	_next_id += 1
	_insert(lot)
	changed.emit()
	return lot.id


## The part of `cell` that roads leave free for a building (world px), or an empty
## Rect2 if too little is left (MIN_PLOT). Each road along or through the cell takes its
## half width plus ROAD_CLEARANCE from the side it runs on (diagonal and curved pieces,
## roughly, from the side facing them).
static func compute_plot(roads: RoadNetwork, cell: Vector2i) -> Rect2:
	var cell_rect := Rect2(Vector2(cell) * Grid.CELL_SIZE, Vector2.ONE * Grid.CELL_SIZE)
	var top_left := cell_rect.position
	var bottom_right := cell_rect.end
	var center := cell_rect.get_center()
	var reach := Grid.CELL_SIZE
	for segment: RoadSegment in roads.segments.values():
		if segment.elevation_layer != 0:
			continue
		var half := segment.road_type.get_total_width() * 0.5 + ROAD_CLEARANCE
		var points := LaneBuilder.get_polyline(segment.centerline)
		for i in points.size() - 1:
			var a := points[i]
			var b := points[i + 1]
			if absf(a.y - b.y) < 0.5 and absf(a.y - center.y) < reach:
				# Along the cell's top or bottom (or through it).
				if minf(a.x, b.x) < cell_rect.end.x - 1.0 and maxf(a.x, b.x) > cell_rect.position.x + 1.0:
					if a.y <= center.y:
						top_left.y = maxf(top_left.y, a.y + half)
					else:
						bottom_right.y = minf(bottom_right.y, a.y - half)
			elif absf(a.x - b.x) < 0.5 and absf(a.x - center.x) < reach:
				if minf(a.y, b.y) < cell_rect.end.y - 1.0 and maxf(a.y, b.y) > cell_rect.position.y + 1.0:
					if a.x <= center.x:
						top_left.x = maxf(top_left.x, a.x + half)
					else:
						bottom_right.x = minf(bottom_right.x, a.x - half)
			elif absf(a.x - b.x) >= 0.5 and absf(a.y - b.y) >= 0.5:
				# A diagonal or curved piece: the plot keeps clear of it on the side facing
				# it (roughly: by the room left between the cell's center and the road).
				var closest := Geometry2D.get_closest_point_to_segment(center, a, b)
				var room := closest.distance_to(center) - half
				if room >= Grid.CELL_SIZE * 0.71:
					continue  # Clear of the whole cell.
				if room < MIN_PLOT * 0.5:
					return Rect2()
				var toward := closest - center
				if absf(toward.x) >= absf(toward.y):
					if toward.x > 0.0:
						bottom_right.x = minf(bottom_right.x, center.x + room)
					else:
						top_left.x = maxf(top_left.x, center.x - room)
				elif toward.y > 0.0:
					bottom_right.y = minf(bottom_right.y, center.y + room)
				else:
					top_left.y = maxf(top_left.y, center.y - room)
	var size := bottom_right - top_left
	if size.x < MIN_PLOT or size.y < MIN_PLOT:
		return Rect2()
	return Rect2(top_left, size)


func remove_lot(lot_id: int) -> bool:
	var lot: Lot = lots.get(lot_id)
	if lot == null:
		return false
	lots.erase(lot_id)
	_lot_by_cell.erase(lot.cell)
	_access_cache.erase(lot_id)
	changed.emit()
	return true


func can_place(cell: Vector2i) -> bool:
	return not _lot_by_cell.has(cell) and Grid.map_rect().has_point(Grid.cell_center(cell))


func get_lot_at_cell(cell: Vector2i) -> int:
	return _lot_by_cell.get(cell, -1)


func get_lots_of_type(type: Lot.Type) -> Array[Lot]:
	var result: Array[Lot] = []
	for lot: Lot in lots.values():
		if lot.type == type:
			result.append(lot)
	return result


## { "lane_id", "distance" } on the near lane of the lot's road, or {} if it has no road
## access.
func get_access(lot_id: int) -> Dictionary:
	var points := get_access_points(lot_id)
	return points[0] if not points.is_empty() else {}


## Every lane position in front of the lot: the near lane first, then the inner lane running
## the other way on the same road. Drivers may turn out of (and into) a driveway in
## either direction, so trips pick whichever is quickest (Router.find_best_route).
## Empty if the lot has no road access.
func get_access_points(lot_id: int) -> Array[Dictionary]:
	if not lots.has(lot_id):
		return []
	if not _access_cache.has(lot_id):
		_access_cache[lot_id] = _find_access_points(lots[lot_id].get_center())
	return _access_cache[lot_id]


func to_dict() -> Dictionary:
	var lot_list: Array = []
	for lot: Lot in lots.values():
		lot_list.append({ "id": lot.id, "cell": [lot.cell.x, lot.cell.y], "type": lot.type })
	return { "lots": lot_list, "next_id": _next_id }


## Replaces all lots with saved ones (keeps this instance, so connections survive).
func restore(data: Dictionary) -> void:
	lots.clear()
	_lot_by_cell.clear()
	_access_cache.clear()
	for entry: Dictionary in data.get("lots", []):
		var lot := Lot.new(int(entry.id), Vector2i(int(entry.cell[0]), int(entry.cell[1])), int(entry.type) as Lot.Type)
		lot.plot = compute_plot(_roads, lot.cell)
		_insert(lot)
	_next_id = int(data.get("next_id", 1))
	changed.emit()


func _insert(lot: Lot) -> void:
	lots[lot.id] = lot
	_lot_by_cell[lot.cell] = lot.id


func _find_access_points(center: Vector2) -> Array[Dictionary]:
	var points: Array[Dictionary] = []
	var near := _roads.find_lane_position(center, ACCESS_DISTANCE)
	if near.is_empty():
		return points
	points.append(near)
	var near_lane := _roads.lanes[near.lane_id]
	# Across the road, only the lane next to the center line: turning left into or out of
	# the driveway, as drivers do.
	for lane_id in _roads.segments[near_lane.segment_id].lane_ids:
		var lane := _roads.lanes[lane_id]
		if lane.from_node_id != near_lane.from_node_id and lane.index_from_center == 0:
			# Turning left across the road takes waiting for a gap: drivers avoid it when
			# the near side isn't much further.
			points.append({ "lane_id": lane_id, "distance": lane.curve.get_closest_offset(center), "penalty": FAR_SIDE_PENALTY })
	return points


func _on_roads_changed(_node_ids: Array, _segment_ids: Array) -> void:
	_access_cache.clear()
	# Roads may have taken land: re-fit every plot, and drop lots left without room.
	var removed := false
	for lot: Lot in lots.values():
		lot.plot = compute_plot(_roads, lot.cell)
		if not lot.plot.has_area():
			lots.erase(lot.id)
			_lot_by_cell.erase(lot.cell)
			removed = true
	if removed:
		changed.emit()
