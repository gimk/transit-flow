class_name CitizenSystem
extends RefCounted
## The city's people. Each home houses RESIDENTS_PER_HOME citizens; each looks for a job
## at the nearest workplace (shop or factory) with a free one. Every citizen follows a
## personal daily schedule:
## - morning rush: leave home between ~06:30 and 09:00 for work,
## - evening rush: leave work between ~16:00 and 19:00, on some evenings via a shop
##   (stay 30-60 game minutes), then go home.
##
## Trips (plan_trip). A trip is a list of legs: walked, driven or by bus. Most citizens
## own a car (CAR_OWNERSHIP), parked where they last left it (Citizen.car_lot_id): they
## can only drive from there, walking to it first if it's nearby. A driver parks at their
## destination if it has a free space, else nearby and walks the rest (Parking). Bus
## trips walk to a stop, ride (changing lines at shared stops) and walk on
## (TransitSystem.plan_ride). Each trip goes the cheapest way by generalized cost: a trip
## on foot costs its time; walking to and from the car or a stop counts WALK_WEIGHT
## times; driving costs its expected time (current congestion included) plus CAR_OVERHEAD
## for getting the car out and parking it, scaled by the citizen's taste for driving;
## waiting for a bus counts double. Walks to or from the car or a stop are at most
## MAX_CHOSEN_WALK. Someone heading home whose car isn't there drives it home.
##
## Flow scores a trip against its legs' free-flow times, capped at the drive between the
## two places on empty roads plus CAR_ACCESS_TIME: walking across town, or parking far
## away, counts as slow even when nothing held it up.
##
## Trips that can't start (no road, no route) are retried shortly. A trip that fails on
## the way (road gone, driver gave up) still ends at its destination if that still
## exists, so the daily rhythm carries on; the Flow metric counts the failure.

## A trip ended (or failed): the citizen's trip_* fields describe it.
signal trip_ended(citizen: Citizen, completed: bool)

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
## Mode choice (see the class doc). Costs are in sim seconds. Distances here are
## compressed (a car crosses a cell in under a second), so the overhead of using a car is
## large next to the drive itself: about the time it takes to walk two or three cells.
## With it, the average citizen walks trips up to ~3 cells. Walks to and from the car or
## a bus stop count WALK_WEIGHT times (a chore on the way); a trip on foot counts as it is.
const WALK_WEIGHT := 1.5
const CAR_OVERHEAD := 10.0
## Longest walk to or from the car or a bus stop.
const MAX_CHOSEN_WALK := PedestrianSystem.MAX_WALK_DISTANCE
## Spread of Citizen.car_preference.
const CAR_PREFERENCE_RANGE := Vector2(0.85, 1.2)
## Share of citizens owning a car.
const CAR_OWNERSHIP := 0.8
## Time a car trip takes beyond the drive itself (to the car, parking), for Flow's
## yardstick (sim seconds).
const CAR_ACCESS_TIME := 4.0

var citizens: Dictionary[int, Citizen] = {}
var parking: Parking

var _lots: LotSystem
var _router: Router
var _vehicles: VehicleSystem
var _pedestrians: PedestrianSystem
var _transit: TransitSystem
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


func _init(lots: LotSystem, router: Router, vehicles: VehicleSystem, pedestrians: PedestrianSystem, transit: TransitSystem,
		clock: SimClock) -> void:
	_lots = lots
	_router = router
	_vehicles = vehicles
	_pedestrians = pedestrians
	_transit = transit
	_clock = clock
	parking = Parking.new(lots, pedestrians)
	_rng.seed = 11
	_lots.changed.connect(_sync_with_lots)
	_vehicles.vehicle_arrived.connect(_on_vehicle_arrived)
	_vehicles.vehicle_failed.connect(_on_vehicle_failed)
	_pedestrians.pedestrian_arrived.connect(_on_pedestrian_arrived)
	_transit.rider_arrived.connect(_on_rider_arrived)
	_transit.rider_stranded.connect(_on_rider_stranded)


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


## The way `citizen` would go from one lot to another now: { "mode": Citizen.Mode,
## "legs": Array[Dictionary] (see Citizen.legs), "cost": generalized cost }, or {} if
## there's none.
func plan_trip(citizen: Citizen, from_lot: int, to_lot: int, going_home := false) -> Dictionary:
	var from := _lots.lots[from_lot].get_center()
	var to := _lots.lots[to_lot].get_center()
	var walk := {}
	var walk_length := _pedestrians.network.get_distance(from, to)
	if walk_length < INF:
		walk = { "mode": Citizen.Mode.WALK, "legs": [_walk_leg(from, to)], "cost": _walk_time(walk_length) }
	var car := _plan_drive(citizen, from_lot, to_lot)
	# The car goes back home with them.
	if going_home and not car.is_empty() and citizen.car_lot_id != citizen.home_lot_id:
		return car
	var options: Array[Dictionary] = [car, _transit.plan_ride(from, to, MAX_CHOSEN_WALK, WALK_WEIGHT), walk]
	var best := {}
	for option in options:
		if not option.is_empty() and (best.is_empty() or option.cost < best.cost):
			best = option
	return best


