class_name FreightSystem
extends RefCounted
## Trucks. Through the day, factories receive raw materials from outside the city, ship
## goods to shops and export the rest; shops also import goods from outside. Trips are
## drawn at random per lot, at a rate that follows the time of day: busiest in the midday
## logistics window (10:00-16:00), lighter in the rush hours, a trickle at night.

const OWNER_ID := -3
const NOWHERE: Array[Dictionary] = []
## Trucks per lot per game hour at the busiest time of day. (Lowered by a third with
## driveways, Phase 6: before, trucks that found no room to appear were silently dropped,
## so these gave about that much less traffic.)
const RAW_MATERIALS_PER_FACTORY := 0.2
const GOODS_PER_FACTORY := 0.27
const EXPORTS_PER_FACTORY := 0.1
const IMPORTS_PER_SHOP := 0.1

var _lots: LotSystem
var _externals: ExternalConnections
var _router: Router
var _vehicles: VehicleSystem
var _clock: SimClock
var _rng := RandomNumberGenerator.new()
var _factories: Array[Lot] = []
var _shops: Array[Lot] = []


func _init(lots: LotSystem, externals: ExternalConnections, router: Router, vehicles: VehicleSystem,
		clock: SimClock) -> void:
	_lots = lots
	_externals = externals
	_router = router
	_vehicles = vehicles
	_clock = clock
	_rng.seed = 23
	_lots.changed.connect(_on_lots_changed)


## Relative freight activity at a given hour of the day (1 = midday peak).
static func activity(hour: float) -> float:
	if hour >= 10.0 and hour < 16.0:
		return 1.0
	if hour >= 6.0 and hour < 20.0:
		return 0.35
	return 0.12


func step(dt: float) -> void:
	if _factories.is_empty() and _shops.is_empty():
		return
	var game_hours := dt * SimClock.GAME_MINUTES_PER_SIM_SECOND / 60.0
	var level := activity(_clock.get_time_of_day_hours()) * game_hours
	var gates := _externals.get_active_points()
	for factory in _factories:
		var factory_access := _lots.get_access_points(factory.id)
		if factory_access.is_empty():
			continue
		if not gates.is_empty() and _rng.randf() < RAW_MATERIALS_PER_FACTORY * level:
			_send(-1, _gate(_externals.get_inbound(_pick(gates))), factory.id, NOWHERE)
		if not _shops.is_empty() and _rng.randf() < GOODS_PER_FACTORY * level:
			var shop: Lot = _shops[_rng.randi() % _shops.size()]
			_send(factory.id, NOWHERE, shop.id, NOWHERE)
		if not gates.is_empty() and _rng.randf() < EXPORTS_PER_FACTORY * level:
			_send(factory.id, NOWHERE, -1, _gate(_externals.get_outbound(_pick(gates))))
	if gates.is_empty():
		return
	for shop in _shops:
		if _rng.randf() < IMPORTS_PER_SHOP * level:
			_send(-1, _gate(_externals.get_inbound(_pick(gates))), shop.id, NOWHERE)


## Sends a truck from a lot's driveway (or, -1, the lane positions `from`, e.g. a highway)
## to a lot (or, -1, the quickest of `to`); quietly skipped if there's no way there.
func _send(origin_lot_id: int, from: Array[Dictionary], destination_lot_id: int, to: Array[Dictionary]) -> void:
	_vehicles.depart(origin_lot_id, from, destination_lot_id, to, OWNER_ID, Vehicle.Kind.TRUCK)


## A highway's lane position as a list of access points (empty if it has no road).
func _gate(position: Dictionary) -> Array[Dictionary]:
	var points: Array[Dictionary] = []
	if not position.is_empty():
		points.append(position)
	return points


func _pick(points: Array[Vector2]) -> Vector2:
	return points[_rng.randi() % points.size()]


func _on_lots_changed() -> void:
	_factories = _lots.get_lots_of_type(Lot.Type.INDUSTRIAL)
	_shops = _lots.get_lots_of_type(Lot.Type.COMMERCIAL)
