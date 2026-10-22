extends GutTest
## When windows are lit (NightLights.is_window_lit): homes in the evening and early
## morning, shops in the evening, a night shift in factories.


func _share_lit(kind: Lot.Type, hours: float) -> float:
	var lit := 0
	for seed in 500:
		if NightLights.is_window_lit(kind, hash(seed), hours):
			lit += 1
	return lit / 500.0


func test_homes_light_up_in_the_evening_and_go_dark_late_at_night() -> void:
	assert_gt(_share_lit(Lot.Type.RESIDENTIAL, 20.0), 0.8, "most at 8 pm")
	assert_lt(_share_lit(Lot.Type.RESIDENTIAL, 23.5), 0.6, "fewer by 11:30 pm")
	assert_eq(_share_lit(Lot.Type.RESIDENTIAL, 3.0), 0.0, "dark at 3 am")
	assert_gt(_share_lit(Lot.Type.RESIDENTIAL, 5.9), 0.3, "some up early")


func test_shops_close_and_factories_keep_a_night_shift() -> void:
	assert_eq(_share_lit(Lot.Type.COMMERCIAL, 18.0), 1.0, "shops open at 6 pm")
	assert_eq(_share_lit(Lot.Type.COMMERCIAL, 3.0), 0.0, "closed at 3 am")
	var shift := _share_lit(Lot.Type.INDUSTRIAL, 3.0)
	assert_between(shift, 0.2, 0.45, "about a third of factory windows lit all night")


func test_lights_are_out_soon_after_six_in_the_morning() -> void:
	assert_gt(NightLights.light_amount(3.0), 0.9, "full at night")
	assert_eq(NightLights.light_amount(NightLights.LIGHTS_OUT), 0.0, "out by then")
	assert_eq(NightLights.light_amount(7.0), 0.0)
	assert_eq(_share_lit(Lot.Type.RESIDENTIAL, 6.5), 0.0, "no windows lit")
	assert_gt(NightLights.light_amount(21.5), 0.9, "back in the evening")
