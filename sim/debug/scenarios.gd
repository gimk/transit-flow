class_name Scenarios
extends RefCounted
## Ready-made layouts for playtesting (F7 in game) and for tests.


## "Bottleneck": a suburb of 28 homes (56 commuters) in the west commutes along one main
## road to 24 factories in the east, through a T-junction with traffic lights that also
## serve a side street of commuters. Everyone leaves within ~30 s, so a queue builds at
## the lights.
## Returns positions (world px) tests need:
## - "junction": the light-controlled junction,
## - "bypass": [from, to] pairs for three roads that route around it (not built).
static func build_bottleneck(sim: Simulation, origin: Vector2i) -> Dictionary:
	var road_type := RoadType.load_by_id(&"local_2lane")
	var at := func(x: int, y: int) -> Vector2: return Vector2(origin + Vector2i(x, y)) * Grid.CELL_SIZE

	sim.roads.add_road(at.call(-15, 0), at.call(15, 0), road_type)
	sim.roads.add_road(at.call(0, 0), at.call(0, 7), road_type)
	sim.roads.set_node_control(sim.roads.find_node_at(at.call(0, 0)), RoadNode.JunctionControl.TRAFFIC_LIGHT)

	for x in range(-14, -2):
		sim.lots.add_lot(origin + Vector2i(x, -1), Lot.Type.RESIDENTIAL)
		sim.lots.add_lot(origin + Vector2i(x, 0), Lot.Type.RESIDENTIAL)
	for y in range(2, 6):
		sim.lots.add_lot(origin + Vector2i(-1, y), Lot.Type.RESIDENTIAL)
	for x in range(3, 15):
		sim.lots.add_lot(origin + Vector2i(x, -1), Lot.Type.INDUSTRIAL)
		sim.lots.add_lot(origin + Vector2i(x, 0), Lot.Type.INDUSTRIAL)
	# Everyone leaves within ~30 s (2.5 game hours), so the queue builds fast but drivers
	# setting off later can see the jam and choose another way.
	sim.citizens.depart_everyone_now(30.0)

	return {
		"junction": at.call(0, 0),
		"bypass": [
			[at.call(-3, 0), at.call(-3, -4)],
			[at.call(-3, -4), at.call(3, -4)],
			[at.call(3, -4), at.call(3, 0)],
		],
	}


## Where the Town scenario sits: its center cell (the Main Street / Avenue crossing),
## placed so its two highways reach the west and north map edges after a short drive.
const TOWN_CENTER := Vector2i(20, 18)


## "Town": a complete small city that lives through a whole day on its own, to test the
## simulation end to end. Cells below are relative to TOWN_CENTER; the town spans
## x -12..12, y -8..8, small enough that commutes take well under a game hour.
## - Roads: a ring road around the town; Main Street (east-west, y 0) runs in from the
##   west highway; the Avenue (north-south, x 0); two cross streets (y -4, y 4) and two
##   side streets (x -6, x 6), the latter fed by the north highway; a cul-de-sac in
##   the south-west.
## - Traffic lights on the Avenue's and Main Street's busiest crossings; every other
##   junction is uncontrolled.
## - Zones, developed at once: two residential districts in the west (Oak Hill north of
##   Main Street, Riverside south of it) and Southgate in the south-east; shops downtown
##   along Main Street and the Avenue; an industrial park in the north-east.
## Commuters from the west cross the Avenue to the factories, Riverside's cross Main
## Street, trucks come in from both highways: every kind of conflict happens somewhere.
## Returns positions (world px):
## - "center": the downtown crossing,
## - "lights": the light-controlled junctions,
## - "highways": the two external connection points.
static func build_town(sim: Simulation) -> Dictionary:
	var road_type := RoadType.load_by_id(&"local_2lane")
	var at := func(x: int, y: int) -> Vector2: return Vector2(TOWN_CENTER + Vector2i(x, y)) * Grid.CELL_SIZE
	var roads: Array[Array] = [
		[-20, 0, 12, 0],    # West highway + Main Street.
		[6, -18, 6, 4],     # North highway, down through the industrial park.
		[0, -8, 0, 8],      # The Avenue.
		[-12, -8, 12, -8],  # Ring road.
		[-12, 8, 12, 8],
		[-12, -8, -12, 8],
		[12, -8, 12, 8],
		[-12, -4, 12, -4],  # North cross street.
		[-12, 4, 12, 4],    # South cross street.
		[-6, -8, -6, 8],    # West side street.
		[-3, 4, -3, 7],     # Cul-de-sac.
	]
	for road in roads:
		sim.roads.add_road(at.call(road[0], road[1]), at.call(road[2], road[3]), road_type)
	var lights: Array[Vector2] = [at.call(0, -4), at.call(0, 0), at.call(0, 4), at.call(-6, 0), at.call(6, 0)]
	for point in lights:
		sim.roads.set_node_control(sim.roads.find_node_at(point), RoadNode.JunctionControl.TRAFFIC_LIGHT)
	var highways: Array[Vector2] = [at.call(-20, 0), at.call(6, -18)]
	for point in highways:
		sim.externals.add(point)

	var zone := func(x0: int, y0: int, x1: int, y1: int, type: Lot.Type) -> void:
		var cells: Array[Vector2i] = []
		for x in range(x0, x1 + 1):
			for y in range(y0, y1 + 1):
				cells.append(TOWN_CENTER + Vector2i(x, y))
		sim.zones.paint(cells, type)
	zone.call(-11, -8, -1, -8, Lot.Type.RESIDENTIAL)  # Oak Hill,
	zone.call(-11, -5, -1, -4, Lot.Type.RESIDENTIAL)
	zone.call(-11, 3, -1, 4, Lot.Type.RESIDENTIAL)    # Riverside,
	zone.call(-11, 7, -4, 7, Lot.Type.RESIDENTIAL)
	zone.call(-4, 5, -3, 6, Lot.Type.RESIDENTIAL)     # and its cul-de-sac.
	zone.call(7, 3, 11, 4, Lot.Type.RESIDENTIAL)      # Southgate.
	zone.call(1, 7, 11, 7, Lot.Type.RESIDENTIAL)
	zone.call(-5, -1, 11, 0, Lot.Type.COMMERCIAL)     # Downtown, along Main Street,
	zone.call(-1, 1, 0, 2, Lot.Type.COMMERCIAL)       # and down the Avenue.
	zone.call(1, -5, 11, -4, Lot.Type.INDUSTRIAL)     # Industrial park.
	zone.call(1, -8, 11, -8, Lot.Type.INDUSTRIAL)
	zone.call(5, -3, 6, -2, Lot.Type.INDUSTRIAL)
	sim.zones.develop_all_now()

	return { "center": at.call(0, 0), "lights": lights, "highways": highways }


## The starting map of a new game: a highway entering from the middle of the west edge,
## a few cells long, registered as the city's connection to the outside world.
static func add_highway(sim: Simulation) -> void:
	var map := Grid.map_rect()
	var entry := Grid.snap_to_point(Vector2(map.position.x, map.get_center().y))
	sim.roads.add_road(entry, entry + Vector2(6, 0) * Grid.CELL_SIZE, RoadType.load_by_id(&"local_2lane"))
	sim.externals.add(entry)


## Builds the bypass roads returned by build_bottleneck.
static func build_bypass(sim: Simulation, info: Dictionary) -> void:
	var road_type := RoadType.load_by_id(&"local_2lane")
	for road: Array in info.bypass:
		sim.roads.add_road(road[0], road[1], road_type)
