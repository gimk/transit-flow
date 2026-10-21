class_name Parking
extends RefCounted
## Where citizens' cars are parked. Homes have room for their residents' cars (not
## tracked); shops and factories have a few spaces each (CAPACITY), shared by their
## workers and visitors. A driver heading for a full lot parks at the nearest lot with a
## free space within walking reach and walks the rest (find_space); if there is none,
## they park on the street by their destination, over capacity.
## Spaces are taken when a trip is planned, so two drivers can't count on the last one.

const CAPACITY := {
	Lot.Type.COMMERCIAL: 3,
	Lot.Type.INDUSTRIAL: 4,
}

var _lots: LotSystem
var _pedestrians: PedestrianSystem
## Lot id -> spaces taken (may exceed its capacity: street parking).
var _used: Dictionary[int, int] = {}


func _init(lots: LotSystem, pedestrians: PedestrianSystem) -> void:
	_lots = lots
	_pedestrians = pedestrians


func get_capacity(lot_id: int) -> int:
	var lot: Lot = _lots.lots.get(lot_id)
	return CAPACITY.get(lot.type, 0) if lot != null else 0


func get_used(lot_id: int) -> int:
	return _used.get(lot_id, 0)


func has_space(lot_id: int) -> bool:
	return get_used(lot_id) < get_capacity(lot_id)


## Is the lot somewhere spaces are counted (not a home)?
func is_tracked(lot_id: int) -> bool:
	return CAPACITY.has(_lots.lots[lot_id].type) if _lots.lots.has(lot_id) else false


func take(lot_id: int) -> void:
	if is_tracked(lot_id):
		_used[lot_id] = get_used(lot_id) + 1


func release(lot_id: int) -> void:
	if _used.has(lot_id):
		_used[lot_id] -= 1
		if _used[lot_id] <= 0:
			_used.erase(lot_id)


## Where to park for a visit to `lot_id`: the lot itself if it has a free space (or is
## a home), else the nearest lot with one within `max_walk` px on foot, else the lot
## itself (on the street).
func find_space(lot_id: int, max_walk: float) -> int:
	if not is_tracked(lot_id) or has_space(lot_id):
		return lot_id
	var center := _lots.lots[lot_id].get_center()
	var candidates: Array[Lot] = []
	for lot: Lot in _lots.lots.values():
		if lot.id != lot_id and is_tracked(lot.id) and has_space(lot.id) \
				and lot.get_center().distance_to(center) <= max_walk:
			candidates.append(lot)
	candidates.sort_custom(func(a: Lot, b: Lot) -> bool:
		return a.get_center().distance_squared_to(center) < b.get_center().distance_squared_to(center))
	for lot in candidates:
		if _pedestrians.get_walking_distance(lot.id, lot_id) <= max_walk:
			return lot.id
	return lot_id


## Forgets lots that are gone.
func sync() -> void:
	for lot_id: int in _used.keys():
		if not _lots.lots.has(lot_id):
			_used.erase(lot_id)
