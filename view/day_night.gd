class_name DayNight
extends CanvasModulate
## Tints the world by time of day: dark blue at night, warm at dawn and dusk, neutral by
## day. The HUD lives on its own CanvasLayer and isn't affected. Toggle from the debug
## menu (overlay id &"day_night").

const OVERLAY_ID := &"day_night"
## (hour, tint) keyframes, interpolated linearly; must start at 0 and end at 24.
const KEYFRAMES := [
	[0.0, Color(0.38, 0.42, 0.62)],
	[5.0, Color(0.38, 0.42, 0.62)],
	[6.5, Color(0.95, 0.78, 0.68)],
	[8.0, Color(1, 1, 1)],
	[17.5, Color(1, 1, 1)],
	[19.5, Color(0.95, 0.72, 0.62)],
	[21.0, Color(0.38, 0.42, 0.62)],
	[24.0, Color(0.38, 0.42, 0.62)],
]


func _ready() -> void:
	Events.debug_overlay_toggle_requested.connect(_on_toggle_requested)


func _process(_delta: float) -> void:
	color = tint_at(Game.simulation.clock.get_time_of_day_hours())


static func tint_at(hour: float) -> Color:
	for i in KEYFRAMES.size() - 1:
		var from: Array = KEYFRAMES[i]
		var to: Array = KEYFRAMES[i + 1]
		if hour <= to[0]:
			return (from[1] as Color).lerp(to[1], inverse_lerp(from[0], to[0], hour))
	return KEYFRAMES[-1][1]


func _on_toggle_requested(overlay_id: StringName) -> void:
	if overlay_id == OVERLAY_ID:
		visible = not visible
		Events.debug_overlay_changed.emit(OVERLAY_ID, visible)
