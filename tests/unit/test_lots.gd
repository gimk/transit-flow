extends GutTest

const G := 64.0

var roads: RoadNetwork
var lots: LotSystem


func before_each() -> void:
	roads = RoadNetwork.new()
	lots = LotSystem.new(roads)


func after_each() -> void:
	roads.dispose()


func _road(from: Vector2, to: Vector2) -> int:
	return roads.add_road(from * G, to * G, RoadType.load_by_id(&"local_2lane"))


func test_add_and_remove_lot() -> void:
	watch_signals(lots)
	var id := lots.add_lot(Vector2i(3, 3), Lot.Type.RESIDENTIAL)
	assert_ne(id, -1)
	assert_eq(lots.get_lot_at_cell(Vector2i(3, 3)), id)
	assert_true(lots.remove_lot(id))
	assert_eq(lots.get_lot_at_cell(Vector2i(3, 3)), -1)
	assert_signal_emit_count(lots, "changed", 2)


func test_cell_can_only_hold_one_lot() -> void:
	lots.add_lot(Vector2i(3, 3), Lot.Type.RESIDENTIAL)
	assert_eq(lots.add_lot(Vector2i(3, 3), Lot.Type.INDUSTRIAL), -1)


func test_cannot_place_outside_map() -> void:
	assert_eq(lots.add_lot(Vector2i(-1, 0), Lot.Type.RESIDENTIAL), -1)
	assert_eq(lots.add_lot(Grid.MAP_SIZE_CELLS, Lot.Type.RESIDENTIAL), -1)


func test_lot_without_road_has_no_access() -> void:
	var id := lots.add_lot(Vector2i(3, 3), Lot.Type.RESIDENTIAL)
	assert_eq(lots.get_access(id), {})


func test_access_is_the_lane_on_the_lots_side_of_the_road() -> void:
	# Road along y=4 (the bottom edge of cell row 3); the lot sits north of it.
	var segment := _road(Vector2(0, 4), Vector2(8, 4))
	var id := lots.add_lot(Vector2i(3, 3), Lot.Type.RESIDENTIAL)
	var access := lots.get_access(id)
	assert_false(access.is_empty())
	var lane := roads.lanes[access.lane_id]
	assert_eq(lane.segment_id, segment)
	# North side of an east-west road = the westbound lane (drives on the right).
	assert_lt(LaneBuilder.end_direction(lane.curve).x, 0.0)
	assert_almost_eq(lane.curve.sample_baked(access.distance).x, 3.5 * G, 1.0, "door faces the lot's center")


func test_access_points_cover_both_directions() -> void:
	_road(Vector2(0, 4), Vector2(8, 4))
	var id := lots.add_lot(Vector2i(3, 3), Lot.Type.RESIDENTIAL)
	var points := lots.get_access_points(id)
	assert_eq(points.size(), 2)
	assert_eq(points[0], lots.get_access(id), "near lane first")
	var far := roads.lanes[points[1].lane_id]
	assert_gt(LaneBuilder.end_direction(far.curve).x, 0.0, "then the eastbound lane across the road")
	assert_almost_eq(far.curve.sample_baked(points[1].distance).x, 3.5 * G, 1.0)


## A driver may turn out of a driveway either way: heading west from a lot on the north
## side of the road, then east to a lot further along, they pick the eastbound lane
## instead of driving to the end of the road and turning around.
func test_trips_leave_and_arrive_by_the_quicker_side() -> void:
	_road(Vector2(0, 4), Vector2(12, 4))
	var router := Router.new(roads)
	var home := lots.add_lot(Vector2i(2, 3), Lot.Type.RESIDENTIAL)
	var work := lots.add_lot(Vector2i(9, 3), Lot.Type.INDUSTRIAL)
	var near_only := router.find_route(lots.get_access(home).lane_id, lots.get_access(home).distance,
			lots.get_access(work).lane_id, lots.get_access(work).distance)
	var best := router.find_best_route(lots.get_access_points(home), lots.get_access_points(work))
	assert_eq(best.lane_ids.size(), 1, "straight along one lane")
	assert_eq(best.lane_ids[0], lots.get_access_points(home)[1].lane_id)
	assert_lt(best.travel_time * 2.0, near_only.travel_time, "much quicker than the U-turn trip")


