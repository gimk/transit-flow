extends GutTest
## Phase 3b: highway connections and freight trucks.

const G := 64.0

var sim: Simulation
var gate: Vector2


func before_each() -> void:
	sim = Simulation.new()
	Scenarios.add_highway(sim)
	gate = sim.externals.points[0]
	# Extend the highway into a street with factories and shops on it.
	sim.roads.add_road(gate + Vector2(6, 0) * G, gate + Vector2(20, 0) * G, RoadType.load_by_id(&"local_2lane"))
	var row := Grid.world_to_cell(gate).y
	var first_x := Grid.world_to_cell(gate).x
	for x in 5:
		sim.lots.add_lot(Vector2i(first_x + 8 + x, row), Lot.Type.INDUSTRIAL)
	for x in 3:
		sim.lots.add_lot(Vector2i(first_x + 15 + x, row), Lot.Type.COMMERCIAL)


func after_each() -> void:
	sim.dispose()


func _set_hour(hour: float) -> void:
	sim.clock.total_minutes = hour * 60.0


## Number of trucks that set off during `game_hours`.
func _count_trucks(game_hours: float) -> int:
	var seen := {}
	var ticks := int(game_hours * 60.0 / SimClock.GAME_MINUTES_PER_SIM_SECOND * Simulation.TICK_RATE)
	for i in ticks:
		sim.step()
		for vehicle: Vehicle in sim.vehicles.vehicles.values():
			if vehicle.kind == Vehicle.Kind.TRUCK:
				seen[vehicle.id] = true
	return seen.size()


func test_highway_connection_has_lanes_in_and_out() -> void:
	var inbound := sim.externals.get_inbound(gate)
	var outbound := sim.externals.get_outbound(gate)
	assert_false(inbound.is_empty())
	assert_false(outbound.is_empty())
	var in_lane := sim.roads.lanes[inbound.lane_id]
	assert_gt(LaneBuilder.start_direction(in_lane.curve).x, 0.9, "inbound lane heads into the map")
	assert_eq(sim.externals.get_active_points(), [gate])


func test_connection_is_inactive_without_a_road() -> void:
	var stub := sim.roads.find_segment_at(gate + Vector2(G, 0), 4.0)
	sim.roads.remove_segment(stub)
	assert_eq(sim.externals.get_active_points(), [])
	assert_eq(sim.externals.get_inbound(gate), {})


func test_freight_peaks_at_midday() -> void:
	_set_hour(10.5)
	var midday := _count_trucks(5.0)
	_set_hour(0.5)
	var night := _count_trucks(5.0)
	gut.p("trucks in 5 game hours: %d midday, %d at night" % [midday, night])
	assert_gt(midday, 0, "trucks run at midday")
	assert_gt(midday, night * 2, "far busier at midday than at night")


func test_activity_curve() -> void:
	assert_eq(FreightSystem.activity(12.0), 1.0)
	assert_lt(FreightSystem.activity(8.0), 1.0)
	assert_lt(FreightSystem.activity(2.0), FreightSystem.activity(8.0))


func test_saves_external_connections() -> void:
	var loaded := Simulation.from_dict(JSON.parse_string(JSON.stringify(sim.to_dict(), "", true, true)))
	assert_eq(loaded.externals.points, sim.externals.points)
	loaded.dispose()
