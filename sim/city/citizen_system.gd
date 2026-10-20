class_name CitizenSystem
extends RefCounted
## Phase 1c commuters: one citizen per residential lot, working at the nearest
## industrial lot, alternating home -> work -> home with a fixed dwell time.
## Phase 3 replaces the dwell timer with the daily schedule and adds shopping trips.

const DWELL_TIME := 8.0  ## Sim seconds at home / at work (40 game minutes).
const RETRY_DELAY := 2.0  ## Wait before retrying when there's no work or no route.
const FIRST_DEPARTURE_DELAY := 1.0

var citizens: Dictionary[int, Citizen] = {}

var _lots: LotSystem
var _router: Router
var _vehicles: VehicleSystem
var _next_id := 1
var _time := 0.0


func _init(lots: LotSystem, router: Router, vehicles: VehicleSystem) -> void:
	_lots = lots
	_router = router
	_vehicles = vehicles
	_lots.changed.connect(_sync_with_lots)
	_vehicles.vehicle_arrived.connect(_on_vehicle_arrived)
	_vehicles.vehicle_failed.connect(_on_vehicle_failed)


func step(time: float) -> void:
	_time = time
	for citizen: Citizen in citizens.values():
		if not citizen.is_travelling() and time >= citizen.next_departure:
			_depart(citizen)


func _depart(citizen: Citizen) -> void:
	if citizen.work_lot_id == -1:
		citizen.work_lot_id = _find_workplace(citizen)
		if citizen.work_lot_id == -1:
			citizen.next_departure = _time + RETRY_DELAY
			return
	var to_work := citizen.state == Citizen.State.AT_HOME
	var from_lot := citizen.home_lot_id if to_work else citizen.work_lot_id
	var to_lot := citizen.work_lot_id if to_work else citizen.home_lot_id
	var from := _lots.get_access(from_lot)
	var to := _lots.get_access(to_lot)
	var route: Route = null
	if not from.is_empty() and not to.is_empty():
		route = _router.find_route(from.lane_id, from.distance, to.lane_id, to.distance)
	if route == null:
		citizen.next_departure = _time + RETRY_DELAY
		return
	citizen.vehicle_id = _vehicles.spawn(route, to_lot, citizen.id).id
	citizen.state = Citizen.State.DRIVING_TO_WORK if to_work else Citizen.State.DRIVING_HOME


func _on_vehicle_arrived(vehicle: Vehicle) -> void:
	var citizen: Citizen = citizens.get(vehicle.owner_id)
	if citizen == null:
		return
	citizen.vehicle_id = -1
	citizen.state = Citizen.State.AT_WORK if citizen.state == Citizen.State.DRIVING_TO_WORK else Citizen.State.AT_HOME
	citizen.next_departure = _time + DWELL_TIME


## A failed trip sends the citizen home (off-screen) to try again later.
func _on_vehicle_failed(vehicle: Vehicle) -> void:
	var citizen: Citizen = citizens.get(vehicle.owner_id)
	if citizen == null:
		return
	citizen.vehicle_id = -1
	citizen.state = Citizen.State.AT_HOME
	citizen.next_departure = _time + RETRY_DELAY


## Keeps exactly one citizen per residential lot and drops references to removed lots.
func _sync_with_lots() -> void:
	var housed := {}
	for citizen: Citizen in citizens.values():
		if not _lots.lots.has(citizen.home_lot_id):
			if citizen.vehicle_id != -1:
				_vehicles.remove(citizen.vehicle_id)
			citizens.erase(citizen.id)
			continue
		housed[citizen.home_lot_id] = true
		if citizen.work_lot_id != -1 and not _lots.lots.has(citizen.work_lot_id):
			citizen.work_lot_id = -1
			if citizen.state == Citizen.State.AT_WORK:
				citizen.state = Citizen.State.AT_HOME
	for lot in _lots.get_lots_of_type(Lot.Type.RESIDENTIAL):
		if not housed.has(lot.id):
			var citizen := Citizen.new(_next_id, lot.id)
			_next_id += 1
			citizen.next_departure = _time + FIRST_DEPARTURE_DELAY
			citizens[citizen.id] = citizen


func _find_workplace(citizen: Citizen) -> int:
	var home := _lots.lots[citizen.home_lot_id].get_center()
	var best_id := -1
	var best_distance := INF
	for lot in _lots.get_lots_of_type(Lot.Type.INDUSTRIAL):
		var distance := home.distance_squared_to(lot.get_center())
		if distance < best_distance:
			best_distance = distance
			best_id = lot.id
	return best_id
