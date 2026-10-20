extends GutTest
## Phase 2c: stop signs and traffic lights.

const G := 64.0
const DT := Simulation.TICK_DT

var sim: Simulation


func before_each() -> void:
	sim = Simulation.new()


func after_each() -> void:
	sim.dispose()


func _road(from: Vector2, to: Vector2) -> void:
	sim.roads.add_road(from * G, to * G, RoadType.load_by_id(&"local_2lane"))


func _four_way() -> int:
	_road(Vector2(0, 4), Vector2(8, 4))
	_road(Vector2(4, 0), Vector2(4, 8))
	return sim.roads.find_node_at(Vector2(4, 4) * G)


func _spawn(from: Vector2, to: Vector2, owner := 1) -> Vehicle:
	var a := sim.roads.find_lane_position(from * G, 4.0)
	var b := sim.roads.find_lane_position(to * G, 4.0)
	var route := sim.router.find_route(a.lane_id, a.distance, b.lane_id, b.distance)
	return sim.vehicles.spawn(route, -1, owner)


func test_four_way_phases_follow_the_road_axes() -> void:
	var node_id := _four_way()
	for segment_id in sim.roads.nodes[node_id].segment_ids:
		for lane_id in sim.roads.segments[segment_id].lane_ids:
			var lane := sim.roads.lanes[lane_id]
			if lane.to_node_id == node_id:
				var east_west := absf(LaneBuilder.end_direction(lane.curve).x) > 0.5
				assert_eq(lane.signal_phase, 0 if east_west else 1)
	assert_eq(sim.roads.nodes[node_id].signal_phase_count, 2)


func test_only_junctions_can_be_controlled() -> void:
	_road(Vector2(0, 4), Vector2(4, 4))
	_road(Vector2(4, 4), Vector2(4, 8))
	var corner := sim.roads.find_node_at(Vector2(4, 4) * G)
	assert_false(sim.roads.set_node_control(corner, RoadNode.JunctionControl.STOP_SIGN))
	var node_id := _four_way()
	watch_signals(sim.roads)
	assert_true(sim.roads.set_node_control(node_id, RoadNode.JunctionControl.TRAFFIC_LIGHT))
	assert_signal_emitted(sim.roads, "changed")


func test_control_is_saved_and_dropped_when_the_junction_goes_away() -> void:
	var node_id := _four_way()
	sim.roads.set_node_control(node_id, RoadNode.JunctionControl.TRAFFIC_LIGHT)
	var loaded := RoadNetwork.from_dict(JSON.parse_string(JSON.stringify(sim.roads.to_dict())))
	assert_eq(loaded.nodes[loaded.find_node_at(Vector2(4, 4) * G)].control, RoadNode.JunctionControl.TRAFFIC_LIGHT)
	loaded.dispose()
	# Down to a corner: no junction left to control.
	var to_remove: Array = []
	for segment_id in sim.roads.nodes[node_id].segment_ids:
		var other := sim.roads.segments[segment_id].get_other_node_id(node_id)
		if sim.roads.nodes[other].position.x > 4 * G or sim.roads.nodes[other].position.y < 4 * G:
			to_remove.append(segment_id)
	for segment_id in to_remove:
		sim.roads.remove_segment(segment_id)
	assert_eq(sim.roads.nodes[node_id].control, RoadNode.JunctionControl.NONE)


func test_vehicle_sharing_the_green_does_not_stop() -> void:
	var node_id := _four_way()
	sim.roads.set_node_control(node_id, RoadNode.JunctionControl.TRAFFIC_LIGHT)
	var east := _spawn(Vector2(1, 4) + Vector2(0, 7) / G, Vector2(7, 4) + Vector2(0, 7) / G)
	var entry_speed := -1.0
	for tick in Simulation.TICK_RATE * 6:
		sim.vehicles.step(DT)
		sim.tick_count += 1
		if entry_speed < 0.0 and east.lane.is_connector():
			entry_speed = east.speed
	assert_gt(entry_speed, 20.0, "drove into the junction without stopping")


