class_name TransitSystem
extends RefCounted
## Bus stops, bus lines, the buses running them and the people riding them.
## - Stops sit at the curb of a road, on its outermost lane (BusStop); a line loops
##   through its stops in order (BusLine).
## - Each line runs enough buses for about TARGET_HEADWAY between them, spawned one by
##   one at its stops. Buses are vehicles driven by VehicleSystem (Vehicle.in_service):
##   from stop to stop. Stops are request stops: where someone gets off or waits for its
##   line, the bus dwells in its lane, holding up the traffic behind, while riders get
##   off and on (DWELL_BASE, plus BOARD_TIME per rider); elsewhere it drives past.
## - Riders (citizens on a bus leg, wait_for_bus) wait at their stop for a bus of their
##   line, ride to their stop and get off (rider_arrived). If their stop or line goes, or
##   no bus comes for too long, they give up (rider_stranded).
## - Trip planning (plan_ride): the cheapest way by bus between two places, by
##   generalized cost: walk to a stop, wait (half the headway, counted WAIT_WEIGHT
##   times), ride (the times buses really take), change lines at the same stop or one a
##   short walk away (a penalty each, at most MAX_TRANSFERS), walk from the last stop.

signal rider_arrived(citizen_id: int)
signal rider_stranded(citizen_id: int)

const OWNER_ID := -4
const CAPACITY := 40
## Dwell at a stop: opening and closing the doors, plus per rider getting off or on.
const DWELL_BASE := 1.0
const BOARD_TIME := 0.2
## Buses per line: one for each TARGET_HEADWAY of a trip round the line, up to a cap.
const TARGET_HEADWAY := 10.0
const MAX_BUSES_PER_LINE := 12
## New buses join a line at most this often (sim seconds), at different stops.
const SPAWN_INTERVAL := 1.0
## A rider gives up after waiting this many headways (and at least WAIT_GIVE_UP_MIN).
const WAIT_GIVE_UP_HEADWAYS := 3.0
const WAIT_GIVE_UP_MIN := 30.0
## How fast measured times between stops replace the estimate (per bus run).
const MEASURE_BLEND := 0.3
## A bus decides to drive past a stop nobody needs within this much more than its
## braking distance of it.
const SKIP_DECISION_MARGIN := 20.0
## Riders' costs: waiting counts double (it feels longer than riding), and every change
## of bus costs extra.
const WAIT_WEIGHT := 2.0
const TRANSFER_PENALTY := 5.0
const MAX_TRANSFERS := 2
## Placing stops: how far from a road a click may be, and how close to an existing stop
## counts as that stop. After a road edit, a stop stays on a lane in its direction within
## RESOLVE_DISTANCE (moving out to the curb if the road got wider).
const SNAP_DISTANCE := 24.0
const MERGE_DISTANCE := 20.0
const RESOLVE_DISTANCE := 24.0
## Line colours, in turn: blue, pink, purple, yellow (none green or brown like cars and
## trucks).
const LINE_COLORS: Array[Color] = [
	Color("1e88e5"), Color("ec407a"), Color("8e24aa"), Color("fdd835"),
]


class Rider:
	var citizen_id: int
	var line_id: int
	var to_stop_id: int
	## When they started waiting.
	var since: float
	## The stop they wait at, or the bus (vehicle id) they ride (-1 when not).
	var stop_id := -1
	var bus_id := -1


class Bus:
	var vehicle: Vehicle
	var line_id: int
	## Index, in the line's stops, of the stop it is heading to or dwelling at, and of the
	## last stop it was at (-1: none yet).
	var stop_index: int
	var last_index := -1
	var riders: Array[Rider] = []
	var dwell_until := 0.0
	## When it arrived at its last stop.
	var arrived_at := 0.0


var stops: Dictionary[int, BusStop] = {}
var lines: Dictionary[int, BusLine] = {}
## Vehicle id -> Bus.
var buses: Dictionary[int, Bus] = {}

var _roads: RoadNetwork
var _router: Router
var _vehicles: VehicleSystem
var _pedestrians: PedestrianSystem
var _time := 0.0
var _next_stop_id := 1
var _next_line_id := 1
## Stops need finding on the roads again, and lines timing again (roads changed).
var _dirty := false
## Citizen id -> their ride.
var _riders: Dictionary[int, Rider] = {}
## Line id -> when its last bus was spawned, and how many spawns it has tried.
var _last_spawn: Dictionary[int, float] = {}
var _spawn_attempts: Dictionary[int, int] = {}
## Stop id -> stops a short walk away (_stops_near_stop).
var _nearby_stops: Dictionary[int, Dictionary] = {}


