class_name CitizenSystem
extends RefCounted
## The city's people. Each home houses RESIDENTS_PER_HOME citizens; each looks for a job
## at the nearest workplace (shop or factory) with a free one. Every citizen follows a
## personal daily schedule:
## - morning rush: leave home between ~06:30 and 09:00 for work,
## - evening rush: leave work between ~16:00 and 19:00, on some evenings via a shop
##   (stay 30-60 game minutes), then go home.
## Trips that can't start (no road, no route, driveway blocked) are retried shortly.
## A trip that fails on the way (road gone, gave up) still ends at its destination if
## that still exists, so the daily rhythm carries on; the Flow metric counts the failure.

const RESIDENTS_PER_HOME := 2
const JOBS := { Lot.Type.COMMERCIAL: 2, Lot.Type.INDUSTRIAL: 3 }
const RETRY_DELAY := 2.0  ## Sim seconds.
const SHOPPING_CHANCE := 0.35
## Shoppers pick one of this many shops nearest their work.
const SHOP_CHOICES := 4
const SHOP_MIN_MINUTES := 30.0
const SHOP_MAX_MINUTES := 60.0
## Morning commutes only start before this hour; someone who missed it waits a day.
const MORNING_END_HOUR := 12.0
## Workers still at work after midnight (missed the evening) go home before this hour.
const LATE_NIGHT_END_HOUR := 5.0
## Job seekers matched to a workplace per tick (keeps big zoning sprees cheap).
const JOB_MATCHES_PER_TICK := 10

var citizens: Dictionary[int, Citizen] = {}

var _lots: LotSystem
var _router: Router
var _vehicles: VehicleSystem
var _pedestrians: PedestrianSystem
var _clock: SimClock
var _rng := RandomNumberGenerator.new()
var _next_id := 1
var _time := 0.0
## Workplace lot id -> jobs taken there.
var _jobs_taken: Dictionary[int, int] = {}
## Citizens without a job, waiting to be matched.
var _job_seekers: Dictionary[int, bool] = {}
var _workplaces: Array[Lot] = []
var _shops: Array[Lot] = []


func _init(lots: LotSystem, router: Router, vehicles: VehicleSystem, pedestrians: PedestrianSystem,
		clock: SimClock) -> void:
	_lots = lots
	_router = router
	_vehicles = vehicles
	_pedestrians = pedestrians
	_clock = clock
	_rng.seed = 11
	_lots.changed.connect(_sync_with_lots)
	_vehicles.vehicle_arrived.connect(_on_vehicle_arrived)
	_vehicles.vehicle_failed.connect(_on_vehicle_failed)
	_pedestrians.pedestrian_arrived.connect(_on_pedestrian_arrived)


func step(time: float) -> void:
	_time = time
	_match_job_seekers()
	var hour := _clock.get_time_of_day_hours()
	var day := _clock.get_day()
	for citizen: Citizen in citizens.values():
		if citizen.is_travelling() or time < citizen.retry_at:
			continue
		match citizen.state:
			Citizen.State.AT_HOME:
				if citizen.work_lot_id == -1:
					continue
				var morning := hour >= citizen.leave_home_hour and hour < MORNING_END_HOUR
				if citizen.leave_now or (morning and citizen.last_commute_day != day):
					_depart(citizen, citizen.home_lot_id, citizen.work_lot_id, Citizen.State.TO_WORK)
			Citizen.State.AT_WORK:
				if citizen.leave_now or hour >= citizen.leave_work_hour or hour < LATE_NIGHT_END_HOUR:
					if citizen.shopping_decided_day != day:
						citizen.shopping_decided_day = day
						citizen.going_shopping = _rng.randf() < SHOPPING_CHANCE
					var shop := _pick_shop(citizen.work_lot_id) if citizen.going_shopping else -1
					if citizen.going_shopping and shop != -1:
						_depart(citizen, citizen.work_lot_id, shop, Citizen.State.TO_SHOP)
					else:
						_depart(citizen, citizen.work_lot_id, citizen.home_lot_id, Citizen.State.TO_HOME)
			Citizen.State.AT_SHOP:
				if time >= citizen.shop_until:
					_depart(citizen, citizen.trip_lot_id, citizen.home_lot_id, Citizen.State.TO_HOME)


