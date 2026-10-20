extends GutTest
## Phase 3a: zoning and buildings developing on zoned land.

const G := 64.0

var sim: Simulation


func before_each() -> void:
	sim = Simulation.new()
	sim.roads.add_road(Vector2(0, 4) * G, Vector2(10, 4) * G, RoadType.load_by_id(&"local_2lane"))


func after_each() -> void:
	sim.dispose()


func _cells(from: Vector2i, to: Vector2i) -> Array[Vector2i]:
	var cells: Array[Vector2i] = []
	for x in range(from.x, to.x + 1):
		for y in range(from.y, to.y + 1):
			cells.append(Vector2i(x, y))
	return cells


## Advances the zone clock well past every pending development (in several steps, as
## developments per tick are capped).
var _clock := 0.0


func _develop_all() -> void:
	for i in 20:
		_clock += 100.0
		sim.zones.step(_clock)


func test_zoned_cells_next_to_a_road_develop_after_a_delay() -> void:
	sim.zones.paint(_cells(Vector2i(1, 3), Vector2i(4, 3)), Lot.Type.COMMERCIAL)
	sim.zones.step(0.0)
	assert_eq(sim.lots.lots.size(), 0, "not instantly")
	sim.zones.step(ZoneSystem.DEVELOP_MAX_DELAY + 0.1)
	assert_eq(sim.lots.lots.size(), 4)
	for lot: Lot in sim.lots.lots.values():
		assert_eq(lot.type, Lot.Type.COMMERCIAL)


func test_cells_without_road_wait_until_a_road_arrives() -> void:
	sim.zones.paint(_cells(Vector2i(1, 8), Vector2i(1, 8)), Lot.Type.RESIDENTIAL)
	_develop_all()
	assert_eq(sim.lots.lots.size(), 0)
	assert_true(sim.zones.is_waiting_for_road(Vector2i(1, 8)))
	sim.roads.add_road(Vector2(0, 9) * G, Vector2(4, 9) * G, RoadType.load_by_id(&"local_2lane"))
	_develop_all()
	assert_eq(sim.lots.lots.size(), 1)
	assert_false(sim.zones.is_waiting_for_road(Vector2i(1, 8)))


func test_rezoning_replaces_and_dezoning_removes_the_building() -> void:
	var cell: Array[Vector2i] = [Vector2i(2, 3)]
	sim.zones.paint(cell, Lot.Type.RESIDENTIAL)
	_develop_all()
	var first_lot := sim.lots.get_lot_at_cell(cell[0])
	sim.zones.paint(cell, Lot.Type.INDUSTRIAL)
	assert_eq(sim.lots.get_lot_at_cell(cell[0]), -1, "old building goes at once")
	_develop_all()
	var second_lot := sim.lots.get_lot_at_cell(cell[0])
	assert_ne(second_lot, first_lot)
	assert_eq(sim.lots.lots[second_lot].type, Lot.Type.INDUSTRIAL)
	sim.zones.erase(cell)
	assert_eq(sim.lots.lots.size(), 0)
	assert_false(sim.zones.zones.has(cell[0]))


func test_hand_placed_lots_are_left_alone() -> void:
	var lot_id := sim.lots.add_lot(Vector2i(2, 3), Lot.Type.RESIDENTIAL)
	var cell: Array[Vector2i] = [Vector2i(2, 3)]
	sim.zones.paint(cell, Lot.Type.INDUSTRIAL)
	assert_true(sim.lots.lots.has(lot_id))
	assert_false(sim.zones.zones.has(cell[0]))


func test_developments_per_tick_are_capped() -> void:
	sim.zones.paint(_cells(Vector2i(0, 3), Vector2i(9, 3)), Lot.Type.RESIDENTIAL)
	sim.zones.step(1000.0)
	assert_eq(sim.lots.lots.size(), ZoneSystem.MAX_DEVELOPMENTS_PER_TICK)


func test_save_round_trip_keeps_zones_and_their_buildings() -> void:
	sim.zones.paint(_cells(Vector2i(1, 3), Vector2i(3, 3)), Lot.Type.RESIDENTIAL)
	_develop_all()
	sim.zones.paint(_cells(Vector2i(6, 3), Vector2i(6, 3)), Lot.Type.INDUSTRIAL)  # Not developed yet.
	var saved := sim.to_dict()
	var loaded := Simulation.from_dict(JSON.parse_string(JSON.stringify(saved, "", true, true)))
	assert_eq_deep(loaded.zones.zones, sim.zones.zones)
	assert_eq(loaded.lots.lots.size(), 3)
	assert_true(loaded.zones.is_developed(Vector2i(1, 3)))
	for i in 20:
		loaded.zones.step(100.0 * (i + 1))
	assert_eq(loaded.lots.lots.size(), 4, "the undeveloped cell develops after loading")
	loaded.dispose()