func _init(roads: RoadNetwork, router: Router, vehicles: VehicleSystem, pedestrians: PedestrianSystem) -> void:
	_roads = roads
	_router = router
	_vehicles = vehicles
	_pedestrians = pedestrians
	_roads.changed.connect(_on_roads_changed)
	_vehicles.vehicle_arrived.connect(_on_vehicle_arrived)
	_vehicles.vehicle_failed.connect(_on_vehicle_failed)


# --- Stops and lines ----------------------------------------------------------------

## Adds a stop at the curb of the road nearest `point`, on the side `point` is on, or
## returns the stop already there (same side, within MERGE_DISTANCE). -1 if there's no
## road near, or it's too short.
func add_stop(point: Vector2) -> int:
	var spot := find_curb_spot(_roads, point)
	if spot.is_empty():
		return -1
	for stop: BusStop in stops.values():
		if stop.position.distance_to(spot.position) <= MERGE_DISTANCE and stop.direction.dot(spot.direction) > 0.9:
			return stop.id
	var stop := BusStop.new(_next_stop_id, spot.position, spot.direction)
	_next_stop_id += 1
	stop.lane_id = spot.lane_id
	stop.distance = spot.distance
	stops[stop.id] = stop
	_nearby_stops.clear()
	return stop.id


## The stop nearest `point` within `radius`, or -1.
func find_stop(point: Vector2, radius := MERGE_DISTANCE) -> int:
	var best := -1
	var best_distance := radius
	for stop: BusStop in stops.values():
		var distance := stop.position.distance_to(point)
		if distance <= best_distance:
			best = stop.id
			best_distance = distance
	return best


## Where a stop for `point` goes: { "lane_id", "distance", "position", "direction" } at
## the curb of the nearest road on point's side, where a whole bus fits; {} if none.
## Static, so tools can preview it from road geometry alone.
static func find_curb_spot(roads: RoadNetwork, point: Vector2) -> Dictionary:
	var near := roads.find_lane_position(point, SNAP_DISTANCE)
	if near.is_empty():
		return {}
	var lane := _outermost(roads, roads.lanes[near.lane_id])
	return _spot_on(lane, lane.curve.get_closest_offset(point))


func remove_stop(stop_id: int) -> void:
	var stop: BusStop = stops.get(stop_id)
	if stop == null:
		return
	for rider: Rider in stop.waiting.duplicate():
		_strand(rider)
	stops.erase(stop_id)
	_nearby_stops.clear()
	for line: BusLine in lines.values():
		if not line.stop_ids.has(stop_id):
			continue
		line.stop_ids.erase(stop_id)
		if line.stop_ids.size() < 2:
			remove_line(line.id)
			continue
		for bus: Bus in buses.values():
			if bus.line_id != line.id:
				continue
			for rider: Rider in bus.riders.duplicate():
				if rider.to_stop_id == stop_id:
					_strand(rider)
			bus.stop_index = mini(bus.stop_index, line.stop_ids.size() - 1)
			bus.last_index = -1
		_update_line(line)


## Adds a line through `stop_ids` (in order; it loops back to the first). Returns its id,
## or -1 if it doesn't have two different existing stops.
func add_line(stop_ids: Array[int]) -> int:
	var ids: Array[int] = []
	for stop_id in stop_ids:
		if stops.has(stop_id) and not ids.has(stop_id):
			ids.append(stop_id)
	if ids.size() < 2:
		return -1
	var line := BusLine.new(_next_line_id, ids)
	_next_line_id += 1
	line.color = LINE_COLORS[(line.id - 1) % LINE_COLORS.size()]
	lines[line.id] = line
	_nearby_stops.clear()
	_update_line(line)
	line.bus_count = clampi(ceili(line.get_loop_time() / TARGET_HEADWAY), 1, MAX_BUSES_PER_LINE) \
			if line.get_loop_time() < INF else 1
	return line.id


