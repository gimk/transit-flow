extends SceneTree
## Headless traffic benchmark. Run from the project folder:
##   <godot console exe> --headless --script res://benchmarks/traffic_benchmark.gd
## Builds a 20 x 20 street grid, fills it with N stress vehicles and reports the average
## and worst cost of one simulation tick, plus the cost of filling the renderer's
## MultiMesh. Budget at 60 FPS: one tick runs every 3 frames at 1x and ~1.3 times per
## frame at 4x, so a tick should stay well under ~8 ms for 4x to hold 60 FPS.

const COUNTS := [500, 1000, 2000, 5000]
const GRID_ROADS := 20
const GRID_SPACING := 3
const MAX_RAMP_TICKS := 1500
const MEASURE_TICKS := 200


func _initialize() -> void:
	var road_data := _build_grid_roads()
	print("Grid: %d x %d roads, %d cells apart" % [GRID_ROADS, GRID_ROADS, GRID_SPACING])
	print("| target | vehicles | avg tick ms | worst tick ms | render fill ms | stuck >30s | arrivals/s |")
	print("|---|---|---|---|---|---|---|")
	for target: int in COUNTS:
		_run(road_data, target)
	quit()


func _build_grid_roads() -> Dictionary:
	var roads := RoadNetwork.new()
	var type := RoadType.load_by_id(&"local_2lane")
	var g := float(Grid.CELL_SIZE)
	var extent := (GRID_ROADS - 1) * GRID_SPACING
	for i in GRID_ROADS:
		var offset := 2 + i * GRID_SPACING
		roads.add_road(Vector2(2, offset) * g, Vector2(2 + extent, offset) * g, type)
		roads.add_road(Vector2(offset, 2) * g, Vector2(offset, 2 + extent) * g, type)
	var data := roads.to_dict()
	roads.dispose()
	return data


func _run(road_data: Dictionary, target: int) -> void:
	var sim := Simulation.new(RoadNetwork.from_dict(road_data))
	sim.stress.target_count = target
	for tick in MAX_RAMP_TICKS:
		sim.step()
		if sim.vehicles.vehicles.size() >= target:
			break
	var arrivals := { "count": 0 }
	sim.vehicles.vehicle_arrived.connect(func(_v: Vehicle) -> void: arrivals.count += 1)
	var total_usec := 0
	var worst_usec := 0
	for tick in MEASURE_TICKS:
		var start := Time.get_ticks_usec()
		sim.step()
		var elapsed := Time.get_ticks_usec() - start
		total_usec += elapsed
		worst_usec = maxi(worst_usec, elapsed)
	print("| %d | %d | %.2f | %.2f | %.2f | %d | %.1f |" % [
		target,
		sim.vehicles.vehicles.size(),
		total_usec / 1000.0 / MEASURE_TICKS,
		worst_usec / 1000.0,
		_measure_render_fill(sim),
		sim.vehicles.get_stuck_count(30.0),
		arrivals.count / (MEASURE_TICKS * Simulation.TICK_DT),
	])
	sim.dispose()


## Same per-instance work as VehicleRenderer._process, on a detached MultiMesh.
## (VehicleRenderer itself needs the Game autoload, which --script runs don't have.)
func _measure_render_fill(sim: Simulation) -> float:
	var multimesh := MultiMesh.new()
	multimesh.transform_format = MultiMesh.TRANSFORM_2D
	multimesh.use_colors = true
	multimesh.instance_count = sim.vehicles.vehicles.size()
	var start := Time.get_ticks_usec()
	var i := 0
	for vehicle: Vehicle in sim.vehicles.vehicles.values():
		var position := vehicle.previous_position.lerp(vehicle.position, 0.5)
		var rotation := lerp_angle(vehicle.previous_rotation, vehicle.rotation, 0.5)
		multimesh.set_instance_transform_2d(i, Transform2D(rotation, position))
		multimesh.set_instance_color(i, Color.from_hsv(fmod(vehicle.owner_id * 0.618034, 1.0), 0.45, 1.0))
		i += 1
	return (Time.get_ticks_usec() - start) / 1000.0
