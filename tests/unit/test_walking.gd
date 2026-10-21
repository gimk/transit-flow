extends GutTest
## Phase 7: sidewalks, crosswalks, pedestrians, and how they meet cars.

const G := 64.0
const DT := Simulation.TICK_DT

var sim: Simulation
var roads: RoadNetwork


func before_each() -> void:
	sim = Simulation.new()
	roads = sim.roads


func after_each() -> void:
	sim.dispose()


func _road(from: Vector2, to: Vector2) -> void:
	roads.add_road(from * G, to * G, RoadType.load_by_id(&"local_2lane"))


## A four-way junction at (6, 4).
func _crossroads() -> int:
	_road(Vector2(0, 4), Vector2(12, 4))
	_road(Vector2(6, 0), Vector2(6, 8))
	return roads.find_node_at(Vector2(6, 4) * G)


func _lot_center(cell: Vector2i) -> Vector2:
	return Grid.cell_center(cell)


## Crosswalk across the road arm of `junction` that lies in `direction` from it.
func _crosswalk(junction: int, direction: Vector2) -> Vector2i:
	for segment_id in roads.nodes[junction].segment_ids:
		var segment := roads.segments[segment_id]
		var other := roads.nodes[segment.get_other_node_id(junction)].position
		if (other - roads.nodes[junction].position).normalized().dot(direction) > 0.9:
			return Vector2i(junction, segment_id)
	return Crosswalks.NONE


func test_sidewalks_and_crosswalks_at_a_junction() -> void:
	var junction := _crossroads()
	var network := sim.pedestrians.network
	network.find_path(Vector2.ZERO, Vector2.ONE)  # Builds it.
	assert_eq(network.sidewalks.size(), 4 * 2, "two sidewalks per road")
	var crossed: Array = network.crosswalk_edges.values()
	assert_eq(crossed.size(), 4, "a crosswalk across each road at the junction")
	for crosswalk: Vector2i in crossed:
		assert_eq(crosswalk.x, junction)
	# Diagonally across the junction: two roads to cross.
	var path: Dictionary = network.find_path(_lot_center(Vector2i(4, 3)), _lot_center(Vector2i(8, 4)))
	var crossings: Array = path.crossings.filter(func(c: Vector2i) -> bool: return c != Crosswalks.NONE)
	assert_eq(crossings.size(), 2)
	assert_lt(path.length, _lot_center(Vector2i(4, 3)).distance_to(_lot_center(Vector2i(8, 4))) * 1.6, "no big detour")
	# Along the same side of a street: nothing to cross.
	var along: Dictionary = network.find_path(_lot_center(Vector2i(1, 3)), _lot_center(Vector2i(4, 3)))
	assert_true(along.crossings.all(func(c: Vector2i) -> bool: return c == Crosswalks.NONE))


func test_cars_give_way_to_people_crossing() -> void:
	var junction := _crossroads()
	var west_arm := _crosswalk(junction, Vector2.LEFT)
	# Walking across the western arm (north side to south side, west of the junction)...
	var home := sim.lots.add_lot(Vector2i(5, 3), Lot.Type.RESIDENTIAL)
	var shop := sim.lots.add_lot(Vector2i(5, 4), Lot.Type.COMMERCIAL)
	var walker := sim.pedestrians.walk(home, shop, 1)
	assert_true(walker.crossings.has(west_arm), "test setup: crosses the western arm")
	# ...while a car comes from the west to go straight on: its turn crosses that crosswalk.
	var from := roads.find_lane_position((Vector2(3.5, 4) + Vector2(0, 7) / G) * G, 4.0)
	var to := roads.find_lane_position((Vector2(11, 4) + Vector2(0, 7) / G) * G, 4.0)
	var car := sim.vehicles.spawn(sim.router.find_route(from.lane_id, from.distance, to.lane_id, to.distance), -1, 2)
	var car_waited := false
	var both := 0
	for i in 600:
		sim.step()
		if sim.vehicles.vehicles.has(car.id) and car.requested_connector != null and sim.crosswalks.has_walkers(west_arm):
			car_waited = true
		if sim.vehicles.vehicles.has(car.id) and car.lane.is_connector() and sim.crosswalks.has_walkers(west_arm):
			both += 1
	assert_eq(both, 0, "never on the crossing together")
	assert_true(car_waited, "the car waited for the pedestrian")
	assert_false(sim.pedestrians.pedestrians.has(walker.id), "walked all the way")
	assert_false(sim.vehicles.vehicles.has(car.id), "drove on after")


