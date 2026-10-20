class_name Vehicle
extends RefCounted
## A vehicle driving a route. (Phase 2b moves vehicle data to struct-of-arrays inside
## VehicleSystem; keep access to vehicles going through VehicleSystem.)

var id: int
## Who the trip belongs to (a citizen id for now).
var owner_id := -1
var destination_lot_id := -1

## Held by reference, not id: a junction rebuild can replace the connector a vehicle is
## on, and the vehicle should still be able to finish driving it.
var lane: Lane
var distance := 0.0
var speed := 0.0
## Lanes still to drive after the current one.
var next_lane_ids: Array[int] = []
## Where to stop on the last lane of the route.
var end_distance := 0.0

## Pose after the last two ticks, for interpolated rendering.
var position := Vector2.ZERO
var rotation := 0.0
var previous_position := Vector2.ZERO
var previous_rotation := 0.0


func is_on_last_lane() -> bool:
	return next_lane_ids.is_empty()


## Distance left before the end of the current lane, or before the stop on the last lane.
func get_remaining_on_lane() -> float:
	return (end_distance if is_on_last_lane() else lane.length) - distance
