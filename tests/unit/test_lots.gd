extends GutTest

const G := 64.0

var roads: RoadNetwork
var lots: LotSystem


func before_each() -> void:
	roads = RoadNetwork.new()
	lots = LotSystem.new(roads)


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
