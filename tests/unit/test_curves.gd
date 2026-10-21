extends GutTest
## Phase 9: curved roads.

const G := 64.0
const Checks := preload("res://tests/traffic_checks.gd")

var sim: Simulation
var roads: RoadNetwork


func before_each() -> void:
	sim = Simulation.new()
	roads = sim.roads


func after_each() -> void:
	sim.dispose()


func _type(id := &"local_2lane") -> RoadType:
	return RoadType.load_by_id(id)


## A quarter turn from (x, y) east then south, bending at (x + r, y), ending at (x + r, y + r).
func _quarter_turn(x: float, y: float, r: float, type_id := &"local_2lane") -> Array[int]:
	return roads.add_curved_road(Vector2(x, y) * G, Vector2(x + r, y) * G, Vector2(x + r, y + r) * G, _type(type_id))


func test_tight_curves_are_refused() -> void:
	var tight := RoadNetwork.curve_path(Vector2(2, 2) * G, Vector2(3, 2) * G, Vector2(3, 3) * G)
	assert_lt(RoadNetwork.min_radius(tight), RoadNetwork.MIN_CURVE_RADIUS, "a quarter turn in one cell")
	assert_eq(roads.plan_path(tight, 0, _type()).error, "Curve too tight (drivers need a wider turn)")
	var wide := RoadNetwork.curve_path(Vector2(2, 2) * G, Vector2(4, 2) * G, Vector2(4, 4) * G)
	assert_gt(RoadNetwork.min_radius(wide), RoadNetwork.MIN_CURVE_RADIUS, "in two cells")
	assert_eq(roads.plan_path(wide, 0, _type()).error, "")


func test_a_curved_road_has_curved_lanes_driven_slower() -> void:
	var ids := _quarter_turn(2, 2, 2)  # Radius ~90 px: slower than 90 px/s.
	assert_eq(ids.size(), 1)
	var segment := roads.segments[ids[0]]
	assert_gt(segment.centerline.point_count, 10, "sampled along the curve")
	assert_almost_eq(segment.get_length(), PI / 2.0 * 2.0 * G, 0.15 * 2.0 * G, "about a quarter circle")
	for lane_id in segment.lane_ids:
		var lane := roads.lanes[lane_id]
		assert_lt(lane.speed_limit, _type().speed_limit, "slower round the bend")
		assert_gt(lane.speed_limit, 50.0)
	# The inside lane is tighter, so slower.
	var limits: Array = segment.lane_ids.map(func(id: int) -> float: return roads.lanes[id].speed_limit)
	assert_ne(limits[0], limits[1])


func test_roads_cross_curves_and_curves_cross_roads() -> void:
	# A straight road built across a curve splits it at a junction...
	_quarter_turn(2, 2, 4)
	var across := roads.add_road_pieces(Vector2(0, 3) * G, Vector2(10, 3) * G, _type())
	assert_eq(across.size(), 2, "split where it meets the curve")
	var junction := roads.find_node_at(roads.nodes[roads.segments[across[0]].end_node_id].position)
	assert_eq(roads.nodes[junction].get_degree(), 4)
	for segment_id in roads.nodes[junction].segment_ids:
		assert_gt(roads.segments[segment_id].get_length(), 16.0)
	# ...and a curve built across straight roads is split at each of them.
	var curve := roads.add_curved_road(Vector2(0, 8) * G, Vector2(8, 8) * G, Vector2(8, 14) * G, _type())
	roads.add_road(Vector2(4, 6) * G, Vector2(4, 12) * G, _type())
	var count_before := roads.segments.size()
	assert_gt(curve.size(), 0)
	assert_eq(roads.segments.size(), count_before)
	var crossings := 0
	for node: RoadNode in roads.nodes.values():
		crossings += 1 if node.get_degree() == 4 else 0
	assert_eq(crossings, 2)


func test_curves_survive_a_save() -> void:
	_quarter_turn(2, 2, 3)
	var loaded := RoadNetwork.from_dict(JSON.parse_string(JSON.stringify(roads.to_dict())))
	var segment: RoadSegment = loaded.segments.values()[0]
	assert_eq(segment.centerline.point_count, roads.segments.values()[0].centerline.point_count)
	assert_almost_eq(segment.get_length(), roads.segments.values()[0].get_length(), 0.01)
	loaded.dispose()