func test_at_lights_people_cross_on_the_walk_light() -> void:
	var junction := _crossroads()
	roads.set_node_control(junction, RoadNode.JunctionControl.TRAFFIC_LIGHT)
	var node := roads.nodes[junction]
	var home := sim.lots.add_lot(Vector2i(5, 3), Lot.Type.RESIDENTIAL)
	var shop := sim.lots.add_lot(Vector2i(5, 4), Lot.Type.COMMERCIAL)
	var west_arm := _crosswalk(junction, Vector2.LEFT)
	var walker := sim.pedestrians.walk(home, shop, 1)
	var crossed_on_red := false
	var waited := false
	for i in 1200:
		sim.step()
		if sim.crosswalks.has_walkers(west_arm) and not sim.signals.is_walk_green(node, west_arm.y):
			# Allowed only if they stepped out while it was still green.
			crossed_on_red = crossed_on_red or walker.leg_progress < 1.0
		waited = waited or sim.crosswalks.has_waiting(west_arm)
	assert_false(crossed_on_red, "never stepped out on red")
	assert_false(sim.pedestrians.pedestrians.has(walker.id), "the walk light came (pedestrians call it)")
	# Crossing the western arm walks alongside north-south traffic.
	var phase := sim.signals.get_walk_phase(node, west_arm.y)
	var north_lane: Lane = null
	for lane: Lane in roads.lanes.values():
		if not lane.is_connector() and lane.to_node_id == junction and absf(LaneBuilder.end_direction(lane.curve).y) > 0.9:
			north_lane = lane
	assert_eq(phase, north_lane.signal_phase)


## Random traffic and random walks around a grid: nobody is ever on a crosswalk while a
## car is on a turn over it.
func test_people_and_cars_never_share_a_crosswalk() -> void:
	for i in 4:
		_road(Vector2(1, 1 + i * 3), Vector2(10, 1 + i * 3))
		_road(Vector2(1 + i * 3, 1), Vector2(1 + i * 3, 10))
	var lots: Array[int] = []
	for x in range(1, 10):
		for y in [0, 3, 6]:
			if x % 3 != 1:
				lots.append(sim.lots.add_lot(Vector2i(x, y), Lot.Type.RESIDENTIAL))
	var rng := RandomNumberGenerator.new()
	rng.seed = 5
	sim.stress.target_count = 120
	var violations := 0
	var walked := { "count": 0 }
	sim.pedestrians.pedestrian_arrived.connect(func(_p: Pedestrian) -> void: walked.count += 1)
	for tick in Simulation.TICK_RATE * 60:
		if tick % 5 == 0:
			sim.pedestrians.walk(lots[rng.randi() % lots.size()], lots[rng.randi() % lots.size()], 1)
		sim.step()
		for vehicle: Vehicle in sim.vehicles.vehicles.values():
			if not vehicle.lane.is_connector():
				continue
			var feeder := roads.lanes[vehicle.lane.feeder_lane_id]
			for segment_id in [feeder.segment_id, vehicle.lane.outgoing_lanes[0].segment_id]:
				if sim.crosswalks.has_walkers(Vector2i(vehicle.lane.from_node_id, segment_id)):
					violations += 1
	gut.p("walks finished: %d, pedestrians now %d, vehicles %d" % [walked.count, sim.pedestrians.pedestrians.size(), sim.vehicles.vehicles.size()])
	assert_eq(violations, 0)
	assert_gt(walked.count, 50, "people get where they're going")
	assert_eq(sim.vehicles.get_stuck_count(30.0), 0, "and cars aren't stopped for good")


func test_citizens_walk_short_trips() -> void:
	_road(Vector2(0, 4), Vector2(12, 4))
	sim.lots.add_lot(Vector2i(2, 3), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(4, 3), Lot.Type.COMMERCIAL)  # 2 cells away: walkable.
	sim.step()
	sim.citizens.depart_everyone_now()
	for i in 5:
		sim.step()
	assert_eq(sim.pedestrians.pedestrians.size(), 2, "both residents walk to work")
	assert_eq(sim.vehicles.vehicles.size() + sim.vehicles.get_waiting_departures().size(), 0, "nobody drives")
	for i in 400:
		sim.step()
	var at_work: Array = sim.citizens.citizens.values().filter(func(c: Citizen) -> bool: return c.state == Citizen.State.AT_WORK)
	assert_eq(at_work.size(), 2)
	assert_gt(sim.flow.get_trip_count(sim.get_sim_time()), 0, "walks count in Flow")
