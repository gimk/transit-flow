class_name Vehicle
extends RefCounted
## A vehicle driving a route. Only VehicleSystem creates, moves and removes vehicles.
## `distance` is the position of the vehicle's center along its current lane.

enum Kind { CAR, TRUCK }

const CAR_LENGTH := 11.0
const CAR_WIDTH := 6.0
const TRUCK_LENGTH := 17.0
const TRUCK_WIDTH := 7.0
## The longest vehicle there is: assumed wherever a vehicle's length isn't known yet.
const MAX_LENGTH := TRUCK_LENGTH

var id: int
var kind := Kind.CAR
var length := CAR_LENGTH
var width := CAR_WIDTH
## Who the trip belongs to (a citizen id, or a debug/traffic owner code).
var owner_id := -1
## Where the trip ends. If `destination_lot_id` is set, re-routing follows that lot's
## (possibly moved) access point; otherwise the fixed lane position below is used.
var destination_lot_id := -1
var destination_lane_id := -1
var destination_distance := 0.0

## Held by reference, not id: a junction rebuild can replace the connector a vehicle is
## on, and the vehicle should still be able to finish driving it.
var lane: Lane
var distance := 0.0
var speed := 0.0
## Lanes still to drive after the current one.
var next_lanes: Array[Lane] = []
## Where to stop on the last lane of the route.
var end_distance := 0.0

## Junction access: the connector this vehicle has been allowed into next (null = none),
## and the one it is asking for while it waits at the stop line, since when.
var granted_connector: Lane = null
var requested_connector: Lane = null
var waiting_since := INF
## When it was let into its current or next junction (VehicleSystem's grant counter).
var entry_order := 0

## Trip timing (sim seconds): when it set off, how long it would take with empty roads,
## and when it ended. Drives patience and the Flow score.
var trip_start_time := 0.0
var ideal_trip_time := 0.0
var trip_end_time := 0.0
## Running late (shown in red); `gave_up` once patience ran out and the trip was abandoned.
var impatient := false
var gave_up := false

## Per-tick scratch filled by VehicleSystem before vehicles move: bumper-to-bumper gap to
## the vehicle ahead and its speed, and the distance to a stop line it may not cross.
var leader_gap := INF
var leader_speed := 0.0
var stop_gap := INF
## How long the vehicle has been (nearly) stationary, in sim seconds.
var stopped_time := 0.0
## How long it has been crawling well below the speed limit (held up), in sim seconds.
var slow_time := 0.0

## Pose after the last two ticks, for interpolated rendering.
var position := Vector2.ZERO
var rotation := 0.0
var previous_position := Vector2.ZERO
var previous_rotation := 0.0


func set_kind(p_kind: Kind) -> void:
	kind = p_kind
	length = TRUCK_LENGTH if kind == Kind.TRUCK else CAR_LENGTH
	width = TRUCK_WIDTH if kind == Kind.TRUCK else CAR_WIDTH


func is_on_last_lane() -> bool:
	return next_lanes.is_empty()


## Distance left before the end of the current lane, or before the stop on the last lane.
func get_remaining_on_lane() -> float:
	return (end_distance if is_on_last_lane() else lane.length) - distance