## Traffic round a loop of curved and straight roads: smooth (never above the lane's
## limit, no overlaps, no conflicts), and nobody stuck.
func test_traffic_drives_round_curves() -> void:
	_quarter_turn(2, 2, 3)                                  # East then south,
	roads.add_road(Vector2(5, 5) * G, Vector2(5, 9) * G, _type())
	roads.add_curved_road(Vector2(5, 9) * G, Vector2(5, 12) * G, Vector2(2, 12) * G, _type())  # south then west,
	roads.add_road(Vector2(2, 12) * G, Vector2(-1, 12) * G, _type())
	roads.add_curved_road(Vector2(-1, 12) * G, Vector2(-4, 12) * G, Vector2(-4, 9) * G, _type())
	roads.add_road(Vector2(-4, 9) * G, Vector2(-4, 5) * G, _type())
	roads.add_curved_road(Vector2(-4, 5) * G, Vector2(-4, 2) * G, Vector2(-1, 2) * G, _type())
	roads.add_road(Vector2(-1, 2) * G, Vector2(2, 2) * G, _type())  # Closing the loop.
	roads.add_road(Vector2(0.5, 2) * G, Vector2(0.5, 12) * G, _type())  # And a road across it.
	sim.stress.target_count = 40
	var too_fast := 0
	var worst_gap := INF
	var violations := 0
	for tick in Simulation.TICK_RATE * 40:
		sim.step()
		worst_gap = minf(worst_gap, Checks.min_queue_gap(sim))
		violations += Checks.connector_conflict_violations(sim)
		for vehicle: Vehicle in sim.vehicles.vehicles.values():
			if vehicle.speed > vehicle.lane.speed_limit + 0.5:
				too_fast += 1
	assert_eq(too_fast, 0, "never above a lane's limit")
	assert_gte(worst_gap, VehicleSystem.MIN_GAP - 0.01)
	assert_eq(violations, 0)
	assert_eq(sim.vehicles.get_stuck_count(20.0), 0)


func test_lots_keep_clear_of_curves() -> void:
	_quarter_turn(2, 2, 3)
	var path := RoadNetwork.curve_path(Vector2(2, 2) * G, Vector2(5, 2) * G, Vector2(5, 5) * G)
	var full := float(Grid.CELL_SIZE * Grid.CELL_SIZE)
	var under := 0
	var beside := 0
	for x in range(1, 7):
		for y in range(1, 7):
			var cell := Vector2i(x, y)
			var gap := LaneBuilder.distance_to_polyline(Grid.cell_center(cell), path)
			var plot := LotSystem.compute_plot(roads, cell)
			if gap < 16.0:
				assert_false(plot.has_area(), "no building on the road itself, cell %s" % cell)
				under += 1
			elif plot.has_area() and plot.get_area() < full:
				beside += 1
				# The plot keeps clear of the road (its center no closer than the road's edge).
				assert_gt(LaneBuilder.distance_to_polyline(plot.get_center(), path), _type().get_total_width() * 0.5)
	assert_gt(under, 0)
	assert_gt(beside, 0, "cells beside the curve keep a smaller plot")


## Phase 9 "done when": a curved bypass changes Flow like a straight one would.
func test_a_curved_bypass_works_like_a_straight_one() -> void:
	var flows := {}
	for variant in ["none", "straight", "curved"]:
		var run := Simulation.new()
		var origin := Vector2i(30, 30)
		var info := Scenarios.build_bottleneck(run, origin)
		var at := func(x: int, y: int) -> Vector2: return Vector2(origin + Vector2i(x, y)) * Grid.CELL_SIZE
		if variant == "straight":
			Scenarios.build_bypass(run, info)
		elif variant == "curved":
			# The same way round, with its corners curved.
			var t := RoadType.load_by_id(&"local_2lane")
			run.roads.add_curved_road(at.call(-3, 0), at.call(-3, -4), at.call(1, -4), t)
			run.roads.add_road(at.call(1, -4), at.call(11, -4), t)
			run.roads.add_curved_road(at.call(11, -4), at.call(15, -4), at.call(15, 0), t)
		for tick in Simulation.TICK_RATE * 90:
			run.step()
		flows[variant] = run.flow.get_flow_percent(run.get_sim_time())
		run.dispose()
	gut.p("bottleneck flow: no bypass %.0f%%, straight bypass %.0f%%, curved bypass %.0f%%" % [flows.none, flows.straight, flows.curved])
	assert_almost_eq(flows.curved, flows.straight, 6.0, "about the same")
