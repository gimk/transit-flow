extends GutTest
## The Town scenario end to end: a whole day of a complete small city, checking that the
## daily rhythm completes, traffic stays safe and nothing deadlocks.

const Checks := preload("res://tests/traffic_checks.gd")
const DAY_TICKS := Simulation.TICK_RATE * SimClock.MINUTES_PER_DAY / int(SimClock.GAME_MINUTES_PER_SIM_SECOND)

var sim: Simulation
var info: Dictionary


func before_each() -> void:
	sim = Simulation.new()
	info = Scenarios.build_town(sim)


func after_each() -> void:
	sim.dispose()


func _count_lots(type: Lot.Type) -> int:
	return sim.lots.get_lots_of_type(type).size()


func _count_citizens(state: Citizen.State) -> int:
	return sim.citizens.citizens.values().filter(func(c: Citizen) -> bool: return c.state == state).size()


func test_town_is_built_and_developed() -> void:
	assert_eq(_count_lots(Lot.Type.RESIDENTIAL), 88)
	assert_eq(_count_lots(Lot.Type.COMMERCIAL), 38)
	assert_eq(_count_lots(Lot.Type.INDUSTRIAL), 37)
	for cell in sim.zones.zones:
		assert_true(sim.zones.is_developed(cell), "zoned cell %s developed at once" % cell)
	assert_eq(sim.externals.get_active_points().size(), 2, "both highways connected")
	for point: Vector2 in info.lights:
		assert_eq(sim.roads.nodes[sim.roads.find_node_at(point)].control, RoadNode.JunctionControl.TRAFFIC_LIGHT)
	sim.step()
	assert_eq(sim.citizens.citizens.size(), 176)
	assert_lte(sim.citizens.citizens.size(), sim.citizens.get_job_count(), "a job for everyone")


func test_town_survives_a_save() -> void:
	var loaded := Simulation.from_dict(JSON.parse_string(JSON.stringify(sim.to_dict())))
	assert_eq(loaded.lots.lots.size(), sim.lots.lots.size())
	assert_eq(loaded.zones.zones.size(), sim.zones.zones.size())
	assert_eq(loaded.externals.get_active_points().size(), 2)
	var lights := loaded.roads.nodes.values().filter(
		func(n: RoadNode) -> bool: return n.control == RoadNode.JunctionControl.TRAFFIC_LIGHT)
	assert_eq(lights.size(), info.lights.size())
	loaded.dispose()


## One full day from 06:00: the morning rush jams but everyone gets to work, trucks use
## both highways, everyone is home by night, and the safety rules hold on every tick.
func test_town_lives_through_a_whole_day() -> void:
	var stats := { "commutes": {}, "trucks": 0, "failed": 0, "exports": {} }
	sim.vehicles.vehicle_arrived.connect(_on_arrived.bind(stats))
	sim.vehicles.vehicle_failed.connect(func(_v: Vehicle) -> void: stats.failed += 1)
	var everyone_at_work := -1.0
	var violations := 0
	var worst_gap := INF
	var peak := 0
	var most_stuck := 0
	for tick in DAY_TICKS:
		sim.step()
		violations += Checks.connector_conflict_violations(sim)
		worst_gap = minf(worst_gap, Checks.min_queue_gap(sim))
		peak = maxi(peak, sim.vehicles.vehicles.size())
		if everyone_at_work < 0.0 and _count_citizens(Citizen.State.AT_WORK) == sim.citizens.citizens.size():
			everyone_at_work = sim.clock.get_time_of_day_hours()
		if tick % Simulation.TICK_RATE == 0:
			most_stuck = maxi(most_stuck, sim.vehicles.get_stuck_count(30.0))

	var commutes: Array = stats.commutes.values()
	commutes.sort()
	var median_ratio: float = commutes[commutes.size() / 2]
	var flow := sim.flow.get_flow_percent(sim.get_sim_time())
	gut.p("town: peak %d vehicles, everyone at work at %.1f h, median commute %.1fx ideal, %d trucks, %d failed, flow %.0f%%" % [
		peak, everyone_at_work, median_ratio, stats.trucks, stats.failed, flow])
	assert_eq(violations, 0, "never two vehicles where conflicting paths meet")
	assert_gte(worst_gap, VehicleSystem.MIN_GAP - 0.01, "no overlaps in any queue")
	assert_eq(most_stuck, 0, "nobody ever stopped for 30 s (no deadlock)")
	assert_eq(commutes.size(), sim.citizens.citizens.size(), "every commuter reached work")
	assert_between(everyone_at_work, 9.0, 16.0, "the morning rush is over before the evening one")
	assert_gt(median_ratio, 1.2, "the morning rush is congested (that's the point of the scenario)")
	assert_eq(_count_citizens(Citizen.State.AT_HOME), sim.citizens.citizens.size(), "everyone home by 06:00")
	assert_gt(stats.trucks, 150, "freight runs all day")
	assert_eq(stats.exports.size(), 2, "exports leave by both highways")
	assert_lt(stats.failed, 10, "almost every trip succeeds")
	assert_gt(flow, 60.0)


func _on_arrived(vehicle: Vehicle, stats: Dictionary) -> void:
	if vehicle.kind == Vehicle.Kind.TRUCK:
		stats.trucks += 1
		if vehicle.destination_lot_id == -1:
			stats.exports[vehicle.lane.to_node_id] = true
	elif vehicle.destination_lot_id == sim.citizens.citizens[vehicle.owner_id].work_lot_id and not stats.commutes.has(vehicle.owner_id):
		# The morning commute (shop workers may later shop where they work, too).
		stats.commutes[vehicle.owner_id] = (vehicle.trip_end_time - vehicle.trip_start_time) / vehicle.ideal_trip_time
