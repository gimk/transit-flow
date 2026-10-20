class_name SimClock
extends RefCounted
## In-game time of day. Advanced only by the simulation tick, never by frame delta.

signal hour_changed(hour: int)
signal day_started(day: int)

enum Period { MORNING_RUSH, MIDDAY_LOGISTICS, EVENING_RUSH, NIGHT }

const PERIOD_NAMES := {
	Period.MORNING_RUSH: "Morning Rush",
	Period.MIDDAY_LOGISTICS: "Midday Logistics",
	Period.EVENING_RUSH: "Evening Rush",
	Period.NIGHT: "Night",
}

## A full day lasts MINUTES_PER_DAY / GAME_MINUTES_PER_SIM_SECOND = 288 s at 1x speed.
const GAME_MINUTES_PER_SIM_SECOND := 5.0
const MINUTES_PER_DAY := 24 * 60
const START_HOUR := 6

## Game minutes elapsed since midnight of day 1.
var total_minutes: float = START_HOUR * 60.0


func advance(sim_seconds: float) -> void:
	var previous_hours := int(total_minutes / 60.0)
	total_minutes += sim_seconds * GAME_MINUTES_PER_SIM_SECOND
	var current_hours := int(total_minutes / 60.0)
	for absolute_hour in range(previous_hours + 1, current_hours + 1):
		var hour := absolute_hour % 24
		if hour == 0:
			day_started.emit(absolute_hour / 24 + 1)
		hour_changed.emit(hour)


func get_day() -> int:
	return int(total_minutes / MINUTES_PER_DAY) + 1


func get_minute_of_day() -> float:
	return fmod(total_minutes, MINUTES_PER_DAY)


func get_hour() -> int:
	return int(get_minute_of_day() / 60.0)


func get_minute() -> int:
	return int(get_minute_of_day()) % 60


## Time of day as fractional hours (0.0 – 24.0), handy for sampling demand curves.
func get_time_of_day_hours() -> float:
	return get_minute_of_day() / 60.0


func format_time() -> String:
	return "%02d:%02d" % [get_hour(), get_minute()]


func get_period() -> Period:
	return period_for_hour(get_hour())


func get_period_name() -> String:
	return PERIOD_NAMES[get_period()]


static func period_for_hour(hour: int) -> Period:
	if hour >= 6 and hour < 10:
		return Period.MORNING_RUSH
	if hour >= 10 and hour < 16:
		return Period.MIDDAY_LOGISTICS
	if hour >= 16 and hour < 20:
		return Period.EVENING_RUSH
	return Period.NIGHT
