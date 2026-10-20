extends RefCounted
## Traffic safety checks shared by the stress and scenario tests (preload this script).


## Smallest bumper-to-bumper gap between consecutive vehicles on any lane.
static func min_queue_gap(sim: Simulation) -> float:
	var smallest := INF
	for lane_id in sim.roads.lanes:
		var queue := sim.vehicles.get_lane_queue(lane_id)
		for i in range(1, queue.size()):
			smallest = minf(smallest, queue[i - 1].distance - queue[i].distance - (queue[i - 1].length + queue[i].length) * 0.5)
	return smallest


## Pairs of vehicles that are both inside the stretch two conflicting connectors share.
static func connector_conflict_violations(sim: Simulation) -> int:
	var on_connectors: Array[Vehicle] = []
	for vehicle: Vehicle in sim.vehicles.vehicles.values():
		if vehicle.lane.is_connector():
			on_connectors.append(vehicle)
	var violations := 0
	for a in on_connectors:
		for b in on_connectors:
			if a.id < b.id and a.lane.conflict_zones.has(b.lane.id):
				if _in_zone(a, a.lane.conflict_zones[b.lane.id]) and _in_zone(b, b.lane.conflict_zones[a.lane.id]):
					violations += 1
	return violations


static func _in_zone(vehicle: Vehicle, zone: Vector2) -> bool:
	return vehicle.distance + vehicle.length * 0.5 >= zone.x and vehicle.distance - vehicle.length * 0.5 <= zone.y
