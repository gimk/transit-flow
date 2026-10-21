class_name Citizen
extends RefCounted
## A lightweight person record. Citizens are only simulated as vehicles while travelling;
## the rest of the time they are a state plus a personal daily schedule.

enum State { AT_HOME, TO_WORK, AT_WORK, TO_SHOP, AT_SHOP, TO_HOME }

var id: int
var home_lot_id: int
var work_lot_id := -1
var state := State.AT_HOME
var vehicle_id := -1
## Walking instead (PedestrianSystem), or -1.
var pedestrian_id := -1
## Lot the current trip heads to.
var trip_lot_id := -1

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


func is_travelling() -> bool:
	return state == State.TO_WORK or state == State.TO_SHOP or state == State.TO_HOME
