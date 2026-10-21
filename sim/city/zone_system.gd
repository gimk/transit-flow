class_name ZoneSystem
extends RefCounted
## Zoned land. The player paints cells Residential, Commercial or Industrial; a zoned
## cell with road access develops into a building (a Lot of the same type) after a short,
## random delay, and loses it again when unzoned or rezoned. Cells without road access,
## or where the roads leave no room for a building (LotSystem.compute_plot),
## wait and are re-checked. Lots placed directly (scenarios, tests) are left alone.

signal changed

const DEVELOP_MIN_DELAY := 2.0  ## Sim seconds.
const DEVELOP_MAX_DELAY := 10.0
## How often waiting cells without road access look again.
const ACCESS_RETRY := 3.0
## Limits buildings appearing per tick, so painting a big area doesn't stall a frame.
const MAX_DEVELOPMENTS_PER_TICK := 8

## Cell -> zone type.
var zones: Dictionary[Vector2i, Lot.Type] = {}

var _roads: RoadNetwork
var _lots: LotSystem
var _rng := RandomNumberGenerator.new()
var _time := 0.0
## Cell -> sim time at which it tries to develop.
var _pending: Dictionary[Vector2i, float] = {}
## Cells that last tried and found no road nearby.
var _without_access: Dictionary[Vector2i, bool] = {}
## Cell -> id of the lot zoning built there.
var _zoned_lots: Dictionary[Vector2i, int] = {}


func _init(roads: RoadNetwork, lots: LotSystem) -> void:
	_roads = roads
	_lots = lots
	_rng.seed = 7
	# A new road can give waiting cells access: let them look again right away.
	_roads.changed.connect(_on_roads_changed)


func paint(cells: Array[Vector2i], type: Lot.Type) -> void:
	var map := Grid.map_rect()
	var touched := false
	for cell in cells:
		if zones.get(cell, -1) == type or not map.has_point(Grid.cell_center(cell)):
			continue
		if _lots.get_lot_at_cell(cell) != -1 and not _zoned_lots.has(cell):
			continue  # A hand-placed lot: not ours to replace.
		_clear_cell(cell)
		zones[cell] = type
		_pending[cell] = _time + _rng.randf_range(DEVELOP_MIN_DELAY, DEVELOP_MAX_DELAY)
		touched = true
	if touched:
		changed.emit()


func erase(cells: Array[Vector2i]) -> void:
	var touched := false
	for cell in cells:
		if zones.has(cell):
			_clear_cell(cell)
			zones.erase(cell)
			touched = true
	if touched:
		changed.emit()


func is_developed(cell: Vector2i) -> bool:
	return _zoned_lots.has(cell)


func is_waiting_for_road(cell: Vector2i) -> bool:
	return _without_access.has(cell)


func step(time: float) -> void:
	_time = time
	if _pending.is_empty():
		return
	var developed := 0
	for cell: Vector2i in _pending.keys():
		if developed >= MAX_DEVELOPMENTS_PER_TICK:
			break
		if _pending[cell] > time:
			continue
		if _roads.find_lane_position(Grid.cell_center(cell), LotSystem.ACCESS_DISTANCE).is_empty() \
				or not LotSystem.compute_plot(_roads, cell).has_area():
			# No road to reach it, or the roads leave no room for a building.
			_without_access[cell] = true
			_pending[cell] = time + ACCESS_RETRY
			continue
		_without_access.erase(cell)
		_pending.erase(cell)
		var lot_id := _lots.add_lot(cell, zones[cell])
		if lot_id != -1:
			_zoned_lots[cell] = lot_id
		developed += 1
	if developed > 0:
		changed.emit()


## Scenarios: every zoned cell with road access develops right away instead of after
## its delay.
func develop_all_now() -> void:
	for cell: Vector2i in _pending.keys():
		_pending[cell] = _time
	var waiting := -1
	# Each step develops a few cells; cells without access stay pending, so stop when a
	# step no longer shrinks the queue.
	while _pending.size() != waiting:
		waiting = _pending.size()
		step(_time)


func to_dict() -> Dictionary:
	var zone_list: Array = []
	for cell in zones:
		zone_list.append([cell.x, cell.y, zones[cell], _zoned_lots.get(cell, -1)])
	return { "zones": zone_list }


## Replaces all zones with saved ones. Lots must be restored first (LotSystem.restore);
## zoned cells whose building is missing start developing again.
func restore(data: Dictionary) -> void:
	zones.clear()
	_pending.clear()
	_without_access.clear()
	_zoned_lots.clear()
	for entry: Array in data.get("zones", []):
		var cell := Vector2i(int(entry[0]), int(entry[1]))
		zones[cell] = int(entry[2]) as Lot.Type
		var lot_id := int(entry[3])
		if lot_id != -1 and _lots.lots.has(lot_id):
			_zoned_lots[cell] = lot_id
		else:
			_pending[cell] = _time
	changed.emit()


func _clear_cell(cell: Vector2i) -> void:
	_pending.erase(cell)
	_without_access.erase(cell)
	if _zoned_lots.has(cell):
		_lots.remove_lot(_zoned_lots[cell])
		_zoned_lots.erase(cell)


func _on_roads_changed(_node_ids: Array, _segment_ids: Array) -> void:
	for cell in _without_access:
		_pending[cell] = _time
	# A road built too close takes a building's land (LotSystem): the cell waits again.
	for cell: Vector2i in _zoned_lots.keys():
		if not _lots.lots.has(_zoned_lots[cell]):
			_zoned_lots.erase(cell)
			_without_access[cell] = true
			_pending[cell] = _time + ACCESS_RETRY