func test_stop_sign_requires_a_full_stop() -> void:
	var node_id := _four_way()
	sim.roads.set_node_control(node_id, RoadNode.JunctionControl.STOP_SIGN)
	var east := _spawn(Vector2(1, 4) + Vector2(0, 7) / G, Vector2(7, 4) + Vector2(0, 7) / G)
	var slowest_before := INF
	for tick in Simulation.TICK_RATE * 10:
		sim.vehicles.step(DT)
		if not sim.vehicles.vehicles.has(east.id):
			break
		if not east.lane.is_connector() and east.lane.to_node_id == node_id:
			slowest_before = minf(slowest_before, east.speed)
	assert_lt(slowest_before, VehicleSystem.STOPPED_SPEED, "stopped at the line")
	assert_false(sim.vehicles.vehicles.has(east.id), "then carried on and arrived")


## Steps the whole simulation (signals included) for `seconds`, calling `each_tick`.
func _run(seconds: float, each_tick: Callable = Callable()) -> void:
	for tick in int(seconds * Simulation.TICK_RATE):
		sim.step()
		if each_tick.is_valid():
			each_tick.call()


func _spawn_eastbound_stream(count: int) -> void:
	for i in count:
		_spawn(Vector2(0.3 + i * 0.4, 4) + Vector2(0, 7) / G, Vector2(7.5, 4) + Vector2(0, 7) / G, 100 + i)


func test_busy_approach_without_competition_keeps_its_green() -> void:
	var node_id := _four_way()
	sim.roads.set_node_control(node_id, RoadNode.JunctionControl.TRAFFIC_LIGHT)
	var node := sim.roads.nodes[node_id]
	_spawn_eastbound_stream(6)
	var saw_red := { "value": false }
	_run(20.0, func() -> void:
		saw_red.value = saw_red.value or sim.signals.get_light(node, 0) != TrafficLights.Light.GREEN)
	assert_false(saw_red.value, "nobody else waits, so east-west never turns red")
	assert_eq(sim.vehicles.vehicles.size(), 0, "the whole stream got through")


func test_light_turns_for_a_lone_waiting_vehicle() -> void:
	var node_id := _four_way()
	sim.roads.set_node_control(node_id, RoadNode.JunctionControl.TRAFFIC_LIGHT)
	# East-west starts green with nobody on it; one car arrives southbound.
	var south := _spawn(Vector2(4, 2.5) + Vector2(-7, 0) / G, Vector2(4, 7) + Vector2(-7, 0) / G)
	var entered := { "at": -1.0 }
	_run(10.0, func() -> void:
		if entered.at < 0.0 and south.lane.is_connector():
			entered.at = sim.get_sim_time())
	assert_gte(entered.at, TrafficLights.MIN_GREEN, "waited for the minimum green to end")
	assert_lt(entered.at, TrafficLights.MIN_GREEN + TrafficLights.CLEARANCE_TIME + 2.0, "then got green quickly")


func test_continuous_stream_yields_after_max_green() -> void:
	var node_id := _four_way()
	sim.roads.set_node_control(node_id, RoadNode.JunctionControl.TRAFFIC_LIGHT)
	var south := _spawn(Vector2(4, 2.5) + Vector2(-7, 0) / G, Vector2(4, 7) + Vector2(-7, 0) / G, 1)
	var entered := { "at": -1.0, "spawned": 0 }
	_run(25.0, func() -> void:
		# Keep feeding east-west so its approach is never empty.
		if sim.tick_count % 10 == 0 and _spawn(Vector2(0.3, 4) + Vector2(0, 7) / G, Vector2(7.5, 4) + Vector2(0, 7) / G, 50) != null:
			entered.spawned += 1
		if entered.at < 0.0 and sim.vehicles.vehicles.has(south.id) and south.lane.is_connector():
			entered.at = sim.get_sim_time())
	assert_gt(entered.spawned, 10, "east-west really was busy")
	assert_gte(entered.at, TrafficLights.MIN_GREEN, "east-west kept its green a while")
	assert_lt(entered.at, TrafficLights.MAX_GREEN + TrafficLights.CLEARANCE_TIME + 2.0, "but not past the maximum")
