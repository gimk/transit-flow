class_name ToolPalette
extends VBoxContainer
## Bottom-left build palette: one toolbar button per tool family (Roads, Zones, …). A
## family with variants (road types, zone types) opens a sub-menu of them just above
## the toolbar; the family's button picks its last-used variant. The active tool's hint
## reads above both. Everything follows the active tool, whether it was picked here or
## with a keyboard shortcut.

## Tool families, left to right. A family has either one "tool", "variants_of" a tool
## (its variants come from data, e.g. RoadType.list_all()), or a list of "entries" that
## are separate tools.
const FAMILIES: Array[Dictionary] = [
	{ "label": "Roads", "variants_of": &"RoadTool", "tooltip": "Build roads (R; R again: next road type)" },
	{ "label": "Zones", "tooltip": "Zone land for buildings", "entries": [
		{ "tool": &"ResidentialZoneTool", "label": "Residential", "tooltip": "Zone Residential (H)" },
		{ "tool": &"CommercialZoneTool", "label": "Commercial", "tooltip": "Zone Commercial (C)" },
		{ "tool": &"IndustrialZoneTool", "label": "Industrial", "tooltip": "Zone Industrial (I)" },
		{ "tool": &"DezoneTool", "label": "Dezone", "tooltip": "Remove zoning (X)" },
	] },
	{ "label": "Transit", "tool": &"BusLineTool", "tooltip": "Bus lines (U): click stops in order, then the first one again to open the line" },
	{ "label": "Signals", "tool": &"SignalTool", "tooltip": "Traffic lights (L): click a junction to toggle its lights" },
	{ "label": "Bulldoze", "tool": &"BulldozeTool", "tooltip": "Bulldoze (B)" },
	{ "label": "Route", "tool": &"RouteTool", "tooltip": "Route inspector (T): click two roads to see the quickest route" },
]

## Per family: its entries as { "tool", "variant" (&"" for none), "label", "tooltip" }.
var _entries: Array[Array] = []
## Per family: the toolbar button, its sub-menu row (null without variants), and the
## sub-menu's buttons (one per entry).
var _family_buttons: Array[Button] = []
var _submenus: Array[Control] = []
var _entry_buttons: Array[Array] = []
## Per family: the entry its button picks (the last used one).
var _remembered: Array[int] = []
## Tool id -> its current variant, as last announced by the tool.
var _variants: Dictionary[StringName, StringName] = {}
var _active_tool := &""

@onready var _hint_label: Label = $HintLabel
@onready var _submenu_holder: Control = $SubMenus
@onready var _toolbar: HBoxContainer = $Toolbar/Buttons


func _ready() -> void:
	for family_index in FAMILIES.size():
		var family := FAMILIES[family_index]
		_entries.append(_family_entries(family))
		_remembered.append(0)
		var button := _make_button(family.label, family.tooltip)
		button.pressed.connect(_on_family_pressed.bind(family_index))
		_toolbar.add_child(button)
		_family_buttons.append(button)
		_add_submenu(family_index)
	# Tools start in their first variant (RoadTool: the narrowest road type).
	for entries in _entries:
		for entry: Dictionary in entries:
			if entry.variant != &"" and not _variants.has(entry.tool):
				_variants[entry.tool] = entry.variant
	Events.tool_changed.connect(_on_tool_changed)
	Events.tool_variant_changed.connect(_on_tool_variant_changed)
	Events.tool_hint_changed.connect(_on_tool_hint_changed)
	_on_tool_changed(&"")
	_on_tool_hint_changed("")


## The sub-menu row of a family (null if it has none), for tests and layout.
func get_submenu(family_label: String) -> Control:
	return _submenus[_family_index(family_label)]


func get_family_button(family_label: String) -> Button:
	return _family_buttons[_family_index(family_label)]


func get_entry_button(family_label: String, entry_label: String) -> Button:
	var index := _family_index(family_label)
	for i in _entries[index].size():
		if _entries[index][i].label == entry_label:
			return _entry_buttons[index][i]
	return null


func _family_index(family_label: String) -> int:
	for i in FAMILIES.size():
		if FAMILIES[i].label == family_label:
			return i
	return -1


