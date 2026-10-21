class_name Scenarios
extends RefCounted
## Ready-made layouts for playtesting (F7 in game) and for tests.


## "Bottleneck": a suburb of 28 homes (56 commuters) in the west commutes along one main
## road to 24 factories in the east, through a T-junction with traffic lights that also
## serve a side street of commuters. Everyone leaves within ~15 s, so a long queue builds
## at the lights.
## Returns positions (world px) tests need:
## - "junction": the light-controlled junction,
## - "bypass": [from, to] pairs for three roads that skip the lights and reach the
##   factories from the far end (not built).
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
	# Everyone leaves within ~15 s (1.25 game hours), so the queue builds fast but drivers
	# setting off later can see the jam and choose another way.
	sim.citizens.depart_everyone_now(15.0)

	return {
		"junction": at.call(0, 0),
		"bypass": [
			[at.call(-3, 0), at.call(-3, -4)],
			[at.call(-3, -4), at.call(15, -4)],
			[at.call(15, -4), at.call(15, 0)],
		],
	}


## A bus line along the Bottleneck's main road: east past the suburb and the factories
## on the south side, back west on the north side, turning round at the road's ends.
static func add_bottleneck_bus_line(sim: Simulation, origin: Vector2i) -> int:
	var stops: Array[int] = []
	for x in [-12, -8, -4, 5, 9, 13]:
		stops.append(sim.transit.add_stop(Vector2(origin + Vector2i(x, 0)) * Grid.CELL_SIZE + Vector2(0, 10)))
	for x in [13, 9, 5, -4, -8, -12]:
		stops.append(sim.transit.add_stop(Vector2(origin + Vector2i(x, 0)) * Grid.CELL_SIZE + Vector2(0, -10)))
	return sim.transit.add_line(stops)


## The Bottleneck with transit priority: its main road made an avenue with bus lanes
## (still one lane each way for cars) and a bus line along it.
static func add_bottleneck_bus_lanes(sim: Simulation, origin: Vector2i) -> int:
	var bus_avenue := RoadType.load_by_id(&"avenue_4lane_bus")
	var y := float(origin.y * Grid.CELL_SIZE)
	for segment: RoadSegment in sim.roads.segments.values().duplicate():
		var a := sim.roads.nodes[segment.start_node_id].position
		var b := sim.roads.nodes[segment.end_node_id].position
		if absf(a.y - y) < 1.0 and absf(b.y - y) < 1.0:
			sim.roads.set_segment_type(segment.id, bus_avenue)
	return add_bottleneck_bus_line(sim, origin)


## Where the Town scenario sits: its center cell (the Main Street / Avenue crossing),
## placed so its two highways reach the west and north map edges after a short drive.
const TOWN_CENTER := Vector2i(20, 18)


