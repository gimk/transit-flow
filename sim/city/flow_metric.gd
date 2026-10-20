class_name FlowMetric
extends RefCounted
## The player's score: how smoothly the city's trips flow. A finished trip scores its
## ideal (free-flow) time divided by its actual time, capped at 1; a failed or abandoned
## trip scores 0. Flow % is the average over trips that ended within the last game day.
## Debug stress traffic doesn't count.

## One game day, in sim seconds.
const WINDOW := SimClock.MINUTES_PER_DAY / SimClock.GAME_MINUTES_PER_SIM_SECOND

## [end time, score] for each counted trip in the window, oldest first.
var _records: Array[Vector2] = []
var _total := 0.0


func _init(vehicles: VehicleSystem) -> void:
	vehicles.vehicle_arrived.connect(_on_arrived)
	vehicles.vehicle_failed.connect(_on_failed)


## Flow over the last game day, 0-100, or -1 when no trip has ended yet.
func get_flow_percent(now: float) -> float:
	_forget_before(now - WINDOW)
	if _records.is_empty():
		return -1.0
	return 100.0 * _total / _records.size()


func get_trip_count(now: float) -> int:
	_forget_before(now - WINDOW)
	return _records.size()


static func score(vehicle: Vehicle, arrived: bool) -> float:
	if not arrived:
		return 0.0
	var actual := vehicle.trip_end_time - vehicle.trip_start_time
	return 1.0 if actual <= 0.0 else clampf(vehicle.ideal_trip_time / actual, 0.0, 1.0)


func _record(vehicle: Vehicle, arrived: bool) -> void:
	if vehicle.owner_id == StressTraffic.OWNER_ID:
		return
	var value := score(vehicle, arrived)
	_records.append(Vector2(vehicle.trip_end_time, value))
	_total += value


func _forget_before(time: float) -> void:
	var drop := 0
	while drop < _records.size() and _records[drop].x < time:
		_total -= _records[drop].y
		drop += 1
	if drop > 0:
		_records = _records.slice(drop)


func _on_arrived(vehicle: Vehicle) -> void:
	_record(vehicle, true)


func _on_failed(vehicle: Vehicle) -> void:
	_record(vehicle, false)