func remove_line(line_id: int) -> void:
	if not lines.has(line_id):
		return
	for bus: Bus in buses.values().duplicate():
		if bus.line_id == line_id:
			_retire(bus)
	for stop: BusStop in stops.values():
		for rider: Rider in stop.waiting.duplicate():
			if rider.line_id == line_id:
				_strand(rider)
	lines.erase(line_id)
	_nearby_stops.clear()
	_last_spawn.erase(line_id)


## Ids of the lines calling at a stop.
func get_lines_at(stop_id: int) -> Array[int]:
	var result: Array[int] = []
	for line: BusLine in lines.values():
		if line.stop_ids.has(stop_id):
			result.append(line.id)
	return result


func get_bus_color(vehicle_id: int) -> Color:
	var bus: Bus = buses.get(vehicle_id)
	var line: BusLine = lines.get(bus.line_id) if bus != null else null
	return line.color if line != null else Color.WHITE


# --- Riders -------------------------------------------------------------------------

## A citizen at `from_stop_id` waits for a bus of `line_id` to ride to `to_stop_id`.
## Returns false if that isn't possible (the stop or line is gone).
func wait_for_bus(citizen_id: int, line_id: int, from_stop_id: int, to_stop_id: int) -> bool:
	var stop: BusStop = stops.get(from_stop_id)
	var line: BusLine = lines.get(line_id)
	if stop == null or line == null or not stop.is_active() or not line.stop_ids.has(to_stop_id):
		return false
	var rider := Rider.new()
	rider.citizen_id = citizen_id
	rider.line_id = line_id
	rider.to_stop_id = to_stop_id
	rider.since = _time
	rider.stop_id = from_stop_id
	stop.waiting.append(rider)
	_riders[citizen_id] = rider
	return true


## Takes a citizen off the network without reporting anything (e.g. their home is gone).
func remove_rider(citizen_id: int) -> void:
	var rider: Rider = _riders.get(citizen_id)
	if rider != null:
		_detach_rider(rider)


## Free-flow riding time on `line_id` from one of its stops to another.
func get_ride_ideal(line_id: int, from_stop_id: int, to_stop_id: int) -> float:
	var line: BusLine = lines.get(line_id)
	if line == null:
		return 0.0
	var i := line.stop_ids.find(from_stop_id)
	var time := 0.0
	for _i in line.stop_ids.size():
		if i == -1 or line.stop_ids[i] == to_stop_id:
			break
		time += line.free_flow_times[i] if line.free_flow_times[i] < INF else 0.0
		i = line.next_index(i)
	return time


