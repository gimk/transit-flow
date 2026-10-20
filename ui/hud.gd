extends CanvasLayer
## Top bar (day, clock, traffic period, speed controls), bottom toolbar,
## tool hint line and short-lived notifications.

const NOTIFICATION_SECONDS := 2.0
## Flow thresholds for the HUD colour (80% is the level-up goal in the design).
const FLOW_GOOD := 80.0
const FLOW_POOR := 60.0

@onready var _time_label: Label = %TimeLabel
@onready var _period_label: Label = %PeriodLabel
@onready var _flow_label: Label = %FlowLabel
@onready var _city_label: Label = %CityLabel
@onready var _stats_label: Label = %StatsLabel
@onready var _speed_buttons: Array[Button] = [%PauseButton, %Speed1Button, %Speed2Button, %Speed3Button]
## Tool id (= ToolManager child node name) -> its toolbar button.
@onready var _tool_buttons: Dictionary[StringName, Button] = {
	&"RoadTool": %RoadButton,
	&"BulldozeTool": %BulldozeButton,
	&"ResidentialZoneTool": %ResidentialButton,
	&"CommercialZoneTool": %CommercialButton,
	&"IndustrialZoneTool": %IndustrialButton,
	&"DezoneTool": %DezoneButton,
	&"SignalTool": %SignalButton,
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
	# Counting stuck vehicles walks every vehicle, so refresh a few times a second only.
	if Engine.get_process_frames() % 15 == 0:
		var sim := Game.simulation
		_stats_label.text = "%d vehicles · %d stuck · tick %.1f ms" % [
			sim.vehicles.vehicles.size(), sim.vehicles.get_stuck_count(), Game.average_tick_ms]
		_city_label.text = "Pop %d · %d/%d jobs filled" % [
			sim.citizens.citizens.size(), sim.citizens.get_employed_count(), sim.citizens.get_job_count()]
		_update_flow(sim.flow.get_flow_percent(sim.get_sim_time()))


## Flow %, coloured: green when traffic flows, amber when it struggles, red when jammed.
func _update_flow(percent: float) -> void:
	if percent < 0.0:
		_flow_label.text = "Flow —"
		_flow_label.modulate = Color.WHITE
		return
	_flow_label.text = "Flow %d%%" % roundi(percent)
	if percent >= FLOW_GOOD:
		_flow_label.modulate = Color("66bb6a")
	elif percent >= FLOW_POOR:
		_flow_label.modulate = Color("ffca28")
	else:
		_flow_label.modulate = Color("ef5350")


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
