class_name Crosswalks
extends RefCounted
## Who is on, or waiting at, each crosswalk: shared by pedestrians (who update it), cars
## (which give way to them) and traffic lights (waiting pedestrians call the walk phase).
## A crosswalk at a junction is identified by Vector2i(junction node id, id of the road it
## crosses). Mid-block crosswalks (at bus stops, laid out by TransitSystem) by
## Vector2i(MID_BLOCK_BASE - stop id, id of the road), and described in `mid_block`.

const NONE := Vector2i(-1, -1)
const MID_BLOCK_BASE := -1000

## Crosswalk -> number of pedestrians on it / waiting at its curb.
var walkers: Dictionary[Vector2i, int] = {}
var waiting: Dictionary[Vector2i, int] = {}
## Mid-block crosswalk -> { "segment_id", "distance" (along the road from its start),
## "center", "across" (unit vector across the road), "half_width", "lanes" (Array of
## [lane id, distance along it], every lane it crosses) }.
var mid_block: Dictionary[Vector2i, Dictionary] = {}
## Goes up whenever `mid_block` changes (WalkNetwork rebuilds then).
var mid_block_version := 0


static func is_mid_block(crosswalk: Vector2i) -> bool:
	return crosswalk.x <= MID_BLOCK_BASE


func set_mid_block(crossings: Dictionary[Vector2i, Dictionary]) -> void:
	mid_block = crossings
	mid_block_version += 1


func is_empty() -> bool:
	return walkers.is_empty() and waiting.is_empty()


func has_walkers(crosswalk: Vector2i) -> bool:
	return walkers.has(crosswalk)


func has_waiting(crosswalk: Vector2i) -> bool:
	return waiting.has(crosswalk)


func add_walker(crosswalk: Vector2i) -> void:
	_change(walkers, crosswalk, 1)


func remove_walker(crosswalk: Vector2i) -> void:
	_change(walkers, crosswalk, -1)


func add_waiting(crosswalk: Vector2i) -> void:
	_change(waiting, crosswalk, 1)


func remove_waiting(crosswalk: Vector2i) -> void:
	_change(waiting, crosswalk, -1)


func clear() -> void:
	walkers.clear()
	waiting.clear()


static func _change(counts: Dictionary[Vector2i, int], crosswalk: Vector2i, by: int) -> void:
	var count: int = counts.get(crosswalk, 0) + by
	if count > 0:
		counts[crosswalk] = count
	else:
		counts.erase(crosswalk)
