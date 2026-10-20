class_name DebugMenu
extends PanelContainer
## Right-hand debug panel: every debug function as a button, labelled with its keyboard
## shortcut. Click the header (or press F1) to fold it away. Toggle buttons follow the
## overlays they control, even when those are switched from the keyboard.

const STATS_REFRESH_FRAMES := 15

var _body: VBoxContainer
var _overlay_buttons: Dictionary[StringName, Button] = {}
var _stress_button: Button


func _ready() -> void:
	var padding := MarginContainer.new()
	for side in ["left", "right", "top", "bottom"]:
		padding.add_theme_constant_override("margin_" + side, 8)
	add_child(padding)
	var column := VBoxContainer.new()
	padding.add_child(column)
	var header := Button.new()
	header.text = "Debug  (F1)"
	header.flat = true
	header.focus_mode = Control.FOCUS_NONE
	header.tooltip_text = "Show or hide the debug functions"
	header.pressed.connect(toggle_collapsed)
	column.add_child(header)
	_body = VBoxContainer.new()
	column.add_child(_body)

	_add_section("Views")
	_add_overlay_toggle(LaneOverlay.OVERLAY_ID, "Lane overlay", "F3")
	_add_overlay_toggle(HeatmapOverlay.OVERLAY_ID, "Congestion heatmap", "F4")
	_add_overlay_toggle(DayNight.OVERLAY_ID, "Day / night lighting", "")
	_add_section("Game")
	_add_button("Quicksave", "F5", Game.quick_save)
	_add_button("Quickload", "F9", Game.quick_load)
	_add_button("New empty map", "", Game.new_empty_game)
	_add_section("Traffic")
	_stress_button = _add_button("Stress traffic", "F6", Game.cycle_stress_traffic)
	_add_button("Rush hour now", "", Game.rush_hour_now)
	_add_section("Scenarios")
	_add_button("Bottleneck", "F7", Game.load_bottleneck_scenario)

	Events.debug_overlay_changed.connect(_on_overlay_changed)
	# Lighting starts on.
	_overlay_buttons[DayNight.OVERLAY_ID].set_pressed_no_signal(true)
	_refresh_stress_label()


func toggle_collapsed() -> void:
	_body.visible = not _body.visible


func _process(_delta: float) -> void:
	if _body.visible and Engine.get_process_frames() % STATS_REFRESH_FRAMES == 0:
		_refresh_stress_label()


func _unhandled_input(event: InputEvent) -> void:
	if event.is_action_pressed("toggle_debug_menu"):
		toggle_collapsed()
		get_viewport().set_input_as_handled()


func _add_section(title: String) -> void:
	var label := Label.new()
	label.text = title.to_upper()
	label.modulate.a = 0.55
	label.add_theme_font_size_override("font_size", 11)
	_body.add_child(label)


func _add_button(text: String, shortcut: String, action: Callable) -> Button:
	var button := Button.new()
	button.text = text if shortcut == "" else "%s  (%s)" % [text, shortcut]
	button.alignment = HORIZONTAL_ALIGNMENT_LEFT
	button.focus_mode = Control.FOCUS_NONE
	button.pressed.connect(action)
	_body.add_child(button)
	return button


func _add_overlay_toggle(overlay_id: StringName, text: String, shortcut: String) -> void:
	var button := _add_button(text, shortcut, Events.debug_overlay_toggle_requested.emit.bind(overlay_id))
	button.toggle_mode = true
	_overlay_buttons[overlay_id] = button


func _on_overlay_changed(overlay_id: StringName, shown: bool) -> void:
	if _overlay_buttons.has(overlay_id):
		_overlay_buttons[overlay_id].set_pressed_no_signal(shown)


func _refresh_stress_label() -> void:
	_stress_button.text = "Stress traffic: %d  (F6)" % Game.simulation.stress.target_count