## The cheapest way by bus from `from` to `to` (world positions, e.g. lot centers):
## { "mode": Citizen.Mode.BUS, "legs": [walk, bus..., walk] (see Citizen.legs),
## "cost": generalized cost }, or {} if there's none. Stops count if they are at most
## `max_walk` px away on foot; walking costs `walk_weight` times its duration.
func plan_ride(from: Vector2, to: Vector2, max_walk: float, walk_weight: float) -> Dictionary:
	_ensure_resolved()
	if lines.is_empty():
		return {}
	var access := _stops_in_reach(from, max_walk)
	if access.is_empty():
		return {}
	var egress := _stops_in_reach(to, max_walk)
	if egress.is_empty():
		return {}
	# Dijkstra over "aboard a bus of line y at stop x" (key Vector2i(stop, line)).
	var cost: Dictionary[Vector2i, float] = {}
	var transfers: Dictionary[Vector2i, int] = {}
	var ridden: Dictionary[Vector2i, int] = {}
	var parent: Dictionary[Vector2i, Vector2i] = {}
	var settled: Dictionary[Vector2i, bool] = {}
	var open: Array[Vector2i] = []
	var none := Vector2i(-1, -1)
	var relax := func(key: Vector2i, value: float, transfer_count: int, rides: int, from_key: Vector2i) -> void:
		if value < cost.get(key, INF):
			cost[key] = value
			transfers[key] = transfer_count
			ridden[key] = rides
			parent[key] = from_key
			open.append(key)
	for stop_id: int in access:
		for line_id in get_lines_at(stop_id):
			var wait: float = lines[line_id].get_headway() * 0.5 * WAIT_WEIGHT
			relax.call(Vector2i(stop_id, line_id), _walk_time(access[stop_id]) * walk_weight + wait, 0, 0, none)
	var best_total := INF
	var best_key := none
	while not open.is_empty():
		var index := 0
		for i in range(1, open.size()):
			if cost[open[i]] < cost[open[index]]:
				index = i
		var key: Vector2i = open[index]
		open.remove_at(index)
		if settled.has(key):
			continue
		settled[key] = true
		var here := cost[key]
		if here >= best_total:
			break
		var line := lines[key.y]
		if egress.has(key.x) and ridden[key] > 0:
			var total := here + _walk_time(egress[key.x]) * walk_weight
			if total < best_total:
				best_total = total
				best_key = key
		var i := line.stop_ids.find(key.x)
		var next_stop: BusStop = stops[line.stop_ids[line.next_index(i)]]
		if line.segment_times[i] < INF and next_stop.is_active():
			relax.call(Vector2i(next_stop.id, line.id), here + line.segment_times[i], transfers[key], ridden[key] + 1, key)
		if ridden[key] > 0 and transfers[key] < MAX_TRANSFERS:
			# Change lines here, or at a stop a short walk away.
			var nearby := _stops_near_stop(key.x, max_walk)
			for stop_id: int in nearby:
				var walk_cost: float = _walk_time(nearby[stop_id]) * walk_weight if stop_id != key.x else 0.0
				for other_id in get_lines_at(stop_id):
					if other_id != line.id:
						var wait: float = lines[other_id].get_headway() * 0.5 * WAIT_WEIGHT
						relax.call(Vector2i(stop_id, other_id), here + walk_cost + TRANSFER_PENALTY + wait, transfers[key] + 1, ridden[key], key)
	if best_key == none:
		return {}
	# Back from the last stop: consecutive states on the same line make one bus leg; a
	# change of line at another stop walks there.
	var chain: Array[Vector2i] = []
	var key := best_key
	while key != none:
		chain.push_front(key)
		key = parent[key]
	var legs: Array[Dictionary] = [{ "mode": Citizen.Mode.WALK, "from": from, "to": stops[chain[0].x].position }]
	for state in chain:
		var last: Dictionary = legs[-1]
		if last.mode == Citizen.Mode.BUS and last.line == state.y:
			last.to = state.x
			continue
		if last.mode == Citizen.Mode.BUS and last.to != state.x:
			legs.append({ "mode": Citizen.Mode.WALK, "from": stops[last.to].position, "to": stops[state.x].position })
		legs.append({ "mode": Citizen.Mode.BUS, "line": state.y, "from": state.x, "to": state.x })
	legs.append({ "mode": Citizen.Mode.WALK, "from": stops[chain[-1].x].position, "to": to })
	return { "mode": Citizen.Mode.BUS, "legs": legs, "cost": best_total }


# --- Each tick -----------------------------------------------------------------------

func step(time: float) -> void:
	_time = time
	_ensure_resolved()
	for line: BusLine in lines.values():
		_spawn_bus(line)
	for bus: Bus in buses.values():
		if not bus.vehicle.dwelling:
			_skip_if_nobody_there(bus)
			continue
		# Riders who reached the stop while it waits get on too.
		_board(bus)
		if time >= bus.dwell_until:
			_send_on(bus)
	_give_up_waiting()


## Saved: stops and lines (buses and riders restart).
func to_dict() -> Dictionary:
	var stop_list: Array = []
	for stop: BusStop in stops.values():
		stop_list.append({ "id": stop.id, "position": [stop.position.x, stop.position.y],
			"direction": [stop.direction.x, stop.direction.y] })
	var line_list: Array = []
	for line: BusLine in lines.values():
		line_list.append({ "id": line.id, "stops": line.stop_ids.duplicate(), "buses": line.bus_count })
	return { "stops": stop_list, "lines": line_list, "next_stop_id": _next_stop_id, "next_line_id": _next_line_id }


func restore(data: Dictionary) -> void:
	for entry: Dictionary in data.get("stops", []):
		var stop := BusStop.new(int(entry.id), Vector2(entry.position[0], entry.position[1]),
			Vector2(entry.direction[0], entry.direction[1]))
		stops[stop.id] = stop
		_nearby_stops.clear()
		_resolve(stop)
	for entry: Dictionary in data.get("lines", []):
		var ids: Array[int] = []
		for stop_id in entry.stops:
			ids.append(int(stop_id))
		var line := BusLine.new(int(entry.id), ids)
		line.color = LINE_COLORS[(line.id - 1) % LINE_COLORS.size()]
		line.bus_count = int(entry.get("buses", 1))
		lines[line.id] = line
		_update_line(line)
	_next_stop_id = int(data.get("next_stop_id", stops.size() + 1))
	_next_line_id = int(data.get("next_line_id", lines.size() + 1))