func test_access_follows_road_changes() -> void:
	var id := lots.add_lot(Vector2i(3, 3), Lot.Type.RESIDENTIAL)
	var segment := _road(Vector2(0, 4), Vector2(8, 4))
	assert_false(lots.get_access(id).is_empty(), "road built after the lot")
	roads.remove_segment(segment)
	assert_eq(lots.get_access(id), {}, "road bulldozed")


func test_save_round_trip() -> void:
	lots.add_lot(Vector2i(1, 2), Lot.Type.RESIDENTIAL)
	var removed := lots.add_lot(Vector2i(5, 5), Lot.Type.INDUSTRIAL)
	lots.add_lot(Vector2i(7, 1), Lot.Type.INDUSTRIAL)
	lots.remove_lot(removed)
	var saved := lots.to_dict()
	var loaded := LotSystem.new(RoadNetwork.new())
	loaded.restore(JSON.parse_string(JSON.stringify(saved)))
	assert_eq_deep(loaded.to_dict(), saved)
	assert_eq(loaded.lots[loaded.get_lot_at_cell(Vector2i(7, 1))].type, Lot.Type.INDUSTRIAL)


func test_plots_leave_room_for_the_road_and_sidewalk() -> void:
	_road(Vector2(0, 4), Vector2(12, 4))
	var lot := lots.lots[lots.add_lot(Vector2i(3, 3), Lot.Type.COMMERCIAL)]
	var cell := Rect2(Vector2(3, 3) * G, Vector2.ONE * G)
	var taken := RoadType.load_by_id(&"local_2lane").get_total_width() * 0.5 + LotSystem.ROAD_CLEARANCE
	assert_eq(lot.plot.position, cell.position, "untouched on the sides away from the road")
	assert_almost_eq(lot.plot.end.y, cell.end.y - taken, 0.01, "the road's side gives way")
	assert_eq(lot.plot.end.x, cell.end.x)


## Lots between two roads one cell apart fit beside local streets, but not beside an
## avenue: upgrading one removes them (zoned land waits for room instead).
func test_an_avenue_too_close_removes_the_lots_between() -> void:
	var sim := Simulation.new()
	var local := RoadType.load_by_id(&"local_2lane")
	sim.roads.add_road(Vector2(0, 4) * G, Vector2(12, 4) * G, local)
	sim.roads.add_road(Vector2(0, 5) * G, Vector2(12, 5) * G, local)
	var cells: Array[Vector2i] = [Vector2i(3, 4), Vector2i(4, 4)]
	sim.zones.paint(cells, Lot.Type.RESIDENTIAL)
	sim.zones.develop_all_now()
	assert_ne(sim.lots.get_lot_at_cell(Vector2i(3, 4)), -1, "room between two local streets")
	assert_gt(sim.lots.lots[sim.lots.get_lot_at_cell(Vector2i(3, 4))].plot.size.y, LotSystem.MIN_PLOT)
	sim.roads.add_road(Vector2(0, 5) * G, Vector2(12, 5) * G, RoadType.load_by_id(&"avenue_4lane"))
	assert_eq(sim.lots.get_lot_at_cell(Vector2i(3, 4)), -1, "the avenue took the land")
	assert_eq(sim.lots.get_lot_at_cell(Vector2i(4, 4)), -1)
	assert_eq(sim.citizens.citizens.size(), 0, "and its residents moved out")
	assert_true(sim.zones.zones.has(Vector2i(3, 4)), "the zone stays")
	for i in 100:
		sim.step()
	assert_true(sim.zones.is_waiting_for_road(Vector2i(3, 4)), "waiting for room")
	assert_eq(sim.lots.get_lot_at_cell(Vector2i(3, 4)), -1)
	assert_eq(sim.lots.add_lot(Vector2i(5, 4), Lot.Type.COMMERCIAL), -1, "nor can one be placed there")
	sim.dispose()


func test_a_corner_lots_driveway_is_set_back_from_the_junction() -> void:
	# Crossroads at (4, 4); the lot in the cell north-west of it.
	_road(Vector2(0, 4), Vector2(8, 4))
	_road(Vector2(4, 0), Vector2(4, 8))
	var id := lots.add_lot(Vector2i(3, 3), Lot.Type.COMMERCIAL)
	var points := lots.get_access_points(id)
	assert_gt(points.size(), 0)
	for point in points:
		var lane := roads.lanes[point.lane_id]
		assert_gte(point.distance, LotSystem.DRIVEWAY_SETBACK - 0.01, "clear of the junction it starts at")
		assert_lte(point.distance, lane.length - LotSystem.DRIVEWAY_SETBACK + 0.01, "clear of the junction it ends at")
