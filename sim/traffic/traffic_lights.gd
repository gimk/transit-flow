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
## Phases belong to the junction's turns (Lane.signal_phase on connectors): the two road
## axes, plus their left turns when the junction has protected left turns. Pedestrians
## waiting call the phase they cross with, and may start only early in its green
## (is_walk_green).
## Light state isn't saved: after loading, lights start over on phase 0.

enum Light { GREEN, AMBER, RED }

const MIN_GREEN := 3.0  ## Sim seconds.
const MAX_GREEN := 12.0
const CLEARANCE_TIME := 1.0
## Pedestrians may start crossing only this long into their green (see is_walk_green).
const WALK_INTERVAL := 4.0
## On a green that rests (nobody waiting on another phase), the walk light comes round
## again this long after it last came on, if pedestrians are waiting; turning traffic
## gets the time between.
const WALK_RECALL := 12.0
## A vehicle this close to its stop line (px, front-most on its lane) is detected.
const DETECTION_DISTANCE := 60.0
## A detected vehicle standing still this long on green counts as blocked, not flowing.
const BLOCKED_TIME := 1.0


class LightState:
	var phase := 0
	var next_phase := 0
	var clearing := false
	var elapsed := 0.0
	## The phases it was set up for; a different set (lights changed) starts over.
	var phase_count := 0


## Pedestrians at crosswalks (set by Simulation), whose waiting calls walk phases.
var crosswalks: Crosswalks

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


## Walk light for the crosswalk across road `segment_id` at `node`: pedestrians cross
## alongside the traffic of the other road axis, during the first WALK_INTERVAL of its
## through phase's green (then "don't walk" flashes: those on the crosswalk finish, nobody
## new starts, so it clears for turning traffic before the green ends). Always on where
## the lights can't alternate.
func is_walk_green(node: RoadNode, segment_id: int) -> bool:
	var phase := get_walk_phase(node, segment_id)
	if phase == -1:
		return true
	var state: LightState = _states.get(node.id)
	return get_light(node, phase) == Light.GREEN and (state == null or state.elapsed < WALK_INTERVAL)


## The light phase that lets pedestrians cross road `segment_id` at `node`, or -1 if any.
func get_walk_phase(node: RoadNode, segment_id: int) -> int:
	if node.signal_phase_count <= 1 or not _roads.segments.has(segment_id):
		return -1
	var axis := -1
	for lane_id in _roads.segments[segment_id].lane_ids:
		var lane := _roads.lanes[lane_id]
		if lane.to_node_id == node.id:
			axis = lane.signal_phase
			break
	if axis == -1:
		# A one-way road leaving the junction: its axis from its direction.
		var direction := LaneBuilder.start_direction(_roads.lanes[_roads.segments[segment_id].lane_ids[0]].curve)
		axis = 0 if absf(direction.x) >= absf(direction.y) else 1
	var alongside := 1 - axis
	return alongside * 2 if node.signal_phase_count == 4 else alongside


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
	if waiting_phase == -1 and state.elapsed >= WALK_RECALL and _pedestrian_demand(node)[state.phase]:
		# People arrived after the walk light went off, and nobody else waits: walk again
		# (a new green, as far as timing goes).
		state.elapsed = 0.0
		return
	if waiting_phase == -1 or state.elapsed < MIN_GREEN:
		return
	if _detect_demand(node, true)[state.phase] and state.elapsed < MAX_GREEN:
		return
	state.next_phase = waiting_phase
	state.clearing = true
	state.elapsed = 0.0


## Per phase: is a vehicle about to take a turn of that phase (the front-most vehicle on
## an incoming lane, close to its stop line, by the connector it is heading for)? With
## `moving_only`, vehicles standing still for BLOCKED_TIME don't count.
func _detect_demand(node: RoadNode, moving_only: bool) -> Array[bool]:
	# Pedestrians waiting at a crosswalk call its walk phase (they never extend a green).
	var demand := _pedestrian_demand(node) if not moving_only else _no_demand(node)
	for segment_id in node.segment_ids:
		for lane_id in _roads.segments[segment_id].lane_ids:
			var lane := _roads.lanes[lane_id]
			if lane.to_node_id != node.id or lane.queue.is_empty():
				continue
			var front: Vehicle = lane.queue[0]
			if moving_only and front.stopped_time >= BLOCKED_TIME:
				continue
			if front.next_lanes.is_empty() or not front.next_lanes[0].is_connector():
				continue
			if lane.length - front.distance <= DETECTION_DISTANCE:
				demand[front.next_lanes[0].signal_phase] = true
	return demand


func _on_roads_changed(node_ids: Array, _segment_ids: Array) -> void:
	for node_id: int in node_ids:
		var node: RoadNode = _roads.nodes.get(node_id)
		if node == null or not node.has_lights():
			_states.erase(node_id)
		elif not _states.has(node_id) or _states[node_id].phase_count != node.signal_phase_count:
			_states[node_id] = LightState.new()
			_states[node_id].phase_count = node.signal_phase_count


## Per phase: are pedestrians waiting to cross with it?
func _pedestrian_demand(node: RoadNode) -> Array[bool]:
	var demand := _no_demand(node)
	if crosswalks != null and not crosswalks.waiting.is_empty():
		for crosswalk: Vector2i in crosswalks.waiting:
			if crosswalk.x == node.id:
				var walk_phase := get_walk_phase(node, crosswalk.y)
				if walk_phase != -1:
					demand[walk_phase] = true
	return demand


static func _no_demand(node: RoadNode) -> Array[bool]:
	var demand: Array[bool] = []
	demand.resize(node.signal_phase_count)
	demand.fill(false)
	return demand
