extends GutTest
## Phase 2b: many vehicles at once: following, queuing and junction rules.

const G := 64.0
const DT := Simulation.TICK_DT

var sim: Simulation


func before_each() -> void:
	sim = Simulation.new()


func after_each() -> void:
	sim.dispose()


func _road(from: Vector2, to: Vector2) -> void:
	sim.roads.add_road(from * G, to * G, RoadType.load_by_id(&"local_2lane"))


## Spawns a vehicle between two points that lie on lanes (in grid units).
func _spawn(from: Vector2, to: Vector2, owner := 1) -> Vehicle:
	var a := sim.roads.find_lane_position(from * G, 4.0)
	var b := sim.roads.find_lane_position(to * G, 4.0)
	assert_false(a.is_empty() or b.is_empty(), "test setup: points are on lanes")
	var route := sim.router.find_route(a.lane_id, a.distance, b.lane_id, b.distance)
	assert_not_null(route, "test setup: route exists")
	return sim.vehicles.spawn(route, -1, owner)


## A grid of `count` x `count` crossing roads, `spacing` cells apart.
func _build_grid(count: int, spacing: int) -> void:
	var extent := (count - 1) * spacing
	for i in count:
		_road(Vector2(1, 1 + i * spacing), Vector2(1 + extent, 1 + i * spacing))
		_road(Vector2(1 + i * spacing, 1), Vector2(1 + i * spacing, 1 + extent))


## Bumper-to-bumper gaps between consecutive vehicles on every lane.
func _min_queue_gap() -> float:
	var smallest := INF
	for lane_id in sim.roads.lanes:
		var queue := sim.vehicles.get_lane_queue(lane_id)
		for i in range(1, queue.size()):
			smallest = minf(smallest, queue[i - 1].distance - queue[i].distance - Vehicle.LENGTH)
	return smallest


func _connector_conflict_violations() -> int:
	var on_connector := {}
	for vehicle: Vehicle in sim.vehicles.vehicles.values():
		if vehicle.lane.is_connector():
			on_connector[vehicle.lane.id] = true
	var violations := 0
	for lane_id: int in on_connector:
		var lane: Lane = sim.roads.lanes.get(lane_id)
		if lane == null:
			continue
		for other_id in lane.conflicting_lane_ids:
			if on_connector.has(other_id):
				violations += 1
	return violations


func test_follower_queues_behind_a_stopping_leader() -> void:
	_road(Vector2(0, 2), Vector2(10, 2))
	# Both stop at the same point; the follower must wait behind the leader.
	var leader := _spawn(Vector2(3, 2) + Vector2(0, 7) / G, Vector2(9, 2) + Vector2(0, 7) / G, 1)
	var follower := _spawn(Vector2(1, 2) + Vector2(0, 7) / G, Vector2(9, 2) + Vector2(0, 7) / G, 2)
	assert_not_null(leader)
	assert_not_null(follower)
	var smallest := INF
	for i in 400:
		sim.vehicles.step(DT)
		if sim.vehicles.vehicles.has(leader.id) and sim.vehicles.vehicles.has(follower.id):
			smallest = minf(smallest, leader.distance - follower.distance - Vehicle.LENGTH)
	assert_gte(smallest, VehicleSystem.MIN_GAP - 0.01, "never closer than the minimum gap")
	assert_eq(sim.vehicles.vehicles.size(), 0, "both eventually arrive")


func test_cannot_spawn_on_top_of_another_vehicle() -> void:
	_road(Vector2(0, 2), Vector2(10, 2))
	var first := _spawn(Vector2(3, 2) + Vector2(0, 7) / G, Vector2(9, 2) + Vector2(0, 7) / G)
	assert_not_null(first)
	assert_false(sim.vehicles.can_spawn_at(first.lane.id, first.distance + 5.0))
	assert_true(sim.vehicles.can_spawn_at(first.lane.id, first.distance + 40.0))
	assert_null(_spawn(Vector2(3, 2) + Vector2(5, 7) / G, Vector2(9, 2) + Vector2(0, 7) / G))


func test_crossing_movements_take_turns() -> void:
	_road(Vector2(0, 4), Vector2(8, 4))
	_road(Vector2(4, 0), Vector2(4, 8))
	# Eastbound and southbound, same distance from the junction: they'd collide.
	var east := _spawn(Vector2(1, 4) + Vector2(0, 7) / G, Vector2(7, 4) + Vector2(0, 7) / G, 1)
	var south := _spawn(Vector2(4, 1) + Vector2(-7, 0) / G, Vector2(4, 7) + Vector2(-7, 0) / G, 2)
	var someone_waited := false
	var both_inside := 0
	for i in 600:
		sim.vehicles.step(DT)
		someone_waited = someone_waited or east.requested_connector != null or south.requested_connector != null
		if east.lane.is_connector() and south.lane.is_connector() \
				and sim.vehicles.vehicles.has(east.id) and sim.vehicles.vehicles.has(south.id):
			both_inside += 1
	assert_true(someone_waited, "one of them had to wait at the stop line")
	assert_eq(both_inside, 0, "never both inside the junction")
	assert_eq(sim.vehicles.vehicles.size(), 0, "both got through")


func test_first_to_arrive_goes_first() -> void:
	_road(Vector2(0, 4), Vector2(8, 4))
	_road(Vector2(4, 0), Vector2(4, 8))
	var east := _spawn(Vector2(2, 4) + Vector2(0, 7) / G, Vector2(7, 4) + Vector2(0, 7) / G, 1)
	var south := _spawn(Vector2(4, 0.5) + Vector2(-7, 0) / G, Vector2(4, 7) + Vector2(-7, 0) / G, 2)
	var order := []
	for i in 600:
		sim.vehicles.step(DT)
		for vehicle: Vehicle in [east, south]:
			if vehicle.lane.is_connector() and not order.has(vehicle.owner_id):
				order.append(vehicle.owner_id)
	assert_eq(order, [1, 2], "the closer vehicle crosses first")


func test_stress_grid_never_overlaps_or_deadlocks() -> void:
	_build_grid(5, 3)
	sim.stress.target_count = 250
	var arrived := { "count": 0 }
	sim.vehicles.vehicle_arrived.connect(func(_v: Vehicle) -> void: arrived.count += 1)
	var worst_gap := INF
	var violations := 0
	var longest_in_box := 0.0
	for tick in Simulation.TICK_RATE * 120:
		sim.step()
		worst_gap = minf(worst_gap, _min_queue_gap())
		violations += _connector_conflict_violations()
		for vehicle: Vehicle in sim.vehicles.vehicles.values():
			if vehicle.lane.is_connector():
				longest_in_box = maxf(longest_in_box, vehicle.stopped_time)
	gut.p("vehicles %d, arrivals %d, stuck %d, smallest gap %.2f, longest stop in a junction %.1f s" % [
		sim.vehicles.vehicles.size(), arrived.count, sim.vehicles.get_stuck_count(), worst_gap, longest_in_box])
	assert_gt(sim.vehicles.vehicles.size(), 200, "traffic ramped up")
	assert_gte(worst_gap, VehicleSystem.MIN_GAP - 0.01, "no overlaps in any queue")
	assert_eq(violations, 0, "no two vehicles on conflicting connectors")
	assert_lt(longest_in_box, 10.0, "nobody gets stuck inside a junction (don't block the box)")
	assert_gt(arrived.count, 250, "traffic keeps flowing")
	assert_eq(sim.vehicles.get_stuck_count(30.0), 0, "no permanent deadlock")