func dispose() -> void:
	buses.clear()
	_riders.clear()
	for stop: BusStop in stops.values():
		stop.waiting.clear()


# --- Buses ----------------------------------------------------------------------------

## Puts the line's next bus on the road at one of its stops, if it is short of buses (one
## every SPAWN_INTERVAL, at a different stop each time, where there's room).
func _spawn_bus(line: BusLine) -> void:
	if _time - _last_spawn.get(line.id, -INF) < SPAWN_INTERVAL:
		return
	var running := 0
	for bus: Bus in buses.values():
		if bus.line_id == line.id:
			running += 1
	if running >= line.bus_count:
		return
	var attempt: int = _spawn_attempts.get(line.id, 0)
	_spawn_attempts[line.id] = attempt + 1
	var n := line.stop_ids.size()
	var index := (attempt * maxi(n / maxi(line.bus_count, 1), 1)) % n
	var stop := stops[line.stop_ids[index]]
	var next_index := _next_reachable(line, index, stop.lane_id, stop.distance)
	if not stop.is_active() or next_index.is_empty():
		return
	var vehicle := _vehicles.spawn(next_index.route, -1, OWNER_ID, Vehicle.Kind.BUS)
	if vehicle == null:
		return  # No room at that stop right now: another one next time.
	vehicle.in_service = true
	vehicle.scored_by_owner = true  # Flow scores the riders' trips, not the bus runs.
	_last_spawn[line.id] = _time
	var bus := Bus.new()
	bus.vehicle = vehicle
	bus.line_id = line.id
	bus.stop_index = next_index.index
	bus.last_index = index
	bus.arrived_at = _time
	buses[vehicle.id] = bus


## The next stop after index `index` that a bus at (`lane_id`, `distance`) can reach:
## { "index", "route" }, or {} if none. Buses keep to their line's way (BusLine.routes)
## from the stop they are at; they only look for another way round a stop that's gone
## or after a road edit moved them off it.
func _next_reachable(line: BusLine, index: int, lane_id: int, distance: float) -> Dictionary:
	var next := line.next_index(index)
	var own: Route = line.routes[index] if index < line.routes.size() else null
	if own != null and own.lane_ids[0] == lane_id and stops[line.stop_ids[next]].is_active():
		return { "index": next, "route": Route.new(own.lane_ids, distance, own.end_distance, own.travel_time) }
	next = index
	for i in line.stop_ids.size() - 1:
		next = line.next_index(next)
		var stop := stops[line.stop_ids[next]]
		if not stop.is_active():
			continue
		var route := _router.find_route(lane_id, distance, stop.lane_id, stop.distance)
		if route != null:
			return { "index": next, "route": route }
	return {}


## Stops are request stops: a bus coming up to one where nobody gets off and nobody
## waits for its line drives on to the next without stopping (decided once it is on the
## stop's lane, about to brake for it).
func _skip_if_nobody_there(bus: Bus) -> void:
	var vehicle := bus.vehicle
	if not vehicle.is_on_last_lane() or _roads.lanes.get(vehicle.lane.id) != vehicle.lane:
		return
	var to_stop := vehicle.get_remaining_on_lane()
	if to_stop > vehicle.speed * vehicle.speed / (2.0 * VehicleSystem.DECELERATION) + SKIP_DECISION_MARGIN:
		return
	var line := lines[bus.line_id]
	var stop: BusStop = stops.get(line.stop_ids[bus.stop_index])
	if stop == null or _has_business(bus, stop):
		return
	var next := _next_reachable(line, bus.stop_index, vehicle.lane.id, vehicle.distance)
	if next.is_empty():
		return
	# Passing the stop counts as calling at it, for the timings.
	_record_arrival(bus, line)
	bus.stop_index = next.index
	_vehicles.set_route(vehicle, next.route)


## Does anyone get off the bus at `stop`, or wait there for its line?
func _has_business(bus: Bus, stop: BusStop) -> bool:
	for rider in bus.riders:
		if rider.to_stop_id == stop.id:
			return true
	for rider: Rider in stop.waiting:
		if rider.line_id == bus.line_id:
			return true
	return false