func _depart(citizen: Citizen, from_lot: int, to_lot: int, travelling: Citizen.State) -> void:
	if from_lot == to_lot:
		# Already there.
		_set_off(citizen, to_lot, travelling)
		_arrive(citizen)
		return
	var plan := plan_trip(citizen, from_lot, to_lot, travelling == Citizen.State.TO_HOME)
	if plan.is_empty():
		# No way there (yet): try again shortly.
		citizen.retry_at = _time + RETRY_DELAY
		return
	citizen.legs.assign(plan.legs)
	citizen.trip_mode = plan.mode
	citizen.trip_start_time = _time
	citizen.trip_ideal_time = 0.0
	citizen.trip_reference_time = _drive_reference(from_lot, to_lot)
	for leg in citizen.legs:
		if leg.mode == Citizen.Mode.CAR:
			# The space at the other end is theirs from now on (the car moves when driven).
			parking.take(leg.to)
	_set_off(citizen, to_lot, travelling)
	_next_leg(citizen)


## A drive from wherever the car is (walking there first) to a parking space at or near
## `to_lot` (walking on from it), as a plan (see plan_trip), or {} if the citizen has no
## car within reach or it can't get there.
func _plan_drive(citizen: Citizen, from_lot: int, to_lot: int) -> Dictionary:
	if not citizen.has_car or not _lots.lots.has(citizen.car_lot_id):
		return {}
	var car_at := citizen.car_lot_id
	var park_at := to_lot if to_lot == citizen.home_lot_id else parking.find_space(to_lot, MAX_CHOSEN_WALK)
	if park_at == car_at:
		return {}  # Nothing to drive.
	var legs: Array[Dictionary] = []
	var cost := 0.0
	if car_at != from_lot:
		var to_car := _pedestrians.get_walking_distance(from_lot, car_at)
		if to_car > MAX_CHOSEN_WALK:
			return {}
		legs.append(_walk_leg(_lots.lots[from_lot].get_center(), _lots.lots[car_at].get_center()))
		cost += _walk_cost(to_car)
	var route := _router.find_best_route(_lots.get_access_points(car_at), _lots.get_access_points(park_at))
	if route == null:
		return {}
	legs.append({ "mode": Citizen.Mode.CAR, "from": car_at, "to": park_at })
	cost += (route.travel_time + CAR_OVERHEAD) * citizen.car_preference
	if park_at != to_lot:
		legs.append(_walk_leg(_lots.lots[park_at].get_center(), _lots.lots[to_lot].get_center()))
		cost += _walk_cost(_pedestrians.get_walking_distance(park_at, to_lot))
	return { "mode": Citizen.Mode.CAR, "legs": legs, "cost": cost }


## Flow's yardstick for a trip between two lots: the drive on empty roads, plus the time
## a car trip takes beyond it (INF if there's no road between them).
func _drive_reference(from_lot: int, to_lot: int) -> float:
	var route := _router.find_best_route(_lots.get_access_points(from_lot), _lots.get_access_points(to_lot))
	if route == null:
		return INF
	return _router.get_free_flow_time(route) + VehicleSystem.START_STOP_ALLOWANCE + CAR_ACCESS_TIME


static func _walk_leg(from: Vector2, to: Vector2) -> Dictionary:
	return { "mode": Citizen.Mode.WALK, "from": from, "to": to }


static func _walk_time(length: float) -> float:
	return length / PedestrianSystem.WALK_SPEED + PedestrianSystem.START_STOP_ALLOWANCE


## Cost of walking to or from the car or a stop.
static func _walk_cost(length: float) -> float:
	return _walk_time(length) * WALK_WEIGHT


func _set_off(citizen: Citizen, to_lot: int, travelling: Citizen.State) -> void:
	citizen.trip_lot_id = to_lot
	citizen.state = travelling
	citizen.leave_now = false
	if travelling == Citizen.State.TO_WORK:
		citizen.last_commute_day = _clock.get_day()


