extends GutTest

var clock: SimClock


func before_each() -> void:
	clock = SimClock.new()


func _advance_game_minutes(minutes: float) -> void:
	clock.advance(minutes / SimClock.GAME_MINUTES_PER_SIM_SECOND)


func test_starts_on_day_one_at_six() -> void:
	assert_eq(clock.get_day(), 1)
	assert_eq(clock.format_time(), "06:00")
	assert_eq(clock.get_period(), SimClock.Period.MORNING_RUSH)


func test_advance_converts_sim_seconds_to_game_minutes() -> void:
	clock.advance(3.0)
	assert_eq(clock.format_time(), "06:15")


func test_hour_changed_emitted_once_per_hour_crossed() -> void:
	watch_signals(clock)
	_advance_game_minutes(150)  # 06:00 -> 08:30
	assert_signal_emit_count(clock, "hour_changed", 2)
	assert_signal_emitted_with_parameters(clock, "hour_changed", [8])


func test_large_step_does_not_skip_hours() -> void:
	watch_signals(clock)
	_advance_game_minutes(5 * 60)
	assert_signal_emit_count(clock, "hour_changed", 5)


func test_day_rolls_over_at_midnight() -> void:
	watch_signals(clock)
	_advance_game_minutes(18 * 60)  # 06:00 -> 00:00 next day
	assert_eq(clock.get_day(), 2)
	assert_eq(clock.format_time(), "00:00")
	assert_signal_emitted_with_parameters(clock, "day_started", [2])
	assert_signal_emit_count(clock, "day_started", 1)


func test_period_boundaries() -> void:
	assert_eq(SimClock.period_for_hour(5), SimClock.Period.NIGHT)
	assert_eq(SimClock.period_for_hour(6), SimClock.Period.MORNING_RUSH)
	assert_eq(SimClock.period_for_hour(10), SimClock.Period.MIDDAY_LOGISTICS)
	assert_eq(SimClock.period_for_hour(16), SimClock.Period.EVENING_RUSH)
	assert_eq(SimClock.period_for_hour(20), SimClock.Period.NIGHT)
	assert_eq(SimClock.period_for_hour(23), SimClock.Period.NIGHT)
