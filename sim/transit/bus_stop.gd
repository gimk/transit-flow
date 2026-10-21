class_name BusStop
extends RefCounted
## A curbside bus stop: a point on the outermost lane of a road, for the traffic going
## one way. Buses stop there in the lane, holding up the traffic behind while they dwell
## (bus bays come later). Kept as a position and direction, since lane ids change as roads
## are edited: TransitSystem finds its lane again after every road change.

var id: int
var position: Vector2
## Direction of the traffic it serves.
var direction: Vector2
## The lane it is on and where along it (lane_id -1: no road there any more).
var lane_id := -1
var distance := 0.0
## Riders waiting here (TransitSystem.Rider), first come first.
var waiting: Array = []


func _init(p_id: int, p_position: Vector2, p_direction: Vector2) -> void:
	id = p_id
	position = p_position
	direction = p_direction


func is_active() -> bool:
	return lane_id != -1
