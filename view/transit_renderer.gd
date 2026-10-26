class_name TransitRenderer
extends Node2D
## Bus lines and stops, from the latest tick snapshot, on three child layers (so the dot
## display can mark each its own way, DotDisplay.ROUTES): Lines, each line's way round as
## a ribbon in its colour along the curb side of the lanes it drives (lines sharing a
## road side by side); Stops, each stop as a sign at the curb (beyond its pull-in, for a
## bus bay); Ground, bus bays and the stops' crosswalks as zebras. Under the dot display
## lines and signs are opaque and at least a dot cell wide, so they read at any zoom.
## Where a stop's waiting riders stand (rider_spot) and get on (door_spot) is here too;
## PedestrianRenderer draws them, as people.

const LINE_WIDTH := 3.0
const LINE_ALPHA := 0.35
## Ribbons start this far right of the lane the buses drive (toward the curb).
const LINE_OFFSET := 4.0
const SIGN_COLOR := Color("f2efe6")
const SIGN_SIZE := Vector2(7.0, 4.0)
## From the stop (on its lane) out to the sign and to the waiting riders.
const SIGN_OFFSET := 11.0
const RIDERS_OFFSET := 17.0
const RIDER_SPACING := 5.0
const MAX_DRAWN_RIDERS := 12
## A bus bay: a pull-in the length of two buses beside the lane.
const BAY_SIZE := Vector2(2.0 * Vehicle.BUS_LENGTH + 6.0, 10.0)
## How much further out the sign and riders stand at a bay.
const BAY_SIGN_SHIFT := 8.0
const BAY_COLOR := Color(0.27, 0.29, 0.32)
const BAY_EDGE_COLOR := Color(1, 1, 1, 0.3)
## Redraw every this many frames (riders come and go a few times a second at most).
const REDRAW_FRAMES := 6

var _ground := Node2D.new()
var _lines := Node2D.new()
var _stops := Node2D.new()
## The dot cell the layers were last drawn for (DotDisplay.current_cell).
var _drawn_cell := -1.0


func _ready() -> void:
	for layer: Array in [[_ground, "Ground", _draw_ground], [_lines, "Lines", _draw_lines], [_stops, "Stops", _draw_stops]]:
		var node: Node2D = layer[0]
		node.name = layer[1]
		node.draw.connect(layer[2])
		add_child(node)


func _process(_delta: float) -> void:
	if Engine.get_process_frames() % REDRAW_FRAMES == 0 or DotDisplay.current_cell != _drawn_cell:
		_drawn_cell = DotDisplay.current_cell
		for node: Node2D in [_ground, _lines, _stops]:
			node.queue_redraw()


func _draw_lines() -> void:
	var view := Game.view_to
	var dots := DotDisplay.current_cell > 0.0
	var width := maxf(LINE_WIDTH, DotDisplay.current_cell)
	for k in view.bus_lines.size():
		var line: Dictionary = view.bus_lines[k]
		var path: PackedVector2Array = line.path
		if path.size() >= 2:
			var color := Color(line.color, 1.0 if dots else LINE_ALPHA)
			_draw_ribbon(path, LINE_OFFSET + (k + 0.5) * width, color, width)


## A line's ribbon: each piece of `path` moved `offset` to its right on its own, with
## round joints. A mitred offset (LaneBuilder.offset_polyline) spikes where a bus path
## doubles back (round a loop's end, into a bay).
func _draw_ribbon(path: PackedVector2Array, offset: float, color: Color, width: float) -> void:
	for i in path.size() - 1:
		var a := path[i]
		var b := path[i + 1]
		if a.distance_squared_to(b) < 0.01:
			continue
		var right := (b - a).normalized().orthogonal() * -offset
		_lines.draw_line(a + right, b + right, color, width)
		if color.a >= 1.0:
			# (Joints would show as darker spots on the see-through ribbons of the old look.)
			_lines.draw_circle(b + right, width * 0.5, color)


func _draw_ground() -> void:
	var view := Game.view_to
	for crossing: Dictionary in view.stop_crossings.values():
		_draw_zebra(crossing.center, crossing.across, crossing.half_width)
	for i in view.stop_positions.size():
		if view.stop_bays[i] != 1:
			continue
		# The bay: a pull-in beside the lane, cut into the sidewalk.
		var along := view.stop_directions[i]
		var out := Vector2(-along.y, along.x)
		_ground.draw_set_transform(view.stop_positions[i] + out * VehicleSystem.BAY_OFFSET, along.angle())
		_ground.draw_rect(Rect2(-BAY_SIZE * 0.5, BAY_SIZE), BAY_COLOR)
		_ground.draw_rect(Rect2(-BAY_SIZE * 0.5, BAY_SIZE), BAY_EDGE_COLOR, false, 1.0)
	_ground.draw_set_transform(Vector2.ZERO)


func _draw_stops() -> void:
	var view := Game.view_to
	var size := SIGN_SIZE.max(Vector2.ONE * DotDisplay.current_cell * 1.5)
	for i in view.stop_positions.size():
		var along := view.stop_directions[i]
		# Toward the curb: right of the traffic's direction (y grows downward).
		var out := Vector2(-along.y, along.x)
		var beyond := BAY_SIGN_SHIFT if view.stop_bays[i] == 1 else 0.0
		_stops.draw_set_transform(view.stop_positions[i] + out * (SIGN_OFFSET + beyond), along.angle())
		_stops.draw_rect(Rect2(-size * 0.5, size), SIGN_COLOR)
		_stops.draw_rect(Rect2(-size * 0.5, size), Color.BLACK, false, 1.0)
		# (Its waiting riders are drawn by PedestrianRenderer, as people.)
	_stops.draw_set_transform(Vector2.ZERO)


## Zebra stripes across the road at a stop's crosswalk (like RoadMarkings' at junctions).
func _draw_zebra(center: Vector2, across: Vector2, half_width: float) -> void:
	var along := across.orthogonal()
	var offset := -half_width + RoadMarkings.ZEBRA_SPACING * 0.5
	while offset < half_width:
		var stripe := center + across * offset
		_ground.draw_line(stripe - along * RoadMarkings.ZEBRA_LENGTH * 0.5, stripe + along * RoadMarkings.ZEBRA_LENGTH * 0.5,
			RoadMarkings.ZEBRA_COLOR, RoadMarkings.ZEBRA_WIDTH)
		offset += RoadMarkings.ZEBRA_SPACING


## Where the `k`th of `waiting` riders stands at stop `i` of a snapshot: in a row on the
## sidewalk beside the sign.
static func rider_spot(view: ViewSnapshot, i: int, k: int, waiting: int) -> Vector2:
	var along := view.stop_directions[i]
	var out := Vector2(-along.y, along.x)
	var beyond := BAY_SIGN_SHIFT if view.stop_bays[i] == 1 else 0.0
	var offset := (k - (waiting - 1) * 0.5) * RIDER_SPACING
	return view.stop_positions[i] + out * (RIDERS_OFFSET + beyond) + along * offset


## Where riders get on at stop `i` of a snapshot: beside the front door of a bus standing
## there (centred on the stop; TransitSystem.get_door_position).
static func door_spot(view: ViewSnapshot, i: int) -> Vector2:
	var along := view.stop_directions[i]
	var out := Vector2(-along.y, along.x)
	var beyond := VehicleSystem.BAY_OFFSET if view.stop_bays[i] == 1 else 0.0
	return view.stop_positions[i] + along * (Vehicle.BUS_LENGTH * 0.5 - TransitSystem.DOOR_SETBACK) \
		+ out * (Vehicle.BUS_WIDTH * 0.5 + 1.0 + beyond)
