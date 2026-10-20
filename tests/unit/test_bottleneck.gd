extends GutTest
## Phase 2c "done when": a single bottleneck junction visibly jams, a parallel road
## relieves it, and rerouting is observable. Uses the same scenario as F7 in game.

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


## Runs until every commuter has reached work. Returns the sim time (INF on timeout)
## and how many commuters drove to work over any of `watched_segments`.
func _run_morning(watched_segments: Array = []) -> Dictionary:
	_reached_work.clear()
	var seen := {}
	for tick in Simulation.TICK_RATE * 300:
		sim.step()
		for vehicle: Vehicle in sim.vehicles.vehicles.values():
			var commuting: Citizen = sim.citizens.citizens.get(vehicle.owner_id)
			var to_work := commuting != null and commuting.state == Citizen.State.TO_WORK
			if to_work and watched_segments.has(vehicle.lane.segment_id):
				seen[vehicle.owner_id] = true
		if _everyone_reached_work():
			return { "time": sim.get_sim_time(), "used_watched": seen.size() }
	return { "time": INF, "used_watched": seen.size() }


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
func test_bypass_relieves_the_jam() -> void:
	var total_without := 0.0
	var total_with := 0.0
	var took_bypass := 0
	for seed in [1, 2, 3]:
		var without := _morning_for_seed(seed, false)
		var with_bypass := _morning_for_seed(seed, true)
		gut.p("seed %d: everyone at work %.1f s without bypass, %.1f s with (%d took it)" % [
			seed, without.time, with_bypass.time, with_bypass.used_watched])
		assert_lt(without.time, INF, "the morning finishes even without the bypass")
		total_without += without.time
		total_with += with_bypass.time
		took_bypass += with_bypass.used_watched
	# The jam comes from one lane carrying the whole suburb, so the bypass mostly spreads
	# the load; it must not make things worse.
	assert_lt(total_with, total_without, "the morning is shorter with the bypass")
	assert_gte(took_bypass, 10, "congestion pushed traffic onto the bypass")


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