## Debug: everyone at home leaves for work right away, ignoring the schedule (once they
## have a job), spread at random over the next `spread_seconds` sim seconds.
func depart_everyone_now(spread_seconds := 0.0) -> void:
	for citizen: Citizen in citizens.values():
		if citizen.state == Citizen.State.AT_HOME:
			citizen.leave_now = true
			citizen.retry_at = _time + _rng.randf() * spread_seconds


func get_job_count() -> int:
	var total := 0
	for lot in _workplaces:
		total += JOBS[lot.type]
	return total


func get_employed_count() -> int:
	return citizens.size() - _job_seekers.size()


func _depart(citizen: Citizen, from_lot: int, to_lot: int, travelling: Citizen.State) -> void:
	if from_lot == to_lot:
		# Already there (no car needed).
		_set_off(citizen, to_lot, travelling)
		_arrive(citizen)
		return
	# Short trips are walked.
	var pedestrian := _pedestrians.walk(from_lot, to_lot, citizen.id, PedestrianSystem.MAX_WALK_DISTANCE)
	if pedestrian != null:
		citizen.pedestrian_id = pedestrian.id
		_set_off(citizen, to_lot, travelling)
		return
	# Gets in the car in its driveway; it pulls out when there is a gap (VehicleSystem.depart).
	var none: Array[Dictionary] = []
	var vehicle := _vehicles.depart(from_lot, none, to_lot, none, citizen.id)
	if vehicle == null:
		# No way there (yet): try again shortly.
		citizen.retry_at = _time + RETRY_DELAY
		return
	citizen.vehicle_id = vehicle.id
	_set_off(citizen, to_lot, travelling)


func _set_off(citizen: Citizen, to_lot: int, travelling: Citizen.State) -> void:
	citizen.trip_lot_id = to_lot
	citizen.state = travelling
	citizen.leave_now = false
	if travelling == Citizen.State.TO_WORK:
		citizen.last_commute_day = _clock.get_day()


func _on_vehicle_arrived(vehicle: Vehicle) -> void:
	var citizen: Citizen = citizens.get(vehicle.owner_id)
	if citizen != null:
		_arrive(citizen)


func _on_pedestrian_arrived(pedestrian: Pedestrian) -> void:
	var citizen: Citizen = citizens.get(pedestrian.owner_id)
	if citizen != null:
		_arrive(citizen)


func _on_vehicle_failed(vehicle: Vehicle) -> void:
	var citizen: Citizen = citizens.get(vehicle.owner_id)
	if citizen == null:
		return
	if _lots.lots.has(citizen.trip_lot_id):
		_arrive(citizen)
	else:
		citizen.vehicle_id = -1
		citizen.state = Citizen.State.AT_HOME
		citizen.retry_at = _time + RETRY_DELAY


func _arrive(citizen: Citizen) -> void:
	citizen.vehicle_id = -1
	citizen.pedestrian_id = -1
	match citizen.state:
		Citizen.State.TO_WORK:
			citizen.state = Citizen.State.AT_WORK
		Citizen.State.TO_SHOP:
			citizen.state = Citizen.State.AT_SHOP
			var minutes := _rng.randf_range(SHOP_MIN_MINUTES, SHOP_MAX_MINUTES)
			citizen.shop_until = _time + minutes / SimClock.GAME_MINUTES_PER_SIM_SECOND
		_:
			citizen.state = Citizen.State.AT_HOME


