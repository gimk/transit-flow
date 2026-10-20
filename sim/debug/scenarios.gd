class_name Scenarios
extends RefCounted
## Ready-made layouts for playtesting (F7 in game) and for tests.


## "Bottleneck": a suburb of 28 homes in the west commutes along one main road to 24
## workplaces in the east, through a stop-sign T-junction that also takes a side street
## of commuters. Everyone leaves at once, so a queue builds at the stop sign.
## Returns positions (world px) tests need:
## - "junction": the stop-sign junction,
## - "bypass": [from, to] pairs for three roads that route around it (not built).
static func build_bottleneck(sim: Simulation, origin: Vector2i) -> Dictionary:
	var road_type := RoadType.load_by_id(&"local_2lane")
	var at := func(x: int, y: int) -> Vector2: return Vector2(origin + Vector2i(x, y)) * Grid.CELL_SIZE

	sim.roads.add_road(at.call(-15, 0), at.call(15, 0), road_type)
	sim.roads.add_road(at.call(0, 0), at.call(0, 7), road_type)
	sim.roads.set_node_control(sim.roads.find_node_at(at.call(0, 0)), RoadNode.JunctionControl.STOP_SIGN)

	for x in range(-14, -2):
		sim.lots.add_lot(origin + Vector2i(x, -1), Lot.Type.RESIDENTIAL)
		sim.lots.add_lot(origin + Vector2i(x, 0), Lot.Type.RESIDENTIAL)
	for y in range(2, 6):
		sim.lots.add_lot(origin + Vector2i(-1, y), Lot.Type.RESIDENTIAL)
	for x in range(3, 15):
		sim.lots.add_lot(origin + Vector2i(x, -1), Lot.Type.INDUSTRIAL)
		sim.lots.add_lot(origin + Vector2i(x, 0), Lot.Type.INDUSTRIAL)

	return {
		"junction": at.call(0, 0),
		"bypass": [
			[at.call(-3, 0), at.call(-3, -4)],
			[at.call(-3, -4), at.call(3, -4)],
			[at.call(3, -4), at.call(3, 0)],
		],
	}


## Builds the bypass roads returned by build_bottleneck.
static func build_bypass(sim: Simulation, info: Dictionary) -> void:
	var road_type := RoadType.load_by_id(&"local_2lane")
	for road: Array in info.bypass:
		sim.roads.add_road(road[0], road[1], road_type)
