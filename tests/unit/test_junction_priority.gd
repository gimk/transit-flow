extends GutTest
## Phase 5c: junctions with many lanes: parallel turns, road priority, protected lefts.

const G := 64.0
const DT := Simulation.TICK_DT

var sim: Simulation
var roads: RoadNetwork


func before_each() -> void:
	sim = Simulation.new()
	roads = sim.roads


func after_each() -> void:
	sim.dispose()


func _road(from: Vector2, to: Vector2, type_id := &"local_2lane") -> void:
	roads.add_road(from * G, to * G, RoadType.load_by_id(type_id))


func _connectors(at: Vector2) -> Array[Lane]:
	var result: Array[Lane] = []
	for lane_id in roads.nodes[roads.find_node_at(at * G)].connector_lane_ids:
		result.append(roads.lanes[lane_id])
	return result


func _spawn(from: Vector2, to: Vector2, owner: int) -> Vehicle:
	var a := roads.find_lane_position(from * G, 4.0)
	var b := roads.find_lane_position(to * G, 4.0)
	return sim.vehicles.spawn(sim.router.find_route(a.lane_id, a.distance, b.lane_id, b.distance), -1, owner)


func test_straight_movements_from_both_lanes_run_side_by_side() -> void:
	_road(Vector2(0, 4), Vector2(10, 4), &"avenue_4lane")
	_road(Vector2(5, 0), Vector2(5, 8), &"avenue_4lane")
	var straight := _connectors(Vector2(5, 4)).filter(func(c: Lane) -> bool: return c.turn == Lane.Turn.STRAIGHT)
	for a: Lane in straight:
		for b: Lane in straight:
			var same_direction := LaneBuilder.start_direction(a.curve).dot(LaneBuilder.start_direction(b.curve)) > 0.9
			if a != b and same_direction:
				assert_false(a.conflicting_lane_ids.has(b.id), "parallel lanes don't conflict")


func test_side_street_gives_way_to_the_avenue() -> void:
	_road(Vector2(0, 4), Vector2(12, 4), &"avenue_4lane")
	_road(Vector2(6, 4), Vector2(6, 9))
	# The avenue driver comes along at speed; the side-street driver reaches the junction
	# first, but the avenue driver is less than a gap away, and has priority.
	var main := _spawn(Vector2(0.5, 4) + Vector2(0, 21) / G, Vector2(11, 4) + Vector2(0, 21) / G, 2)
	assert_not_null(main)
	while main.position.x < 4.3 * G:
		sim.vehicles.step(DT)
	var side := _spawn(Vector2(6, 5.2) + Vector2(7, 0) / G, Vector2(11, 4) + Vector2(0, 21) / G, 1)
	assert_not_null(side)
	var order := []
	var slowest_main := INF
	var moving := false
	for i in 600:
		sim.vehicles.step(DT)
		for vehicle: Vehicle in [side, main]:
			if sim.vehicles.vehicles.has(vehicle.id) and vehicle.lane.is_connector() and not order.has(vehicle.owner_id):
				order.append(vehicle.owner_id)
		if sim.vehicles.vehicles.has(main.id) and order.size() < 2:
			moving = moving or main.speed > 85.0
			if moving:
				slowest_main = minf(slowest_main, main.speed)
	assert_eq(order, [2, 1], "the avenue driver goes first")
	assert_gt(slowest_main, 80.0, "and doesn't slow down to look: it has priority")


func test_must_yield_follows_road_priority_first() -> void:
	_road(Vector2(0, 4), Vector2(12, 4), &"avenue_4lane")
	_road(Vector2(6, 0), Vector2(6, 9))
	var connectors := _connectors(Vector2(6, 4))
	var avenue_left: Lane = null
	var side_straight: Lane = null
	for c in connectors:
		var from_avenue: bool = roads.segments[roads.lanes[c.feeder_lane_id].segment_id].road_type.priority > 0
		if from_avenue and c.turn == Lane.Turn.LEFT:
			avenue_left = c
		if not from_avenue and c.turn == Lane.Turn.STRAIGHT:
			side_straight = c
	assert_true(sim.vehicles._must_yield(side_straight, avenue_left), "even a left turn from the avenue goes before the side street")
	assert_false(sim.vehicles._must_yield(avenue_left, side_straight))


func test_protected_left_turns_get_their_own_phase() -> void:
	_road(Vector2(0, 4), Vector2(10, 4), &"avenue_4lane")
	_road(Vector2(5, 0), Vector2(5, 8), &"avenue_4lane")
	var node_id := roads.find_node_at(Vector2(5, 4) * G)
	roads.set_node_control(node_id, RoadNode.JunctionControl.TRAFFIC_LIGHT_PROTECTED_LEFT)
	assert_eq(roads.nodes[node_id].signal_phase_count, 4)
	for c in _connectors(Vector2(5, 4)):
		var left := c.turn == Lane.Turn.LEFT
		assert_eq(c.signal_phase % 2, 1 if left else 0, "left turns on odd phases")
	# A westbound left turner and an eastbound driver going straight: never green together,
	# and the left turn gets through on its own phase.
	var turning := _spawn(Vector2(8, 4) + Vector2(0, -7) / G, Vector2(5, 7) + Vector2(7, 0) / G, 1)
	var oncoming := _spawn(Vector2(2, 4) + Vector2(0, 7) / G, Vector2(9, 4) + Vector2(0, 7) / G, 2)
	assert_eq(turning.next_lanes[0].turn, Lane.Turn.LEFT, "test setup: a left turn")
	var both_green := false
	for i in 1200:
		sim.step()
		var node := roads.nodes[node_id]
		if sim.vehicles.vehicles.has(turning.id) and sim.vehicles.vehicles.has(oncoming.id) \
				and not turning.next_lanes.is_empty() and turning.next_lanes[0].is_connector() \
				and not oncoming.next_lanes.is_empty() and oncoming.next_lanes[0].is_connector():
			both_green = both_green or (sim.signals.get_light(node, turning.next_lanes[0].signal_phase) == TrafficLights.Light.GREEN \
				and sim.signals.get_light(node, oncoming.next_lanes[0].signal_phase) == TrafficLights.Light.GREEN)
	assert_false(both_green, "the left turn never shares a green with oncoming traffic")
	assert_false(sim.vehicles.vehicles.has(turning.id), "the left turner got through")
	assert_false(sim.vehicles.vehicles.has(oncoming.id))


func test_protected_left_lights_survive_a_save() -> void:
	_road(Vector2(0, 4), Vector2(10, 4), &"avenue_4lane")
	_road(Vector2(5, 0), Vector2(5, 8), &"avenue_4lane")
	roads.set_node_control(roads.find_node_at(Vector2(5, 4) * G), RoadNode.JunctionControl.TRAFFIC_LIGHT_PROTECTED_LEFT)
	var loaded := RoadNetwork.from_dict(JSON.parse_string(JSON.stringify(roads.to_dict())))
	var node := loaded.nodes[loaded.find_node_at(Vector2(5, 4) * G)]
	assert_eq(node.control, RoadNode.JunctionControl.TRAFFIC_LIGHT_PROTECTED_LEFT)
	assert_eq(node.signal_phase_count, 4)
	loaded.dispose()
