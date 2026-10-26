class_name DayNight
extends CanvasModulate
## Tints the world by time of day: dark blue at night, warm at dawn and dusk, neutral by
## day. The HUD lives on its own CanvasLayer and isn't affected. Toggle from the debug
## menu (overlay id &"day_night"). Under the dot display the world is drawn unlit (no
## tint): DotDisplay does the lighting itself.

const OVERLAY_ID := &"day_night"
## (hour, tint) keyframes, interpolated linearly; must start at 0 and end at 24.
const KEYFRAMES := [
	[0.0, Color(0.48, 0.52, 0.72)],
	[5.0, Color(0.48, 0.52, 0.72)],
	[6.5, Color(0.95, 0.78, 0.68)],
	[8.0, Color(1, 1, 1)],
	[17.5, Color(1, 1, 1)],
	[19.5, Color(0.95, 0.72, 0.62)],
	[21.0, Color(0.48, 0.52, 0.72)],
	[24.0, Color(0.48, 0.52, 0.72)],
]

## The tint the world is drawn with right now (white when off or under the dot display):
## lights divide by it so they keep their own colour and strength.
static var applied_tint := Color.WHITE

## The dot display is on (it lights the world itself).
var _dot_display := true


func _ready() -> void:
	Events.debug_overlay_toggle_requested.connect(_on_toggle_requested)
	Events.debug_overlay_changed.connect(_on_overlay_changed)


func _process(_delta: float) -> void:
	color = Color.WHITE if _dot_display else tint_at(Game.view_to.hours)
	applied_tint = color if visible else Color.WHITE


func _exit_tree() -> void:
	applied_tint = Color.WHITE


func _on_overlay_changed(overlay_id: StringName, shown: bool) -> void:
	if overlay_id == DotDisplay.OVERLAY_ID:
		_dot_display = shown


## How dark it is (0 day, 1 night), from the tint: drives headlights and the light layer
## (NightLights).
static func night_amount(hour: float) -> float:
	var night_tint: Color = KEYFRAMES[0][1]
	return clampf((1.0 - tint_at(hour).get_luminance()) / (1.0 - night_tint.get_luminance()), 0.0, 1.0)


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
