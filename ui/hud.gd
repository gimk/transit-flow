extends CanvasLayer
## Top bar (day, clock, traffic period, Flow, speed controls) and short-lived
## notifications. The build palette (bottom left) is ToolPalette.

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
@onready var _notification_label: Label = %NotificationLabel

var _notification_tween: Tween


func _ready() -> void:
	for i in _speed_buttons.size():
		_speed_buttons[i].pressed.connect(Game.set_speed.bind(i))
	Events.speed_changed.connect(_on_speed_changed)
	Events.notification_posted.connect(_on_notification_posted)
	_on_speed_changed(Game.speed_index, Game.SPEED_MULTIPLIERS[Game.speed_index])
	_notification_label.modulate.a = 0.0


func _process(_delta: float) -> void:
	# From the latest tick snapshot (see Game), not the live simulation.
	var view := Game.view_to
	_time_label.text = "Day %d · %s" % [view.day, view.time_text]
	_period_label.text = view.period_name
	if Engine.get_process_frames() % 15 == 0:
		_stats_label.text = "%d vehicles · %d stuck · tick %.1f ms%s" % [
			view.vehicle_count, view.stuck_count, Game.average_tick_ms,
			" · sim at %.1f× (can't keep up)" % Game.effective_speed if Game.sim_lagging else ""]
		_city_label.text = "Pop %d · %d/%d jobs filled" % [view.population, view.employed, view.jobs]
		_update_flow(view.flow_percent)


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


func _on_notification_posted(text: String) -> void:
	_notification_label.text = text
	if _notification_tween != null:
		_notification_tween.kill()
	_notification_label.modulate.a = 1.0
	_notification_tween = create_tween()
	_notification_tween.tween_interval(NOTIFICATION_SECONDS)
	_notification_tween.tween_property(_notification_label, "modulate:a", 0.0, 0.4)
