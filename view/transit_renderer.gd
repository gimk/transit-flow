class_name TransitRenderer
extends Node2D
## Bus lines and stops, from the latest tick snapshot: each line's way round as a faint
## coloured band, each stop as a sign at the curb (beyond its pull-in, for a bus bay), and
## its crosswalk as a zebra. Where its waiting riders stand (rider_spot) and get on
## (door_spot) is here too; PedestrianRenderer draws them, as people.

const LINE_WIDTH := 3.0
const LINE_ALPHA := 0.35
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


func _process(_delta: float) -> void:
	if Engine.get_process_frames() % REDRAW_FRAMES == 0:
		queue_redraw()


func _draw() -> void:
	var view := Game.view_to
	for line: Dictionary in view.bus_lines:
		var path: PackedVector2Array = line.path
		if path.size() >= 2:
			var color: Color = line.color
			draw_polyline(path, Color(color, LINE_ALPHA), LINE_WIDTH)
	for crossing: Dictionary in view.stop_crossings.values():
		_draw_zebra(crossing.center, crossing.across, crossing.half_width)
	for i in view.stop_positions.size():
		var along := view.stop_directions[i]
		# Toward the curb: right of the traffic's direction (y grows downward).
		var out := Vector2(-along.y, along.x)
		var bay := view.stop_bays[i] == 1
		if bay:
			# The bay: a pull-in beside the lane, cut into the sidewalk.
			draw_set_transform(view.stop_positions[i] + out * VehicleSystem.BAY_OFFSET, along.angle())
			draw_rect(Rect2(-BAY_SIZE * 0.5, BAY_SIZE), BAY_COLOR)
			draw_rect(Rect2(-BAY_SIZE * 0.5, BAY_SIZE), BAY_EDGE_COLOR, false, 1.0)
			draw_set_transform(Vector2.ZERO)
		var beyond := BAY_SIGN_SHIFT if bay else 0.0
		var sign_at := view.stop_positions[i] + out * (SIGN_OFFSET + beyond)
		draw_set_transform(sign_at, along.angle())
		draw_rect(Rect2(-SIGN_SIZE * 0.5, SIGN_SIZE), SIGN_COLOR)
		draw_rect(Rect2(-SIGN_SIZE * 0.5, SIGN_SIZE), Color.BLACK, false, 1.0)
		draw_set_transform(Vector2.ZERO)
		# (Its waiting riders are drawn by PedestrianRenderer, as people.)


## Zebra stripes across the road at a stop's crosswalk (like RoadMarkings' at junctions).
func _draw_zebra(center: Vector2, across: Vector2, half_width: float) -> void:
	var along := across.orthogonal()
	var offset := -half_width + RoadMarkings.ZEBRA_SPACING * 0.5
	while offset < half_width:
		var stripe := center + across * offset
		draw_line(stripe - along * RoadMarkings.ZEBRA_LENGTH * 0.5, stripe + along * RoadMarkings.ZEBRA_LENGTH * 0.5,
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
