extends GutTest
## Phase 2c "done when": a single bottleneck junction visibly jams, a parallel road
## draws drivers out of the jam, and rerouting is observable. Uses the same scenario as F7 in game.

const ORIGIN := Vector2i(30, 30)

var sim: Simulation
var info: Dictionary


func before_each() -> void:
	sim = Simulation.new()
	info = Scenarios.build_bottleneck(sim, ORIGIN)


func after_each() -> void:
	sim.dispose()


## The main-road lane arriving at the lights from the west.
func _approach_lane() -> Lane:
	var junction := sim.roads.find_node_at(info.junction)
	for lane: Lane in sim.roads.lanes.values():
		if not lane.is_connector() and lane.to_node_id == junction \
				and LaneBuilder.end_direction(lane.curve).x > 0.9:
			return lane
	return null


## Citizens that have reached work at least once (they leave again after a while).
var _reached_work := {}


func _everyone_reached_work() -> bool:
	for citizen: Citizen in sim.citizens.citizens.values():
		if citizen.state == Citizen.State.AT_WORK:
			_reached_work[citizen.id] = true
	return _reached_work.size() == sim.citizens.citizens.size()


## Runs until every commuter has reached work. Returns the sim time (INF on timeout), the
## total time commuters spent driving to work, and how many of them drove over any of
## `watched_segments`.
func _run_morning(watched_segments: Array = []) -> Dictionary:
	_reached_work.clear()
	var seen := {}
	var driving := { "total": 0.0 }
	sim.vehicles.vehicle_arrived.connect(_add_commute.bind(driving))
	for tick in Simulation.TICK_RATE * 300:
		sim.step()
		for vehicle: Vehicle in sim.vehicles.vehicles.values():
			var commuting: Citizen = sim.citizens.citizens.get(vehicle.owner_id)
			var to_work := commuting != null and commuting.state == Citizen.State.TO_WORK
			if to_work and watched_segments.has(vehicle.lane.segment_id):
				seen[vehicle.owner_id] = true
		if _everyone_reached_work():
			return { "time": sim.get_sim_time(), "driving": driving.total, "used_watched": seen.size() }
	return { "time": INF, "driving": driving.total, "used_watched": seen.size() }


func _add_commute(vehicle: Vehicle, driving: Dictionary) -> void:
	var citizen: Citizen = sim.citizens.citizens.get(vehicle.owner_id)
	if citizen != null and vehicle.destination_lot_id == citizen.work_lot_id:
		driving.total += vehicle.trip_end_time - vehicle.trip_start_time


func test_scenario_builds() -> void:
	assert_eq(sim.citizens.citizens.size(), 56)
	assert_eq(sim.roads.nodes[sim.roads.find_node_at(info.junction)].control, RoadNode.JunctionControl.TRAFFIC_LIGHT)
	for lot: Lot in sim.lots.lots.values():
		assert_false(sim.lots.get_access(lot.id).is_empty(), "every lot reaches a road")


func test_queue_builds_at_the_lights_and_shows_as_congestion() -> void:
	var approach := _approach_lane()
	var longest := 0
	var worst_congestion := 0.0
	for tick in Simulation.TICK_RATE * 30:
		sim.step()
		longest = maxi(longest, approach.queue.size())
		worst_congestion = maxf(worst_congestion, sim.congestion.congestion.get(approach.id, 0.0))
	gut.p("longest queue on the approach: %d, worst congestion there: %.2f" % [longest, worst_congestion])
	assert_gte(longest, 6, "a queue forms")
	assert_gt(worst_congestion, 0.5, "the approach is marked congested")


## The simulation is deterministic but chaotic (small differences in who leaves when
## change which drivers get stuck), so compare over a few departure orders.
func test_bypass_draws_drivers_out_of_the_jam() -> void:
	var total_without := 0.0
	var total_with := 0.0
	var took_bypass := 0
	for seed in [1, 2, 3]:
		var without := _morning_for_seed(seed, false)
		var with_bypass := _morning_for_seed(seed, true)
		gut.p("seed %d: everyone at work %.1f s / commuting %.0f s without bypass, %.1f s / %.0f s with (%d took it)" % [
			seed, without.time, without.driving, with_bypass.time, with_bypass.driving, with_bypass.used_watched])
		assert_lt(without.time, INF, "the morning finishes even without the bypass")
		total_without += without.driving
		total_with += with_bypass.driving
		took_bypass += with_bypass.used_watched
	# Every factory sits on the one main road, so no bypass adds capacity where the cars
	# end up: the bypass spreads the load (drivers held up in the queue take it) but can't
	# do much better than break even. It must not make the morning worse.
	assert_lt(total_with, total_without * 1.02, "the bypass doesn't make commuting slower")
	assert_gte(took_bypass, 5, "congestion pushed traffic onto the bypass")


## Rebuilds the scenario with a given departure order (and optionally the bypass), then
## runs the morning, counting commuters who drove over the bypass.
func _morning_for_seed(seed: int, bypass: bool) -> Dictionary:
	sim.dispose()
	sim = Simulation.new()
	sim.citizens._rng.seed = seed
	info = Scenarios.build_bottleneck(sim, ORIGIN)
	var before := sim.roads.segments.keys()
	if bypass:
		Scenarios.build_bypass(sim, info)
	var bypass_segments := sim.roads.segments.keys().filter(
		func(id: int) -> bool: return not before.has(id) and not _on_main_road(sim.roads.segments[id]))
	return _run_morning(bypass_segments)


func _on_main_road(segment: RoadSegment) -> bool:
	var y: float = info.junction.y
	var start_y := sim.roads.nodes[segment.start_node_id].position.y
	var end_y := sim.roads.nodes[segment.end_node_id].position.y
	return is_equal_approx(start_y, y) and is_equal_approx(end_y, y)