static func _family_entries(family: Dictionary) -> Array[Dictionary]:
	var entries: Array[Dictionary] = []
	if family.has("tool"):
		entries.append({ "tool": family.tool, "variant": &"", "label": family.label, "tooltip": family.tooltip })
	elif family.has("variants_of"):
		# Only road types for now; other tools with data-driven variants slot in here.
		for road_type in RoadType.list_all():
			var lanes := road_type.lanes_per_direction
			var layout := "%d lane%s, one way (drag the way traffic goes)" if road_type.one_way else "%d lane%s each way"
			entries.append({ "tool": family.variants_of, "variant": road_type.id, "label": road_type.display_name,
				"tooltip": "%s: %s" % [road_type.display_name, layout % [lanes, "" if lanes == 1 else "s"]] })
	else:
		for entry: Dictionary in family.entries:
			entries.append({ "tool": entry.tool, "variant": &"", "label": entry.label, "tooltip": entry.tooltip })
	return entries


func _add_submenu(family_index: int) -> void:
	var buttons: Array[Button] = []
	var family := FAMILIES[family_index]
	if family.has("tool"):
		_submenus.append(null)
		_entry_buttons.append(buttons)
		return
	var panel := PanelContainer.new()
	panel.visible = false
	var row := HBoxContainer.new()
	panel.add_child(row)
	for entry_index in _entries[family_index].size():
		var entry: Dictionary = _entries[family_index][entry_index]
		var button := _make_button(entry.label, entry.tooltip)
		button.pressed.connect(_on_entry_pressed.bind(family_index, entry_index))
		row.add_child(button)
		buttons.append(button)
	_submenu_holder.add_child(panel)
	_submenus.append(panel)
	_entry_buttons.append(buttons)


func _make_button(text: String, tooltip: String) -> Button:
	var button := Button.new()
	button.text = text
	button.tooltip_text = tooltip
	button.toggle_mode = true
	button.focus_mode = Control.FOCUS_NONE
	return button


func _on_family_pressed(family_index: int) -> void:
	if _family_of(_active_tool) == family_index:
		Events.tool_requested.emit(&"")
		return
	var entry: Dictionary = _entries[family_index][_remembered[family_index]]
	Events.tool_variant_requested.emit(entry.tool, entry.variant)


func _on_entry_pressed(family_index: int, entry_index: int) -> void:
	var entry: Dictionary = _entries[family_index][entry_index]
	Events.tool_variant_requested.emit(entry.tool, entry.variant)
	# Already active in that variant: nothing changes, so put the buttons back as they were.
	_refresh()


func _on_tool_changed(tool_id: StringName) -> void:
	_active_tool = tool_id
	_refresh()


func _on_tool_variant_changed(tool_id: StringName, variant: StringName) -> void:
	_variants[tool_id] = variant
	_refresh()


## Family and entry of the active tool (in its current variant), or -1.
func _family_of(tool_id: StringName) -> int:
	return _locate(tool_id).x


func _locate(tool_id: StringName) -> Vector2i:
	if tool_id == &"":
		return Vector2i(-1, -1)
	for family_index in _entries.size():
		for entry_index in _entries[family_index].size():
			var entry: Dictionary = _entries[family_index][entry_index]
			if entry.tool == tool_id and (entry.variant == &"" or entry.variant == _variants.get(tool_id, &"")):
				return Vector2i(family_index, entry_index)
	return Vector2i(-1, -1)


func _refresh() -> void:
	var active := _locate(_active_tool)
	if active.x != -1:
		_remembered[active.x] = active.y
	for family_index in _family_buttons.size():
		var is_open := family_index == active.x
		_family_buttons[family_index].set_pressed_no_signal(is_open)
		var submenu := _submenus[family_index]
		if submenu == null:
			continue
		submenu.visible = is_open
		for entry_index in _entry_buttons[family_index].size():
			_entry_buttons[family_index][entry_index].set_pressed_no_signal(is_open and entry_index == active.y)
	_place_submenu.call_deferred(active.x)


## Lines the open sub-menu up with its family's button.
func _place_submenu(family_index: int) -> void:
	if family_index == -1 or _submenus[family_index] == null:
		_submenu_holder.custom_minimum_size.y = 0.0
		return
	var submenu := _submenus[family_index]
	submenu.size = submenu.get_combined_minimum_size()
	submenu.position = Vector2(_family_buttons[family_index].position.x, 0.0)
	_submenu_holder.custom_minimum_size.y = submenu.size.y


func _on_tool_hint_changed(text: String) -> void:
	_hint_label.text = text
	_hint_label.visible = text != ""
