extends CanvasLayer
## Top bar: day, clock, current traffic period and sim speed controls.

@onready var _time_label: Label = %TimeLabel
@onready var _period_label: Label = %PeriodLabel
@onready var _speed_buttons: Array[Button] = [%PauseButton, %Speed1Button, %Speed2Button, %Speed3Button]


func _ready() -> void:
	for i in _speed_buttons.size():
		_speed_buttons[i].pressed.connect(Game.set_speed.bind(i))
	Events.speed_changed.connect(_on_speed_changed)
	_on_speed_changed(Game.speed_index, Game.SPEED_MULTIPLIERS[Game.speed_index])


func _process(_delta: float) -> void:
	var clock := Game.simulation.clock
	_time_label.text = "Day %d · %s" % [clock.get_day(), clock.format_time()]
	_period_label.text = clock.get_period_name()


func _on_speed_changed(speed_index: int, _multiplier: int) -> void:
	for i in _speed_buttons.size():
		_speed_buttons[i].set_pressed_no_signal(i == speed_index)