## The bus reached (or passed) its stop: times the run from the stop before.
func _record_arrival(bus: Bus, line: BusLine) -> void:
	if bus.last_index != -1 and line.next_index(bus.last_index) == bus.stop_index:
		var measured := _time - bus.arrived_at
		var old := line.segment_times[bus.last_index]
		line.segment_times[bus.last_index] = lerpf(old, measured, MEASURE_BLEND) if old < INF else measured
	bus.last_index = bus.stop_index
	bus.arrived_at = _time


## A dwelling bus whose time is up drives on to its next stop.
func _send_on(bus: Bus) -> void:
	var vehicle := bus.vehicle
	if _roads.lanes.get(vehicle.lane.id) != vehicle.lane:
		return  # Its road was just rebuilt: VehicleSystem moves it onto the new lanes first.
	var next := _next_reachable(lines[bus.line_id], bus.stop_index, vehicle.lane.id, vehicle.distance)
	if next.is_empty():
		return  # Nowhere to go for now: it waits where it is.
	bus.stop_index = next.index
	_vehicles.set_route(vehicle, next.route)


func _on_vehicle_arrived(vehicle: Vehicle) -> void:
	var bus: Bus = buses.get(vehicle.id) if vehicle.owner_id == OWNER_ID else null
	if bus == null:
		return
	var line := lines[bus.line_id]
	_record_arrival(bus, line)
	var stop_id := line.stop_ids[bus.stop_index]
	var alighting: Array[Rider] = []
	for rider in bus.riders:
		if rider.to_stop_id == stop_id:
			alighting.append(rider)
	bus.dwell_until = _time + DWELL_BASE + BOARD_TIME * alighting.size()
	for rider in alighting:
		_detach_rider(rider)
		rider_arrived.emit(rider.citizen_id)
	_board(bus)


## Riders waiting for the bus's line at its stop get on, while there's room.
func _board(bus: Bus) -> void:
	var line: BusLine = lines.get(bus.line_id)
	if line == null:
		return
	var stop: BusStop = stops.get(line.stop_ids[bus.stop_index])
	if stop == null or stop.waiting.is_empty():
		return
	for rider: Rider in stop.waiting.duplicate():
		if bus.riders.size() >= CAPACITY:
			return
		if rider.line_id != bus.line_id:
			continue
		stop.waiting.erase(rider)
		rider.stop_id = -1
		rider.bus_id = bus.vehicle.id
		bus.riders.append(rider)
		bus.dwell_until += BOARD_TIME


func _on_vehicle_failed(vehicle: Vehicle) -> void:
	var bus: Bus = buses.get(vehicle.id) if vehicle.owner_id == OWNER_ID else null
	if bus != null:
		for rider in bus.riders.duplicate():
			_strand(rider)
		buses.erase(vehicle.id)


## Takes a bus off the road; its riders are stranded.
func _retire(bus: Bus) -> void:
	for rider in bus.riders.duplicate():
		_strand(rider)
	buses.erase(bus.vehicle.id)
	_vehicles.remove(bus.vehicle.id)


func _give_up_waiting() -> void:
	for stop: BusStop in stops.values():
		for rider: Rider in stop.waiting.duplicate():
			var line: BusLine = lines.get(rider.line_id)
			var patience := WAIT_GIVE_UP_MIN
			if line != null and line.get_headway() < INF:
				patience = maxf(patience, line.get_headway() * WAIT_GIVE_UP_HEADWAYS)
			if line == null or _time - rider.since > patience:
				_strand(rider)


func _strand(rider: Rider) -> void:
	_detach_rider(rider)
	rider_stranded.emit(rider.citizen_id)


func _detach_rider(rider: Rider) -> void:
	_riders.erase(rider.citizen_id)
	var stop: BusStop = stops.get(rider.stop_id)
	if stop != null:
		stop.waiting.erase(rider)
	var bus: Bus = buses.get(rider.bus_id)
	if bus != null:
		bus.riders.erase(rider)
	rider.stop_id = -1
	rider.bus_id = -1


# --- Keeping up with road changes ---------------------------------------------------

func _on_roads_changed(_node_ids: Array, _segment_ids: Array) -> void:
	_dirty = true
	_nearby_stops.clear()


func _ensure_resolved() -> void:
	if not _dirty:
		return
	_dirty = false
	for stop: BusStop in stops.values():
		_resolve(stop)
		if not stop.is_active():
			for rider: Rider in stop.waiting.duplicate():
				_strand(rider)
	for line: BusLine in lines.values():
		_update_line(line)


