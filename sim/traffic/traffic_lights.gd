class_name TrafficLights
extends RefCounted
## Vehicle-actuated traffic lights. Each light detects vehicles near its stop lines and:
## - rests on green for as long as nobody is waiting on another phase (so a single busy
##   approach simply keeps its green),
## - otherwise holds green at least MIN_GREEN, keeps it while its own vehicles keep
##   moving through, and gives way once its approach is empty, blocked (its vehicles
##   stand still on green, e.g. because the road beyond is full) or after MAX_GREEN,
## - then shows amber for CLEARANCE_TIME (no new entries) before the next phase that
##   has someone waiting turns green; phases nobody waits for are skipped.
## Light state isn't saved: after loading, lights start over on phase 0.

enum Light { GREEN, AMBER, RED }

const MIN_GREEN := 3.0  ## Sim seconds.
const MAX_GREEN := 12.0
const CLEARANCE_TIME := 1.0
## A vehicle this close to its stop line (px, front-most on its lane) is detected.
const DETECTION_DISTANCE := 60.0
## A detected vehicle standing still this long on green counts as blocked, not flowing.
const BLOCKED_TIME := 1.0


class LightState:
	var phase := 0
	var next_phase := 0
	var clearing := false
	var elapsed := 0.0


var _roads: RoadNetwork
## Node id -> state, for every junction with traffic lights.
var _states: Dictionary[int, LightState] = {}


func _init(roads: RoadNetwork) -> void:
	_roads = roads
	_roads.changed.connect(_on_roads_changed)
	_on_roads_changed(_roads.nodes.keys(), [])


func step(dt: float) -> void:
	for node_id in _states:
		_update(_roads.nodes[node_id], _states[node_id], dt)


func get_light(node: RoadNode, phase: int) -> Light:
	if node.signal_phase_count <= 1:
		return Light.GREEN
	var state: LightState = _states.get(node.id)
	if state == null:
		return Light.GREEN if phase == 0 else Light.RED
	if state.clearing:
		return Light.AMBER if phase == state.phase else Light.RED
	return Light.GREEN if phase == state.phase else Light.RED


func _update(node: RoadNode, state: LightState, dt: float) -> void:
	state.elapsed += dt
	if state.clearing:
		if state.elapsed >= CLEARANCE_TIME:
			state.clearing = false
			state.phase = state.next_phase
			state.elapsed = 0.0
		return
	var count := node.signal_phase_count
	if count <= 1:
		return
	var demand := _detect_demand(node, false)
	var waiting_phase := -1
	for offset in range(1, count):
		var phase := (state.phase + offset) % count
		if demand[phase]:
			waiting_phase = phase
			break
	if waiting_phase == -1 or state.elapsed < MIN_GREEN:
		return
	if _detect_demand(node, true)[state.phase] and state.elapsed < MAX_GREEN:
		return
	state.next_phase = waiting_phase
	state.clearing = true
	state.elapsed = 0.0


## Per phase: is a vehicle about to cross a stop line of that phase? With
## `moving_only`, vehicles standing still for BLOCKED_TIME don't count.
func _detect_demand(node: RoadNode, moving_only: bool) -> Array[bool]:
	var demand: Array[bool] = []
	demand.resize(node.signal_phase_count)
	demand.fill(false)
	for segment_id in node.segment_ids:
		for lane_id in _roads.segments[segment_id].lane_ids:
			var lane := _roads.lanes[lane_id]
			if lane.to_node_id != node.id or lane.queue.is_empty():
				continue
			var front: Vehicle = lane.queue[0]
			if moving_only and front.stopped_time >= BLOCKED_TIME:
				continue
			if not front.is_on_last_lane() and lane.length - front.distance <= DETECTION_DISTANCE:
				demand[lane.signal_phase] = true
	return demand


func _on_roads_changed(node_ids: Array, _segment_ids: Array) -> void:
	for node_id: int in node_ids:
		var node: RoadNode = _roads.nodes.get(node_id)
		if node == null or node.control != RoadNode.JunctionControl.TRAFFIC_LIGHT:
			_states.erase(node_id)
		elif not _states.has(node_id) or _states[node_id].phase >= node.signal_phase_count:
			_states[node_id] = LightState.new()
