extends GutTest
## Phase 3 "done when": one in-game day plays through the four pulse periods with
## visibly different traffic: commuter cars in the rushes, trucks at midday, quiet nights.

const G := 64.0

var sim: Simulation


func before_each() -> void:
	sim = Simulation.new()


func after_each() -> void:
	sim.dispose()


func _cells(from: Vector2i, to: Vector2i) -> Array[Vector2i]:
	var cells: Array[Vector2i] = []
	for x in range(from.x, to.x + 1):
		for y in range(from.y, to.y + 1):
			cells.append(Vector2i(x, y))
	return cells


func test_a_day_has_four_distinct_traffic_periods() -> void:
	Scenarios.add_highway(sim)
	var origin := Grid.world_to_cell(sim.externals.points[0]) + Vector2i(6, 0)
	var t := RoadType.load_by_id(&"local_2lane")
	var at := func(x: int, y: int) -> Vector2: return Vector2(origin + Vector2i(x, y)) * G
	sim.roads.add_road(at.call(0, 0), at.call(16, 0), t)
	for x in [0, 4, 8, 12, 16]:
		sim.roads.add_road(at.call(x, -3), at.call(x, 3), t)
	sim.roads.add_road(at.call(0, -3), at.call(16, -3), t)
	sim.roads.add_road(at.call(0, 3), at.call(16, 3), t)
	sim.zones.paint(_cells(origin + Vector2i(0, -3), origin + Vector2i(7, 2)), Lot.Type.RESIDENTIAL)
	sim.zones.paint(_cells(origin + Vector2i(8, -3), origin + Vector2i(15, -1)), Lot.Type.COMMERCIAL)
	sim.zones.paint(_cells(origin + Vector2i(8, 0), origin + Vector2i(15, 2)), Lot.Type.INDUSTRIAL)

	# New vehicles per period: [cars, trucks].
	var counts := {}
	for period in SimClock.Period.values():
		counts[period] = [0, 0]
	var seen := {}
	var ticks := int(SimClock.MINUTES_PER_DAY / SimClock.GAME_MINUTES_PER_SIM_SECOND * Simulation.TICK_RATE)
	for i in ticks:
		sim.step()
		var period := sim.clock.get_period()
		for vehicle: Vehicle in sim.vehicles.vehicles.values():
			if not seen.has(vehicle.id):
				seen[vehicle.id] = true
				counts[period][1 if vehicle.kind == Vehicle.Kind.TRUCK else 0] += 1
	var P := SimClock.Period
	gut.p("cars/trucks per period: morning %s, midday %s, evening %s, night %s, flow %.0f%%" % [
		counts[P.MORNING_RUSH], counts[P.MIDDAY_LOGISTICS], counts[P.EVENING_RUSH], counts[P.NIGHT],
		sim.flow.get_flow_percent(sim.get_sim_time())])

	assert_gt(counts[P.MORNING_RUSH][0], counts[P.MORNING_RUSH][1], "morning: mostly commuter cars")
	assert_gt(counts[P.MIDDAY_LOGISTICS][1], counts[P.MIDDAY_LOGISTICS][0], "midday: mostly trucks")
	assert_gt(counts[P.EVENING_RUSH][0], counts[P.EVENING_RUSH][1], "evening: mostly commuter cars")
	var night_total: int = counts[P.NIGHT][0] + counts[P.NIGHT][1]
	assert_lt(night_total, counts[P.MORNING_RUSH][0], "night is quiet")
	assert_gt(sim.flow.get_flow_percent(sim.get_sim_time()), 0.0, "trips were scored")
