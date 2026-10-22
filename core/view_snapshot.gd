class_name ViewSnapshot
extends RefCounted
## What the screen needs from one simulation tick, copied out of the simulation right
## after the tick. The simulation runs on a worker thread (see Game), so per-frame views
## (vehicles, lights, HUD, day/night, heatmap) draw from snapshots instead of reading the
## simulation while it is being stepped. Captured on the worker; read-only afterwards.

## Sim time of the tick (sim seconds).
var time := 0.0
## Vehicle id -> index into the arrays below.
var vehicle_index: Dictionary[int, int] = {}
var positions := PackedVector2Array()
var rotations := PackedFloat32Array()
## Length x width per vehicle.
var sizes := PackedVector2Array()
var colors := PackedColorArray()
## Vehicle.Kind per vehicle, its speed (px/s; the renderer tells braking from the change
## between snapshots) and its indicator (VehicleRenderer.LAMP_LEFT / LAMP_RIGHT, or 0).
var kinds := PackedByteArray()
var speeds := PackedFloat32Array()
var indicators := PackedByteArray()
## How late each vehicle is, from 0 (within its ideal trip time) to 1 (late enough for its
## driver to turn impatient, VehicleSystem.IMPATIENT_FACTOR), for the lateness view.
var lateness := PackedFloat32Array()
## Lot id -> citizens' cars parked there (not driving, not waiting in the driveway).
var parked: Dictionary[int, int] = {}
## Pedestrian id -> index into pedestrian_positions.
var pedestrian_index: Dictionary[int, int] = {}
var pedestrian_positions := PackedVector2Array()
## Light-controlled junction node id -> light (TrafficLights.Light) per phase.
var lights: Dictionary[int, PackedInt32Array] = {}
## Bus stops: where, the direction of the traffic they serve, riders waiting, bay or not.
var stop_positions := PackedVector2Array()
var stop_directions := PackedVector2Array()
var stop_waiting := PackedInt32Array()
var stop_bays := PackedByteArray()
## Bus lines: { "color": Color, "path": PackedVector2Array } each (paths are replaced,
## never edited, so sharing them is safe).
var bus_lines: Array[Dictionary] = []
## Crosswalks at bus stops (Crosswalks.mid_block: replaced, never edited, so sharing it is
## safe).
var stop_crossings: Dictionary[Vector2i, Dictionary] = {}
## Lane id -> congestion (CongestionMonitor replaces its dictionary rather than editing
## it, so holding on to it is safe).
var congestion: Dictionary[int, float] = {}

var day := 1
var time_text := ""
var period_name := ""
var hours := 0.0
var vehicle_count := 0
## Vehicles waiting in driveways to pull out.
var waiting_count := 0
var stuck_count := 0
var population := 0
var employed := 0
var jobs := 0
var flow_percent := -1.0
## Citizen.Mode -> share of citizens' trips over the last day (FlowMetric.get_mode_shares).
var mode_shares := {}

const STUCK_SECONDS := 30.0


static func capture(sim: Simulation) -> ViewSnapshot:
	var snapshot := ViewSnapshot.new()
	snapshot.time = sim.get_sim_time()
	var vehicles := sim.vehicles.vehicles
	# Vehicles waiting in driveways are drawn too, parked on their lot.
	var waiting := sim.vehicles.get_waiting_departures()
	var count := vehicles.size() + waiting.size()
	snapshot.positions.resize(count)
	snapshot.rotations.resize(count)
	snapshot.sizes.resize(count)
	snapshot.colors.resize(count)
	snapshot.lateness.resize(count)
	snapshot.kinds.resize(count)
	snapshot.speeds.resize(count)
	snapshot.indicators.resize(count)
	var i := 0
	var stuck := 0
	var drawn: Array = vehicles.values()
	drawn.append_array(waiting)
	for vehicle: Vehicle in drawn:
		snapshot.vehicle_index[vehicle.id] = i
		snapshot.positions[i] = vehicle.position
		snapshot.rotations[i] = vehicle.rotation
		snapshot.sizes[i] = Vector2(vehicle.length, vehicle.width)
		snapshot.colors[i] = sim.transit.get_bus_color(vehicle.id) if vehicle.kind == Vehicle.Kind.BUS \
				else VehicleRenderer.color_for_vehicle(vehicle)
		snapshot.kinds[i] = vehicle.kind
		snapshot.speeds[i] = vehicle.speed
		snapshot.indicators[i] = VehicleRenderer.indicator_for(vehicle)
		var late_after := vehicle.ideal_trip_time * (VehicleSystem.IMPATIENT_FACTOR - 1.0) + VehicleSystem.IMPATIENT_GRACE
		var late_by := snapshot.time - vehicle.trip_start_time - vehicle.ideal_trip_time
		snapshot.lateness[i] = clampf(late_by / late_after, 0.0, 1.0)
		if vehicle.stopped_time >= STUCK_SECONDS:
			stuck += 1
		i += 1
	for node: RoadNode in sim.roads.nodes.values():
		if node.has_lights():
			var phases := PackedInt32Array()
			for phase in node.signal_phase_count:
				phases.append(sim.signals.get_light(node, phase))
			snapshot.lights[node.id] = phases
	var walkers := sim.pedestrians.pedestrians
	snapshot.pedestrian_positions.resize(walkers.size())
	var p := 0
	for pedestrian: Pedestrian in walkers.values():
		snapshot.pedestrian_index[pedestrian.id] = p
		snapshot.pedestrian_positions[p] = pedestrian.position
		p += 1
	for stop: BusStop in sim.transit.stops.values():
		if stop.is_active():
			snapshot.stop_positions.append(stop.position)
			snapshot.stop_directions.append(stop.direction)
			snapshot.stop_waiting.append(stop.waiting.size())
			snapshot.stop_bays.append(1 if stop.bay else 0)
	snapshot.stop_crossings = sim.crosswalks.mid_block
	for line: BusLine in sim.transit.lines.values():
		snapshot.bus_lines.append({ "color": line.color, "path": line.path })
	snapshot.congestion = sim.congestion.congestion
	snapshot.day = sim.clock.get_day()
	snapshot.time_text = sim.clock.format_time()
	snapshot.period_name = sim.clock.get_period_name()
	snapshot.hours = sim.clock.get_time_of_day_hours()
	snapshot.vehicle_count = vehicles.size()
	snapshot.waiting_count = waiting.size()
	snapshot.stuck_count = stuck
	snapshot.population = sim.citizens.citizens.size()
	for citizen: Citizen in sim.citizens.citizens.values():
		if citizen.has_car and citizen.vehicle_id == -1:
			snapshot.parked[citizen.car_lot_id] = snapshot.parked.get(citizen.car_lot_id, 0) + 1
	snapshot.employed = sim.citizens.get_employed_count()
	snapshot.jobs = sim.citizens.get_job_count()
	snapshot.flow_percent = sim.flow.get_flow_percent(snapshot.time)
	snapshot.mode_shares = sim.flow.get_mode_shares(snapshot.time)
	return snapshot


## The light a phase of a junction showed (green if unknown, e.g. lights just added).
func get_light(node_id: int, phase: int) -> TrafficLights.Light:
	var phases: PackedInt32Array = lights.get(node_id, PackedInt32Array())
	return phases[phase] as TrafficLights.Light if phase < phases.size() else TrafficLights.Light.GREEN
