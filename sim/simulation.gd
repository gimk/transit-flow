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
var zones: ZoneSystem
var externals: ExternalConnections
var signals: TrafficLights
var vehicles: VehicleSystem
var crosswalks := Crosswalks.new()
var pedestrians: PedestrianSystem
var citizens: CitizenSystem
var freight: FreightSystem
var stress: StressTraffic
var congestion: CongestionMonitor
var flow: FlowMetric
var tick_count := 0


func _init(p_roads: RoadNetwork = null) -> void:
	roads = p_roads if p_roads != null else RoadNetwork.new()
	router = Router.new(roads)
	lots = LotSystem.new(roads)
	zones = ZoneSystem.new(roads, lots)
	externals = ExternalConnections.new(roads)
	signals = TrafficLights.new(roads)
	vehicles = VehicleSystem.new(roads, router, lots, signals)
	signals.crosswalks = crosswalks
	vehicles.crosswalks = crosswalks
	pedestrians = PedestrianSystem.new(roads, lots, signals, vehicles, crosswalks)
	citizens = CitizenSystem.new(lots, router, vehicles, pedestrians, clock)
	freight = FreightSystem.new(lots, externals, router, vehicles, clock)
	stress = StressTraffic.new(roads, router, vehicles)
	congestion = CongestionMonitor.new(roads, router, vehicles)
	flow = FlowMetric.new(vehicles, pedestrians)


func step() -> void:
	tick_count += 1
	clock.advance(TICK_DT)
	zones.step(get_sim_time())
	citizens.step(get_sim_time())
	freight.step(TICK_DT)
	stress.step()
	signals.step(TICK_DT)
	pedestrians.step(get_sim_time(), TICK_DT)
	vehicles.step(TICK_DT)
	congestion.step(get_sim_time())


## Breaks internal reference cycles so the simulation can be freed. Call before
## dropping the last reference to a simulation (Game does this when replacing it).
func dispose() -> void:
	vehicles.dispose()
	roads.dispose()


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
		"zones": zones.to_dict(),
		"externals": externals.to_dict(),
	}


## Returns null if the data is from an unsupported save version.
static func from_dict(data: Dictionary) -> Simulation:
	if int(data.get("version", -1)) != SAVE_VERSION:
		return null
	var sim := Simulation.new(RoadNetwork.from_dict(data.roads))
	sim.tick_count = int(data.tick_count)
	sim.clock.total_minutes = float(data.clock_minutes)
	sim.lots.restore(data.get("lots", {}))
	sim.zones.restore(data.get("zones", {}))
	sim.externals.restore(data.get("externals", {}))
	return sim