## "Town": a complete small city that lives through a whole day on its own, to test the
## simulation end to end, using every feature built so far (keep it that way: update it
## at the end of each phase). Cells below are relative to TOWN_CENTER; the town spans
## x -12..12, y -8..8.
## - Roads: a ring road around the town; Main Street (east-west, y 0), an avenue, runs in
##   from the west highway; the Avenue (north-south, x 0) has bus lanes; the two cross
##   streets (y -4, y 4) are a one-way pair (eastbound north, westbound south); two side
##   streets (x -6, x 6), the latter fed by the north highway; a cul-de-sac in the
##   south-west.
## - Traffic lights on the Avenue's and Main Street's busiest crossings, the Avenue's with
##   the south cross street with protected left turns; every other junction is
##   uncontrolled (the avenues and the two-lane one-ways have priority there).
## - Zones, developed at once: two residential districts in the west (Oak Hill north of
##   Main Street, Riverside south of it) and Southgate in the south-east; shops downtown
##   along Main Street and the Avenue; an industrial park in the north-east. Buildings
##   stand back from the avenues, with their parking.
## - Three bus lines: Main Street there and back with bus bays, the Avenue there and back
##   in its bus lanes, and the Crosstown loop round the one-way pair with curbside stops.
## Commuters from the west cross the Avenue to the factories, Riverside's cross Main
## Street, trucks come in from both highways, a fifth of the people have no car and walk
## or ride: every kind of conflict happens somewhere.
## Returns (positions in world px):
## - "center": the downtown crossing,
## - "lights": the light-controlled junctions,
## - "highways": the two external connection points,
## - "bus_lines": the bus line ids.
static func build_town(sim: Simulation) -> Dictionary:
	var at := func(x: int, y: int) -> Vector2: return Vector2(TOWN_CENTER + Vector2i(x, y)) * Grid.CELL_SIZE
	# [from x, from y, to x, to y, road type]; one-way roads run from -> to.
	var roads: Array[Array] = [
		[-20, 0, -12, 0, &"local_2lane"],         # West highway,
		[-12, 0, 12, 0, &"avenue_4lane"],         # into Main Street, an avenue.
		[6, -18, 6, 4, &"local_2lane"],           # North highway, down through the industrial park.
		[0, -8, 0, 8, &"avenue_4lane_bus"],       # The Avenue, with bus lanes.
		[-12, -8, 12, -8, &"local_2lane"],        # Ring road.
		[-12, 8, 12, 8, &"local_2lane"],
		[-12, -8, -12, 8, &"local_2lane"],
		[12, -8, 12, 8, &"local_2lane"],
		[-12, -4, 12, -4, &"one_way_2lane"],      # Cross streets: a one-way pair, eastbound
		[12, 4, -12, 4, &"one_way_2lane"],        # in the north, westbound in the south.
		[-6, -8, -6, 8, &"local_2lane"],          # West side street.
		[-3, 4, -3, 7, &"local_2lane"],           # Cul-de-sac.
	]
	for road in roads:
		sim.roads.add_road(at.call(road[0], road[1]), at.call(road[2], road[3]), RoadType.load_by_id(road[4]))
	var lights: Array[Vector2] = [at.call(0, -4), at.call(0, 0), at.call(0, 4), at.call(-6, 0), at.call(6, 0)]
	for point in lights:
		sim.roads.set_node_control(sim.roads.find_node_at(point), RoadNode.JunctionControl.TRAFFIC_LIGHT)
	# The Avenue's crossing with the south cross street gives left turns a phase of their
	# own (tried elsewhere: at busier crossings the four-phase cycle jams the morning).
	sim.roads.set_node_control(sim.roads.find_node_at(at.call(0, 4)), RoadNode.JunctionControl.TRAFFIC_LIGHT_PROTECTED_LEFT)
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

	# Three bus lines: along Main Street and back, stopping in bus bays (so the avenue's
	# traffic drives past); up and down the Avenue in its bus lanes; and the Crosstown
	# loop round the one-way pair (east along the north cross street, west along the
	# south one), with curbside stops. Riders change lines at stops a short walk apart.
	var bus_lines: Array[int] = []
	var main_street: Array[int] = []
	for x in [-9, -2, 2, 9]:
		main_street.append(sim.transit.add_stop(at.call(x, 0) + Vector2(0, 10), true))
	for x in [9, 2, -2, -9]:
		main_street.append(sim.transit.add_stop(at.call(x, 0) + Vector2(0, -10), true))
	bus_lines.append(sim.transit.add_line(main_street))
	var avenue: Array[int] = []
	for y in [-6, -2, 2, 6]:
		avenue.append(sim.transit.add_stop(at.call(0, y) + Vector2(-10, 0)))
	for y in [6, 2, -2, -6]:
		avenue.append(sim.transit.add_stop(at.call(0, y) + Vector2(10, 0)))
	bus_lines.append(sim.transit.add_line(avenue))
	var crosstown: Array[int] = []
	for x in [-9, -3, 3, 9]:
		crosstown.append(sim.transit.add_stop(at.call(x, -4) + Vector2(0, 10)))
	for x in [9, 3, -3, -9]:
		crosstown.append(sim.transit.add_stop(at.call(x, 4) + Vector2(0, -10)))
	bus_lines.append(sim.transit.add_line(crosstown))

	return { "center": at.call(0, 0), "lights": lights, "highways": highways, "bus_lines": bus_lines }


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
