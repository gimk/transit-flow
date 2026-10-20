class_name TrafficSignal
extends RefCounted
## Traffic-light timing. Stateless: the light for a phase follows from the sim time, so
## nothing needs saving or stepping. Each phase gets GREEN_TIME, then CLEARANCE_TIME in
## which nobody new may enter (amber/all-red) so the junction empties before the next
## phase. All lights share one clock, which gives simple green waves on a grid.

enum Light { GREEN, AMBER, RED }

const GREEN_TIME := 10.0  ## Sim seconds (50 game minutes at the current clock rate).
const CLEARANCE_TIME := 2.0


static func get_light(node: RoadNode, phase: int, time: float) -> Light:
	if node.signal_phase_count <= 1:
		return Light.GREEN
	var slot_length := GREEN_TIME + CLEARANCE_TIME
	var into_cycle := fmod(time, node.signal_phase_count * slot_length)
	var active_phase := int(into_cycle / slot_length)
	if active_phase != phase:
		return Light.RED
	return Light.GREEN if into_cycle - active_phase * slot_length < GREEN_TIME else Light.AMBER
