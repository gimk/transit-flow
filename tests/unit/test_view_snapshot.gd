extends GutTest
## What the views get from each tick: lateness for the lateness colours, parked cars.

const G := 64.0

var sim: Simulation


func before_each() -> void:
	sim = Simulation.new()
	sim.roads.add_road(Vector2(0, 4) * G, Vector2(12, 4) * G, RoadType.load_by_id(&"local_2lane"))


func after_each() -> void:
	sim.dispose()


func test_cars_are_parked_at_home_until_driven() -> void:
	var home := sim.lots.add_lot(Vector2i(2, 3), Lot.Type.RESIDENTIAL)
	sim.lots.add_lot(Vector2i(10, 3), Lot.Type.INDUSTRIAL)
	sim.step()
	var owners := sim.citizens.citizens.values().filter(func(c: Citizen) -> bool: return c.has_car).size()
	assert_eq(ViewSnapshot.capture(sim).parked.get(home, 0), owners)
	for citizen: Citizen in sim.citizens.citizens.values():
		citizen.has_car = true
		citizen.car_preference = CitizenSystem.CAR_PREFERENCE_RANGE.x
	sim.citizens.depart_everyone_now()
	sim.step()
	assert_eq(ViewSnapshot.capture(sim).parked.get(home, 0), 0, "both cars out")


func test_lateness_grows_from_on_time_to_impatient() -> void:
	var from := sim.roads.find_lane_position(Vector2(1, 4) * G + Vector2(0, 7), 4.0)
	var to := sim.roads.find_lane_position(Vector2(11, 4) * G + Vector2(0, 7), 4.0)
	var car := sim.vehicles.spawn(sim.router.find_route(from.lane_id, from.distance, to.lane_id, to.distance), -1, 1)
	sim.step()
	var snapshot := ViewSnapshot.capture(sim)
	assert_eq(snapshot.lateness[snapshot.vehicle_index[car.id]], 0.0, "on time")
	car.trip_start_time -= car.ideal_trip_time * VehicleSystem.IMPATIENT_FACTOR + VehicleSystem.IMPATIENT_GRACE
	snapshot = ViewSnapshot.capture(sim)
	assert_eq(snapshot.lateness[snapshot.vehicle_index[car.id]], 1.0, "as late as an impatient driver")
	var color := snapshot.colors[snapshot.vehicle_index[car.id]]
	assert_eq(color, VehicleRenderer.color_for_vehicle(car), "normal colours stay")
	assert_almost_eq(color.h, VehicleRenderer.CAR_COLOR.h, 0.05, "a shade of green")


func test_drivers_signal_before_turning() -> void:
	sim.roads.add_road(Vector2(6, 0) * G, Vector2(6, 8) * G, RoadType.load_by_id(&"local_2lane"))
	var start := sim.roads.find_lane_position(Vector2(6 * G - 40.0, 4 * G + 7.0), 4.0)
	# Eastbound: north is a left turn, south a right turn.
	var targets := {
		VehicleRenderer.LAMP_LEFT: Vector2(6 * G + 7.0, 1 * G),
		VehicleRenderer.LAMP_RIGHT: Vector2(6 * G - 7.0, 7 * G),
		0: Vector2(11 * G, 4 * G + 7.0),
	}
	for lamp: int in targets:
		var to := sim.roads.find_lane_position(targets[lamp], 4.0)
		var car := sim.vehicles.spawn(sim.router.find_route(start.lane_id, start.distance, to.lane_id, to.distance), -1, 1)
		var snapshot := ViewSnapshot.capture(sim)
		assert_eq(snapshot.indicators[snapshot.vehicle_index[car.id]], lamp, "indicator for lamp %d" % lamp)
		sim.vehicles.remove(car.id)
