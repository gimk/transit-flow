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


func test_light_cycle() -> void:
	var node := RoadNode.new(1, Vector2.ZERO, 0)
	node.signal_phase_count = 2
	var L := TrafficSignal.Light
	assert_eq(TrafficSignal.get_light(node, 0, 0.0), L.GREEN)
	assert_eq(TrafficSignal.get_light(node, 1, 0.0), L.RED)
	assert_eq(TrafficSignal.get_light(node, 0, TrafficSignal.GREEN_TIME + 0.5), L.AMBER)
	assert_eq(TrafficSignal.get_light(node, 1, TrafficSignal.GREEN_TIME + 0.5), L.RED)
	var second_slot := TrafficSignal.GREEN_TIME + TrafficSignal.CLEARANCE_TIME + 0.5
	assert_eq(TrafficSignal.get_light(node, 0, second_slot), L.RED)
	assert_eq(TrafficSignal.get_light(node, 1, second_slot), L.GREEN)
	node.signal_phase_count = 1
	assert_eq(TrafficSignal.get_light(node, 0, second_slot), L.GREEN, "a single phase is always green")


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


func test_red_light_holds_traffic_until_green() -> void:
	var node_id := _four_way()
	sim.roads.set_node_control(node_id, RoadNode.JunctionControl.TRAFFIC_LIGHT)
	# Southbound arrives during the east-west green (phase 0 runs first).
	var south := _spawn(Vector2(4, 2.5) + Vector2(-7, 0) / G, Vector2(4, 7) + Vector2(-7, 0) / G)
	var entered_at := -1.0
	for tick in Simulation.TICK_RATE * 20:
		sim.vehicles.step(DT)
		sim.tick_count += 1
		if entered_at < 0.0 and south.lane.is_connector():
			entered_at = sim.get_sim_time()
	var own_green := TrafficSignal.GREEN_TIME + TrafficSignal.CLEARANCE_TIME
	assert_gte(entered_at, own_green, "waited for its green")
	assert_lt(entered_at, own_green + 3.0, "went as soon as it turned green")


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
