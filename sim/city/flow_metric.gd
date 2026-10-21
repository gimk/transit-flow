class_name FlowMetric
extends RefCounted
## The player's score: how smoothly the city's trips flow. A finished trip scores its
## ideal (free-flow) time divided by its actual time, capped at 1; a failed or abandoned
## trip scores 0. Flow % is the average over trips that ended within the last game day.
## Citizens' trips are scored door to door, all legs together (walking to the car, the
## drive, walking from where it was parked); freight is scored per vehicle. Debug stress
## traffic doesn't count. Also keeps the share of each mode among citizens' trips.

## One game day, in sim seconds.
const WINDOW := SimClock.MINUTES_PER_DAY / SimClock.GAME_MINUTES_PER_SIM_SECOND

## [end time, score] for each counted trip in the window, oldest first.
var _records: Array[Vector2] = []
var _total := 0.0
## [end time, Citizen.Mode] for each citizen trip in the window, oldest first.
var _modes: Array[Vector2i] = []


func _init(vehicles: VehicleSystem, citizens: CitizenSystem = null) -> void:
	vehicles.vehicle_arrived.connect(_on_arrived)
	vehicles.vehicle_failed.connect(_on_failed)
	if citizens != null:
		citizens.trip_ended.connect(_on_trip_ended)


## Flow over the last game day, 0-100, or -1 when no trip has ended yet.
func get_flow_percent(now: float) -> float:
	_forget_before(now - WINDOW)
	if _records.is_empty():
		return -1.0
	return 100.0 * _total / _records.size()


func get_trip_count(now: float) -> int:
	_forget_before(now - WINDOW)
	return _records.size()


## Citizen.Mode -> share (0-1) of citizens' trips over the last game day ({} if none).
func get_mode_shares(now: float) -> Dictionary:
	_forget_before(now - WINDOW)
	var shares := {}
	for record in _modes:
		shares[record.y] = shares.get(record.y, 0.0) + 1.0 / _modes.size()
	return shares


static func score(vehicle: Vehicle, arrived: bool) -> float:
	if not arrived:
		return 0.0
	return _trip_score(vehicle.trip_start_time, vehicle.ideal_trip_time, vehicle.trip_end_time)


static func _trip_score(start: float, ideal: float, end: float) -> float:
	var actual := end - start
	return 1.0 if actual <= 0.0 else clampf(ideal / actual, 0.0, 1.0)


func _record(vehicle: Vehicle, arrived: bool) -> void:
	if vehicle.owner_id == StressTraffic.OWNER_ID or vehicle.scored_by_owner:
		return
	_add(vehicle.trip_end_time, score(vehicle, arrived))


func _add(end_time: float, value: float) -> void:
	_records.append(Vector2(end_time, value))
	_total += value


func _forget_before(time: float) -> void:
	var drop := 0
	while drop < _records.size() and _records[drop].x < time:
		_total -= _records[drop].y
		drop += 1
	if drop > 0:
		_records = _records.slice(drop)
	drop = 0
	while drop < _modes.size() and _modes[drop].x < time:
		drop += 1
	if drop > 0:
		_modes = _modes.slice(drop)


func _on_arrived(vehicle: Vehicle) -> void:
	_record(vehicle, true)


func _on_failed(vehicle: Vehicle) -> void:
	_record(vehicle, false)


func _on_trip_ended(citizen: Citizen, completed: bool) -> void:
	var value := _trip_score(citizen.trip_start_time, citizen.trip_ideal_time, citizen.trip_end_time) if completed else 0.0
	_add(citizen.trip_end_time, value)
	# Whole sim seconds are plenty to forget old records by.
	_modes.append(Vector2i(floori(citizen.trip_end_time), citizen.trip_mode))