## Keeps RESIDENTS_PER_HOME citizens per home, and drops people and jobs whose lots are gone.
func _sync_with_lots() -> void:
	_workplaces.clear()
	_shops.clear()
	for lot: Lot in _lots.lots.values():
		if JOBS.has(lot.type):
			_workplaces.append(lot)
		if lot.type == Lot.Type.COMMERCIAL:
			_shops.append(lot)

	var residents := {}
	for citizen: Citizen in citizens.values():
		if not _lots.lots.has(citizen.home_lot_id):
			if citizen.vehicle_id != -1:
				_vehicles.remove(citizen.vehicle_id)
			if citizen.pedestrian_id != -1:
				_pedestrians.remove(citizen.pedestrian_id)
			_leave_job(citizen)
			_job_seekers.erase(citizen.id)
			citizens.erase(citizen.id)
			continue
		residents[citizen.home_lot_id] = residents.get(citizen.home_lot_id, 0) + 1
		if citizen.state == Citizen.State.AT_SHOP and not _lots.lots.has(citizen.trip_lot_id):
			citizen.state = Citizen.State.AT_HOME
		if citizen.work_lot_id != -1 and not _lots.lots.has(citizen.work_lot_id):
			_jobs_taken.erase(citizen.work_lot_id)
			citizen.work_lot_id = -1
			_job_seekers[citizen.id] = true
			if citizen.state == Citizen.State.AT_WORK:
				citizen.state = Citizen.State.AT_HOME
	for lot in _lots.get_lots_of_type(Lot.Type.RESIDENTIAL):
		for i in RESIDENTS_PER_HOME - residents.get(lot.id, 0):
			_add_citizen(lot.id)


func _add_citizen(home_lot_id: int) -> void:
	var citizen := Citizen.new(_next_id, home_lot_id)
	_next_id += 1
	# A bell-ish spread (average of two uniform draws) around the rush-hour peaks.
	citizen.leave_home_hour = 6.5 + (_rng.randf() + _rng.randf()) * 0.5 * 2.5
	citizen.leave_work_hour = 16.0 + (_rng.randf() + _rng.randf()) * 0.5 * 3.0
	citizens[citizen.id] = citizen
	_job_seekers[citizen.id] = true


func _match_job_seekers() -> void:
	if _job_seekers.is_empty() or _workplaces.is_empty():
		return
	var matched := 0
	for citizen_id: int in _job_seekers.keys():
		if matched >= JOB_MATCHES_PER_TICK:
			return
		var citizen := citizens[citizen_id]
		var work := _nearest(_workplaces, _lots.lots[citizen.home_lot_id].get_center(), true)
		if work == -1:
			return  # No free job anywhere.
		citizen.work_lot_id = work
		_jobs_taken[work] = _jobs_taken.get(work, 0) + 1
		_job_seekers.erase(citizen_id)
		matched += 1


func _leave_job(citizen: Citizen) -> void:
	if citizen.work_lot_id != -1 and _jobs_taken.has(citizen.work_lot_id):
		_jobs_taken[citizen.work_lot_id] -= 1
	citizen.work_lot_id = -1


## Id of the lot in `candidates` closest to `position` (with a free job if
## `needs_free_job`), or -1.
func _nearest(candidates: Array[Lot], position: Vector2, needs_free_job: bool) -> int:
	var best_id := -1
	var best_distance := INF
	for lot in candidates:
		if needs_free_job and _jobs_taken.get(lot.id, 0) >= JOBS[lot.type]:
			continue
		var distance := position.distance_squared_to(lot.get_center())
		if distance < best_distance:
			best_distance = distance
			best_id = lot.id
	return best_id


## A shop to stop at on the way home from `work_lot_id`: one of the SHOP_CHOICES nearest,
## at random, so the nearest one doesn't get everybody (never the shop they work at).
func _pick_shop(work_lot_id: int) -> int:
	var position := _lots.lots[work_lot_id].get_center()
	var by_distance := _shops.filter(func(shop: Lot) -> bool: return shop.id != work_lot_id)
	if by_distance.is_empty():
		return -1
	by_distance.sort_custom(func(a: Lot, b: Lot) -> bool:
		return position.distance_squared_to(a.get_center()) < position.distance_squared_to(b.get_center()))
	return by_distance[_rng.randi() % mini(SHOP_CHOICES, by_distance.size())].id
