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
