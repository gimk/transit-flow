class_name Citizen
extends RefCounted
## A lightweight commuter record. Citizens are only simulated as vehicles while
## travelling; the rest of the time they are just a state and a departure time.

enum State { AT_HOME, DRIVING_TO_WORK, AT_WORK, DRIVING_HOME }

var id: int
var home_lot_id: int
var work_lot_id := -1
var state := State.AT_HOME
## Sim time at which the citizen next tries to leave (when at home or at work).
var next_departure := 0.0
var vehicle_id := -1


func _init(p_id: int, p_home_lot_id: int) -> void:
	id = p_id
	home_lot_id = p_home_lot_id


func is_travelling() -> bool:
	return state == State.DRIVING_TO_WORK or state == State.DRIVING_HOME