## Starts the trip's next leg, or ends the trip if there's none left.
func _next_leg(citizen: Citizen) -> void:
	citizen.vehicle_id = -1
	citizen.pedestrian_id = -1
	citizen.on_bus = false
	if not _lots.lots.has(citizen.trip_lot_id):
		_finish_trip(citizen, false)  # The destination went while they were on the way.
		return
	if citizen.legs.is_empty():
		_finish_trip(citizen, true)
		return
	var leg: Dictionary = citizen.legs.pop_front()
	match leg.mode:
		Citizen.Mode.WALK:
			var pedestrian := _pedestrians.walk_between(leg.from, leg.to, citizen.id)
			if pedestrian != null:
				citizen.pedestrian_id = pedestrian.id
				citizen.trip_ideal_time += pedestrian.ideal_trip_time
				return
		Citizen.Mode.CAR:
			# Gets in the car in its driveway; it pulls out when there is a gap (VehicleSystem.depart).
			var none: Array[Dictionary] = []
			var vehicle := _vehicles.depart(leg.from, none, leg.to, none, citizen.id)
			if vehicle != null:
				vehicle.scored_by_owner = true
				citizen.vehicle_id = vehicle.id
				citizen.trip_ideal_time += vehicle.ideal_trip_time
				# The car leaves its space for the one taken at the other end.
				parking.release(citizen.car_lot_id)
				citizen.car_lot_id = leg.to
				return
		Citizen.Mode.BUS:
			if _transit.wait_for_bus(citizen.id, leg.line, leg.from, leg.to):
				citizen.on_bus = true
				citizen.trip_ideal_time += _transit.get_ride_ideal(leg.line, leg.from, leg.to)
				return
	# The way vanished since the trip was planned (a road, lot or bus line changed).
	if leg.mode == Citizen.Mode.CAR:
		parking.release(leg.to)
	_finish_trip(citizen, false)


## Reports the trip and puts the citizen at its destination (also after a failed trip,
## if the destination still exists, so the day carries on).
func _finish_trip(citizen: Citizen, completed: bool) -> void:
	for leg in citizen.legs:
		if leg.mode == Citizen.Mode.CAR:
			parking.release(leg.to)  # A space taken for a drive that won't happen.
	citizen.legs.clear()
	citizen.on_bus = false
	citizen.trip_end_time = _time
	citizen.trip_ideal_time = minf(citizen.trip_ideal_time, citizen.trip_reference_time)
	trip_ended.emit(citizen, completed)
	if completed or _lots.lots.has(citizen.trip_lot_id):
		_arrive(citizen)
	else:
		citizen.vehicle_id = -1
		citizen.pedestrian_id = -1
		citizen.state = Citizen.State.AT_HOME
		citizen.retry_at = _time + RETRY_DELAY


func _on_vehicle_arrived(vehicle: Vehicle) -> void:
	var citizen: Citizen = citizens.get(vehicle.owner_id)
	if citizen != null and citizen.vehicle_id == vehicle.id:
		_next_leg(citizen)


func _on_pedestrian_arrived(pedestrian: Pedestrian) -> void:
	var citizen: Citizen = citizens.get(pedestrian.owner_id)
	if citizen != null and citizen.pedestrian_id == pedestrian.id:
		_next_leg(citizen)


func _on_rider_arrived(citizen_id: int) -> void:
	var citizen: Citizen = citizens.get(citizen_id)
	if citizen != null and citizen.on_bus:
		_next_leg(citizen)


## Their stop or line went, or no bus came: the trip fails.
func _on_rider_stranded(citizen_id: int) -> void:
	var citizen: Citizen = citizens.get(citizen_id)
	if citizen != null and citizen.on_bus:
		_finish_trip(citizen, false)


## The car ends up where it was going to park anyway (Citizen.car_lot_id).
func _on_vehicle_failed(vehicle: Vehicle) -> void:
	var citizen: Citizen = citizens.get(vehicle.owner_id)
	if citizen != null and citizen.vehicle_id == vehicle.id:
		citizen.vehicle_id = -1
		_finish_trip(citizen, false)


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
	parking.sync()

	var residents := {}
	for citizen: Citizen in citizens.values():
		if not _lots.lots.has(citizen.home_lot_id):
			if citizen.vehicle_id != -1:
				_vehicles.remove(citizen.vehicle_id)
			if citizen.on_bus:
				_transit.remove_rider(citizen.id)
			if citizen.pedestrian_id != -1:
				_pedestrians.remove(citizen.pedestrian_id)
			parking.release(citizen.car_lot_id)
			_leave_job(citizen)
			_job_seekers.erase(citizen.id)
			citizens.erase(citizen.id)
			continue
		residents[citizen.home_lot_id] = residents.get(citizen.home_lot_id, 0) + 1
		if not _lots.lots.has(citizen.car_lot_id):
			# Its car park is gone: the car is towed home.
			citizen.car_lot_id = citizen.home_lot_id
		if citizen.state == Citizen.State.AT_SHOP and not _lots.lots.has(citizen.trip_lot_id):
			citizen.state = Citizen.State.AT_HOME
		if citizen.is_travelling() and not _lots.lots.has(citizen.trip_lot_id):
			# Heading for a place that's gone: they give up and go home. (A car on the way
			# is turned back by VehicleSystem, which fails it; walkers and riders here.)
			if citizen.pedestrian_id != -1:
				_pedestrians.remove(citizen.pedestrian_id)
				_finish_trip(citizen, false)
			elif citizen.on_bus:
				_transit.remove_rider(citizen.id)
				_finish_trip(citizen, false)
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
	citizen.car_preference = _rng.randf_range(CAR_PREFERENCE_RANGE.x, CAR_PREFERENCE_RANGE.y)
	citizen.has_car = _rng.randf() < CAR_OWNERSHIP
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
