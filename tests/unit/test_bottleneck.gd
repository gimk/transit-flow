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


## The main-road lane arriving at the stop sign from the west.
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
	assert_eq(sim.roads.nodes[sim.roads.find_node_at(info.junction)].control, RoadNode.JunctionControl.STOP_SIGN)
	for lot: Lot in sim.lots.lots.values():
		assert_false(sim.lots.get_access(lot.id).is_empty(), "every lot reaches a road")


func test_queue_builds_at_the_stop_sign_and_shows_as_congestion() -> void:
	var approach := _approach_lane()
	var longest := 0
	for tick in Simulation.TICK_RATE * 30:
		sim.step()
		longest = maxi(longest, approach.queue.size())
	gut.p("longest queue on the approach: %d, congestion there: %.2f" % [longest, sim.congestion.congestion.get(approach.id, 0.0)])
	assert_gte(longest, 6, "a queue forms")
	assert_gt(sim.congestion.congestion.get(approach.id, 0.0), 1.0, "the approach is marked congested")


func test_bypass_relieves_the_jam() -> void:
	var without := _run_morning()
	sim.dispose()

	sim = Simulation.new()
	info = Scenarios.build_bottleneck(sim, ORIGIN)
	var before := sim.roads.segments.keys()
	Scenarios.build_bypass(sim, info)
	var bypass_segments := []
	for segment_id in sim.roads.segments:
		var segment := sim.roads.segments[segment_id]
		# Bypass pieces are new and lie off the main road (y != junction row).
		if not before.has(segment_id) and absf(sim.roads.nodes[segment.start_node_id].position.y - info.junction.y) + \
				absf(sim.roads.nodes[segment.end_node_id].position.y - info.junction.y) > 0.0:
			bypass_segments.append(segment_id)
	var with_bypass := _run_morning(bypass_segments)

	gut.p("everyone at work: %.1f s without bypass, %.1f s with (%d vehicles took it)" % [
		without.time, with_bypass.time, with_bypass.used_watched])
	assert_lt(without.time, INF, "the morning finishes even without the bypass")
	assert_lt(with_bypass.time, without.time * 0.85, "the bypass makes the morning clearly shorter")
	assert_gte(with_bypass.used_watched, 3, "congestion pushed some traffic onto the bypass")
