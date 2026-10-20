extends CanvasLayer
## Top bar (day, clock, traffic period, speed controls), bottom toolbar,
## tool hint line and short-lived notifications.

const NOTIFICATION_SECONDS := 2.0

@onready var _time_label: Label = %TimeLabel
@onready var _period_label: Label = %PeriodLabel
@onready var _speed_buttons: Array[Button] = [%PauseButton, %Speed1Button, %Speed2Button, %Speed3Button]
## Tool id (= ToolManager child node name) -> its toolbar button.
@onready var _tool_buttons: Dictionary[StringName, Button] = {
	&"RoadTool": %RoadButton,
	&"BulldozeTool": %BulldozeButton,
	&"RouteTool": %RouteButton,
}
@onready var _hint_label: Label = %HintLabel
@onready var _notification_label: Label = %NotificationLabel

var _notification_tween: Tween


func _ready() -> void:
	for i in _speed_buttons.size():
		_speed_buttons[i].pressed.connect(Game.set_speed.bind(i))
	for tool_id in _tool_buttons:
		_tool_buttons[tool_id].pressed.connect(Events.tool_requested.emit.bind(tool_id))
	Events.speed_changed.connect(_on_speed_changed)
	Events.tool_changed.connect(_on_tool_changed)
	Events.tool_hint_changed.connect(_on_tool_hint_changed)
	Events.notification_posted.connect(_on_notification_posted)
	_on_speed_changed(Game.speed_index, Game.SPEED_MULTIPLIERS[Game.speed_index])
	_on_tool_changed(&"")
	_on_tool_hint_changed("")
	_notification_label.modulate.a = 0.0


func _process(_delta: float) -> void:
	var clock := Game.simulation.clock
	_time_label.text = "Day %d · %s" % [clock.get_day(), clock.format_time()]
	_period_label.text = clock.get_period_name()


func _on_speed_changed(speed_index: int, _multiplier: int) -> void:
	for i in _speed_buttons.size():
		_speed_buttons[i].set_pressed_no_signal(i == speed_index)


func _on_tool_changed(tool_id: StringName) -> void:
	for id in _tool_buttons:
		_tool_buttons[id].set_pressed_no_signal(id == tool_id)


func _on_tool_hint_changed(text: String) -> void:
	_hint_label.text = text
	_hint_label.visible = text != ""


func _on_notification_posted(text: String) -> void:
	_notification_label.text = text
	if _notification_tween != null:
		_notification_tween.kill()
	_notification_label.modulate.a = 1.0
	_notification_tween = create_tween()
	_notification_tween.tween_interval(NOTIFICATION_SECONDS)
	_notification_tween.tween_property(_notification_label, "modulate:a", 0.0, 0.4)
