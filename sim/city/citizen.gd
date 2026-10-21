class_name Citizen
extends RefCounted
## A lightweight person record. Citizens are only simulated (as a vehicle or a
## pedestrian) while travelling; the rest of the time they are a state plus a personal
## daily schedule. A trip is a list of legs, each walked or driven (CitizenSystem).

enum State { AT_HOME, TO_WORK, AT_WORK, TO_SHOP, AT_SHOP, TO_HOME }
## How a leg is travelled; a trip's mode is its main one (any car leg makes it a car trip).
enum Mode { WALK, CAR }

const MODE_NAMES := { Mode.WALK: "Walk", Mode.CAR: "Car" }

var id: int
var home_lot_id: int
var work_lot_id := -1
var state := State.AT_HOME
## The current leg's vehicle or pedestrian (-1 if none).
var vehicle_id := -1
var pedestrian_id := -1
## Lot the current trip heads to.
var trip_lot_id := -1

## The citizen's car, if they own one: the lot it is parked at (or heading to, while it
## is driven). They can only drive from where it is.
var has_car := true
var car_lot_id := -1
## Personal taste for driving: multiplies the car's cost in mode choice (below 1 likes it).
var car_preference := 1.0

## The current trip: legs still to go ({ "mode": Mode, "from": lot id, "to": lot id }),
## its main mode, when it started, and its ideal (free-flow) time, summed over the legs
## started so far.
var legs: Array[Dictionary] = []
var trip_mode := Mode.CAR
var trip_start_time := 0.0
var trip_ideal_time := 0.0
var trip_end_time := 0.0

## Personal schedule, in hours of the day.
var leave_home_hour := 7.5
var leave_work_hour := 17.5
## Day on which the citizen last set off to work (they go once a day).
var last_commute_day := 0
## Debug: leave home as soon as possible, ignoring the schedule (once).
var leave_now := false
## Evening shopping: decided once per day when leaving work.
var going_shopping := false
var shopping_decided_day := 0
## Sim time until which the citizen stays at the shop.
var shop_until := 0.0
## Sim time before which no new trip is attempted (after a failed start).
var retry_at := 0.0


func _init(p_id: int, p_home_lot_id: int) -> void:
	id = p_id
	home_lot_id = p_home_lot_id
	car_lot_id = p_home_lot_id


func is_travelling() -> bool:
	return state == State.TO_WORK or state == State.TO_SHOP or state == State.TO_HOME
