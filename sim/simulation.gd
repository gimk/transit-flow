class_name Simulation
extends RefCounted
## Root of the simulation state. Pure data + logic: no Node dependencies,
## so it can be stepped headless from unit tests.

const TICK_RATE := 20
const TICK_DT := 1.0 / TICK_RATE
const SAVE_VERSION := 1

var clock := SimClock.new()
var roads: RoadNetwork
var router: Router
var lots: LotSystem
var vehicles: VehicleSystem
var citizens: CitizenSystem
var tick_count := 0


func _init(p_roads: RoadNetwork = null) -> void:
	roads = p_roads if p_roads != null else RoadNetwork.new()
	router = Router.new(roads)
	lots = LotSystem.new(roads)
	vehicles = VehicleSystem.new(roads, router, lots)
	citizens = CitizenSystem.new(lots, router, vehicles)


func step() -> void:
	tick_count += 1
	clock.advance(TICK_DT)
	citizens.step(get_sim_time())
	vehicles.step(TICK_DT)


## Simulated seconds since the start of the game.
func get_sim_time() -> float:
	return tick_count * TICK_DT


## Citizens and vehicles are not saved yet: on load everyone starts at home.
func to_dict() -> Dictionary:
	return {
		"version": SAVE_VERSION,
		"tick_count": tick_count,
		"clock_minutes": clock.total_minutes,
		"roads": roads.to_dict(),
		"lots": lots.to_dict(),
	}


## Returns null if the data is from an unsupported save version.
static func from_dict(data: Dictionary) -> Simulation:
	if int(data.get("version", -1)) != SAVE_VERSION:
		return null
	var sim := Simulation.new(RoadNetwork.from_dict(data.roads))
	sim.tick_count = int(data.tick_count)
	sim.clock.total_minutes = float(data.clock_minutes)
	sim.lots.restore(data.get("lots", {}))
	return sim
