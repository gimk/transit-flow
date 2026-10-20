extends GutTest
## Phase 3c: patience, failed trips and the Flow score.

const G := 64.0
const DT := Simulation.TICK_DT

var sim: Simulation


func before_each() -> void:
	sim = Simulation.new()


func after_each() -> void:
	sim.dispose()


func _road(from: Vector2, to: Vector2) -> void:
	sim.roads.add_road(from * G, to * G, RoadType.load_by_id(&"local_2lane"))


func _spawn(from: Vector2, to: Vector2, owner := 1) -> Vehicle:
	var a := sim.roads.find_lane_position(from * G, 4.0)
	var b := sim.roads.find_lane_position(to * G, 4.0)
	return sim.vehicles.spawn(sim.router.find_route(a.lane_id, a.distance, b.lane_id, b.distance), -1, owner)


func _trip(start: float, ideal: float, end: float, owner := 1) -> Vehicle:
	var vehicle := Vehicle.new()
	vehicle.owner_id = owner
	vehicle.trip_start_time = start
	vehicle.ideal_trip_time = ideal
	vehicle.trip_end_time = end
	return vehicle


func test_score_is_ideal_over_actual_capped_at_one() -> void:
	assert_eq(FlowMetric.score(_trip(0.0, 10.0, 10.0), true), 1.0)
	assert_eq(FlowMetric.score(_trip(0.0, 10.0, 8.0), true), 1.0, "faster than ideal still caps at 1")
	assert_almost_eq(FlowMetric.score(_trip(0.0, 10.0, 40.0), true), 0.25, 0.0001)
	assert_eq(FlowMetric.score(_trip(0.0, 10.0, 12.0), false), 0.0, "failed trips score 0")


func test_flow_averages_trips_of_the_last_day() -> void:
	assert_eq(sim.flow.get_flow_percent(0.0), -1.0, "no data yet")
	sim.vehicles.vehicle_arrived.emit(_trip(0.0, 10.0, 10.0))
	sim.vehicles.vehicle_arrived.emit(_trip(0.0, 10.0, 20.0))
	sim.vehicles.vehicle_failed.emit(_trip(0.0, 10.0, 30.0))
	assert_almost_eq(sim.flow.get_flow_percent(30.0), 50.0, 0.001)
	assert_eq(sim.flow.get_trip_count(30.0), 3)
	# A day later, only newer trips count.
	sim.vehicles.vehicle_arrived.emit(_trip(400.0, 10.0, 410.0))
	assert_almost_eq(sim.flow.get_flow_percent(410.0), 100.0, 0.001)
	assert_eq(sim.flow.get_trip_count(410.0), 1)


func test_stress_traffic_is_not_counted() -> void:
	sim.vehicles.vehicle_failed.emit(_trip(0.0, 10.0, 30.0, StressTraffic.OWNER_ID))
	assert_eq(sim.flow.get_trip_count(30.0), 0)


func test_free_flow_trip_scores_near_full_marks() -> void:
	_road(Vector2(0, 4), Vector2(12, 4))
	var vehicle := _spawn(Vector2(1, 4) + Vector2(0, 7) / G, Vector2(11, 4) + Vector2(0, 7) / G)
	assert_gt(vehicle.ideal_trip_time, 0.0)
	while sim.vehicles.vehicles.has(vehicle.id):
		sim.vehicles.step(DT)
	assert_gt(FlowMetric.score(vehicle, true), 0.9, "an empty road is close to ideal")
	assert_false(vehicle.impatient)


func test_late_drivers_turn_impatient_then_give_up() -> void:
	_road(Vector2(0, 4), Vector2(12, 4))
	var vehicle := _spawn(Vector2(1, 4) + Vector2(0, 7) / G, Vector2(11, 4) + Vector2(0, 7) / G)
	var ideal := vehicle.ideal_trip_time
	# Pretend the trip has dragged on.
	vehicle.trip_start_time = -(ideal * VehicleSystem.IMPATIENT_FACTOR + VehicleSystem.IMPATIENT_GRACE + 1.0)
	sim.vehicles.step(DT)
	assert_true(vehicle.impatient)
	assert_true(sim.vehicles.vehicles.has(vehicle.id), "still driving")
	vehicle.trip_start_time = -(ideal * VehicleSystem.GIVE_UP_FACTOR + VehicleSystem.GIVE_UP_GRACE + 1.0)
	watch_signals(sim.vehicles)
	sim.vehicles.step(DT)
	assert_true(vehicle.gave_up)
	assert_false(sim.vehicles.vehicles.has(vehicle.id))
	assert_signal_emitted_with_parameters(sim.vehicles, "vehicle_failed", [vehicle])


## Flow reacts to the network: the bottleneck morning scores worse with its stop sign
## than with the junction left uncontrolled.
func test_flow_reacts_to_junction_control() -> void:
	var with_stop := _bottleneck_flow(RoadNode.JunctionControl.STOP_SIGN)
	var uncontrolled := _bottleneck_flow(RoadNode.JunctionControl.NONE)
	gut.p("bottleneck morning flow: %.0f%% with the stop sign, %.0f%% uncontrolled" % [with_stop, uncontrolled])
	assert_gt(uncontrolled, with_stop + 5.0)


func _bottleneck_flow(control: RoadNode.JunctionControl) -> float:
	sim.dispose()
	sim = Simulation.new()
	var info := Scenarios.build_bottleneck(sim, Vector2i(30, 30))
	sim.roads.set_node_control(sim.roads.find_node_at(info.junction), control)
	for tick in Simulation.TICK_RATE * 120:
		sim.step()
	return sim.flow.get_flow_percent(sim.get_sim_time())
