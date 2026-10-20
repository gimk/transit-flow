class_name LotSystem
extends RefCounted
## Owns all lots and resolves each lot's road access point: the nearest road-lane
## position within reach of the lot's center. Because lanes sit on the right of their
## direction of travel, that is the lane running along the lot's side of the road.
## Access is cached and recomputed lazily after any road change.

signal changed

## From a cell center, the near lane of a road on the cell's edge is ~25 px away and
## the far lane ~39 px; anything beyond a cell's reach doesn't count.
const ACCESS_DISTANCE := 48.0

var lots: Dictionary[int, Lot] = {}

var _roads: RoadNetwork
var _lot_by_cell: Dictionary[Vector2i, int] = {}
var _access_cache: Dictionary[int, Dictionary] = {}
var _next_id := 1


func _init(roads: RoadNetwork) -> void:
	_roads = roads
	_roads.changed.connect(_on_roads_changed)


## Returns the new lot id, or -1 if the cell is taken or outside the map.
func add_lot(cell: Vector2i, type: Lot.Type) -> int:
	if not can_place(cell):
		return -1
	var lot := Lot.new(_next_id, cell, type)
	_next_id += 1
	_insert(lot)
	changed.emit()
	return lot.id


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


## { "lane_id", "distance" } on the lot's road, or {} if it has no road access.
func get_access(lot_id: int) -> Dictionary:
	if not lots.has(lot_id):
		return {}
	if not _access_cache.has(lot_id):
		_access_cache[lot_id] = _roads.find_lane_position(lots[lot_id].get_center(), ACCESS_DISTANCE)
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
		_insert(Lot.new(int(entry.id), Vector2i(int(entry.cell[0]), int(entry.cell[1])), int(entry.type) as Lot.Type))
	_next_id = int(data.get("next_id", 1))
	changed.emit()


func _insert(lot: Lot) -> void:
	lots[lot.id] = lot
	_lot_by_cell[lot.cell] = lot.id


func _on_roads_changed(_node_ids: Array, _segment_ids: Array) -> void:
	_access_cache.clear()