## Finds the stop's lane again: the nearest lane going its way within RESOLVE_DISTANCE,
## then that road's curb lane (the road may have been widened).
func _resolve(stop: BusStop) -> void:
	stop.lane_id = -1
	var best: Lane = null
	var best_distance := RESOLVE_DISTANCE
	for lane: Lane in _roads.lanes.values():
		if lane.is_connector():
			continue
		var offset := lane.curve.get_closest_offset(stop.position)
		var pose := lane.sample_pose(offset)
		var distance := pose.origin.distance_to(stop.position)
		if distance <= best_distance and pose.x.normalized().dot(stop.direction) > 0.9:
			best = lane
			best_distance = distance
	if best == null:
		return
	var lane := _outermost(_roads, best)
	var spot := _spot_on(lane, lane.curve.get_closest_offset(stop.position))
	if spot.is_empty():
		return
	stop.lane_id = spot.lane_id
	stop.distance = spot.distance
	stop.position = spot.position
	stop.direction = spot.direction


## The lane at the curb on the same road and going the same way as `lane`.
static func _outermost(roads: RoadNetwork, lane: Lane) -> Lane:
	var segment: RoadSegment = roads.segments.get(lane.segment_id)
	if segment == null:
		return lane
	for lane_id in segment.lane_ids:
		var other := roads.lanes[lane_id]
		if other.from_node_id == lane.from_node_id and other.index_from_center > lane.index_from_center:
			lane = other
	return lane


## A stop spot `distance` along `lane`, moved so that a whole bus fits on the lane; {}
## if it never does.
static func _spot_on(lane: Lane, distance: float) -> Dictionary:
	var margin := Vehicle.BUS_LENGTH * 0.5 + VehicleSystem.MIN_GAP
	if lane.length < margin * 2.0:
		return {}
	distance = clampf(distance, margin, lane.length - margin)
	var pose := lane.sample_pose(distance)
	return { "lane_id": lane.id, "distance": distance, "position": pose.origin, "direction": pose.x.normalized() }


## Re-times a line on the current roads (keeping measured times where it still runs) and
## redraws its path.
func _update_line(line: BusLine) -> void:
	var n := line.stop_ids.size()
	line.free_flow_times.resize(n)
	line.routes.resize(n)
	var previous := line.segment_times.duplicate()
	line.segment_times.resize(n)
	var path := PackedVector2Array()
	for i in n:
		var a := stops[line.stop_ids[i]]
		var b := stops[line.stop_ids[line.next_index(i)]]
		var route: Route = null
		line.routes[i] = null
		if a.is_active() and b.is_active():
			route = _router.find_route(a.lane_id, a.distance, b.lane_id, b.distance)
		if route == null:
			line.free_flow_times[i] = INF
			line.segment_times[i] = INF
			continue
		line.routes[i] = route
		line.free_flow_times[i] = route.get_free_flow_time(_roads)
		var estimate := line.free_flow_times[i] + DWELL_BASE
		var measured: float = previous[i] if i < previous.size() else INF
		line.segment_times[i] = measured if measured < INF and measured > 0.0 else estimate
		path.append_array(route.get_polyline(_roads))
	line.path = path


## Active stops on some line within `max_walk` px on foot of `point`: stop id -> walking
## distance.
func _stops_in_reach(point: Vector2, max_walk: float) -> Dictionary:
	var result := {}
	for stop: BusStop in stops.values():
		if not stop.is_active() or stop.position.distance_to(point) > max_walk or get_lines_at(stop.id).is_empty():
			continue
		var distance := _pedestrians.network.get_distance(point, stop.position)
		if distance <= max_walk:
			result[stop.id] = distance
	return result


## Stops (on some line) within `max_walk` px on foot of stop `stop_id`, itself included:
## stop id -> walking distance. Cached until roads or stops change.
func _stops_near_stop(stop_id: int, max_walk: float) -> Dictionary:
	if not _nearby_stops.has(stop_id):
		_nearby_stops[stop_id] = _stops_in_reach(stops[stop_id].position, max_walk)
		_nearby_stops[stop_id][stop_id] = 0.0
	return _nearby_stops[stop_id]


static func _walk_time(length: float) -> float:
	return length / PedestrianSystem.WALK_SPEED + PedestrianSystem.START_STOP_ALLOWANCE
