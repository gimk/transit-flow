class_name DotDisplay
extends CanvasLayer
## Shows the world as an amber LED dot matrix (dot_display.gdshader), the game's look
## (Phase D). Every renderer under World draws into a display layer by what it shows,
## not how it looks: the map (ground, roads, buildings), actors (vehicles, people), light
## (lamps, windows, beams, lit lamps) and information, by how it is marked: solid dots
## over everything (transit lines, signals, overlays, tool previews), square dots on the
## ground (bus lanes, stop signs) and hollow rings on the ground (zones). Each layer is a
## SubViewport sharing the world's World2D and keeping only its renderers
## (CanvasItem.visibility_layer against canvas_cull_mask); the composite reads them all
## and turns them into dots on a grid fixed to the world, with fixed priorities and the
## lighting done there (the world is drawn unlit: DayNight leaves it untinted while this
## is on).
##
## The grid's cell is a power of two in world px (1 to 16), picked by the zoom so dots
## stay 3-4 screen px apart (MapCamera zooms in matching steps); information renderers
## read it (current_cell) to keep their marks at least a cell wide. The zoom also sets a
## level of detail (Band, current_band, Events.dot_band_changed): close up renderers
## draw objects, far out the system. Problems (jams, stuck vehicles, crowded stops,
## zones waiting) are information too, blinking (AlertRenderer). The composite reads at
## most six layers: a seventh texture was not reliably bound (D3D12). Under the HUD (layer
## 10). Toggle from the debug menu (overlay id &"dot_display"): off shows the old look.

## Levels of detail, from close up to far out (see the plan, Phase D-LOD).
enum Band { DETAIL, STREET, DISTRICT, CITY }

const OVERLAY_ID := &"dot_display"
## Above the world, under the HUD.
const LAYER := 5
## Display layers, as visibility_layer bits (bit 1 is everything else: the UI).
const MAP := 1 << 1
const ACTORS := 1 << 2
const LIGHT := 1 << 3
const INFO := 1 << 4
const SQUARE_MARKS := 1 << 5
const RING_MARKS := 1 << 6
const ALL_LAYERS := 0xFFFFF
## The lowest camera zoom of each band (DETAIL, STREET, DISTRICT; below: CITY).
const BAND_ZOOMS: Array[float] = [2.0, 1.0, 0.5]
## Which display layer each renderer under World draws into (with all its children). A
## deeper path overrides its parent's layer for that branch.
const ROUTES: Dictionary[NodePath, int] = {
	^"GridRenderer": MAP,
	^"ZoneRenderer": RING_MARKS,
	^"NatureRenderer": MAP,
	^"RoadRenderer": MAP,
	^"RoadRenderer/BusLanes": SQUARE_MARKS,
	^"ExternalRenderer": MAP,
	^"LotRenderer": MAP,
	^"VehicleRenderer": ACTORS,
	^"PedestrianRenderer": ACTORS,
	^"VehicleLights": LIGHT,
	^"NightLights": LIGHT,
	^"IntersectionRenderer": INFO,
	^"HeatmapOverlay": INFO,
	^"TransitRenderer": MAP,
	^"TransitRenderer/Lines": INFO,
	^"TransitRenderer/Stops": SQUARE_MARKS,
	^"LaneOverlay": INFO,
	^"Tools": INFO,
	^"AlertRenderer": INFO,
	^"AlertRenderer/Marks": SQUARE_MARKS,
}
## Screen px between dots (before UI scaling): the grid's cell is the largest power of
## two in world px that keeps the gap at most this.
const MAX_PITCH := 4.0
const MAX_CELL := 16.0
## The layers' texture names in the composite.
const LAYER_UNIFORMS: Dictionary[int, String] = {
	MAP: "map_layer", ACTORS: "actor_layer", LIGHT: "light_layer",
	INFO: "info_layer", SQUARE_MARKS: "square_layer", RING_MARKS: "ring_layer",
}

## The grid's cell (world px) right now, or 0 when the dot display is off.
static var current_cell := 0.0
## The level of detail right now (DETAIL when the dot display is off).
static var current_band := Band.DETAIL

var _viewports: Dictionary[int, SubViewport] = {}
var _screen := ColorRect.new()
var _material := ShaderMaterial.new()
var _day_night := true


func _ready() -> void:
	layer = LAYER
	# After the camera has moved this frame, so the layers follow it without lag.
	process_priority = 100
	var world_2d := get_viewport().world_2d
	_material.shader = preload("res://view/dot_display.gdshader")
	for bits: int in LAYER_UNIFORMS:
		var viewport := SubViewport.new()
		viewport.name = "Layer%d" % bits
		viewport.world_2d = world_2d
		viewport.canvas_cull_mask = bits
		viewport.transparent_bg = true
		viewport.disable_3d = true
		viewport.gui_disable_input = true
		add_child(viewport)
		_viewports[bits] = viewport
		_material.set_shader_parameter(LAYER_UNIFORMS[bits], viewport.get_texture())
	_screen.material = _material
	_screen.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_screen.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(_screen)
	Events.debug_overlay_toggle_requested.connect(_on_toggle_requested)
	Events.debug_overlay_changed.connect(_on_overlay_changed)
	# VehicleRenderer adds its light layer as a sibling once World is ready.
	_route_world.call_deferred()
	_apply_visibility()


## Puts every renderer under World into its display layer, parents before the deeper
## paths that override them. A CanvasItem shows in a viewport only if it and all its
## parents share a bit with the viewport's mask, so parents get their children's bits
## too (the renderers that have routed children draw nothing themselves).
func _route_world() -> void:
	var world := get_node_or_null(^"../World") as CanvasItem
	if world == null:
		return
	(get_parent() as CanvasItem).visibility_layer = ALL_LAYERS
	world.visibility_layer = ALL_LAYERS
	for child in world.get_children():
		if child is CanvasItem and not child is CanvasModulate and not ROUTES.has(NodePath(child.name)):
			push_warning("DotDisplay: %s has no display layer; it won't show under the dot display" % child.name)
	var paths := ROUTES.keys()
	paths.sort_custom(func(a: NodePath, b: NodePath) -> bool: return a.get_name_count() < b.get_name_count())
	for path: NodePath in paths:
		var item := world.get_node_or_null(path) as CanvasItem
		if item == null:
			continue
		_route(item, ROUTES[path])
		var parent := item.get_parent() as CanvasItem
		while parent != world:
			parent.visibility_layer |= ROUTES[path]
			parent = parent.get_parent() as CanvasItem


func _route(item: CanvasItem, bits: int) -> void:
	item.visibility_layer = bits
	if item.child_entered_tree.is_connected(_on_child_entered):
		item.child_entered_tree.disconnect(_on_child_entered)
	item.child_entered_tree.connect(_on_child_entered.bind(bits))
	for child in item.get_children():
		if child is CanvasItem:
			_route(child, bits)


## Renderers add children as the map changes (road pieces, building regions).
func _on_child_entered(child: Node, bits: int) -> void:
	if child is CanvasItem:
		_route(child, bits)


func _process(_delta: float) -> void:
	if not visible:
		return
	var viewport := get_viewport()
	var to_screen := viewport.get_final_transform() * viewport.canvas_transform
	var size := Vector2i((viewport.get_visible_rect().size * viewport.get_final_transform().get_scale()).round())
	for layer_viewport: SubViewport in _viewports.values():
		layer_viewport.size = size
		layer_viewport.canvas_transform = to_screen
	var to_world := to_screen.affine_inverse()
	var world_per_px := to_world.basis_xform(Vector2.RIGHT).length()
	var ui_scale := viewport.get_final_transform().get_scale().x
	_material.set_shader_parameter("world_origin", to_world * Vector2.ZERO)
	_material.set_shader_parameter("world_per_px", world_per_px)
	var zoom := 1.0 / world_per_px / ui_scale
	current_cell = cell_size(zoom)
	_material.set_shader_parameter("cell", current_cell)
	_material.set_shader_parameter("night", DayNight.night_amount(Game.view_to.hours) if _day_night else 0.0)
	_set_band(band_for(zoom))


## The grid's cell (world px) at camera `zoom`: the largest power of two that keeps dots
## at most MAX_PITCH screen px apart.
static func cell_size(zoom: float) -> float:
	return clampf(pow(2.0, floorf(log(MAX_PITCH / zoom) / log(2.0) + 0.001)), 1.0, MAX_CELL)


## The level of detail at camera `zoom`.
static func band_for(zoom: float) -> Band:
	for band in BAND_ZOOMS.size():
		if zoom >= BAND_ZOOMS[band] - 0.001:
			return band as Band
	return Band.CITY


func _set_band(band: Band) -> void:
	if band != current_band:
		current_band = band
		Events.dot_band_changed.emit(band)


func _on_toggle_requested(overlay_id: StringName) -> void:
	if overlay_id == OVERLAY_ID:
		visible = not visible
		_apply_visibility()
		Events.debug_overlay_changed.emit(OVERLAY_ID, visible)


## On, the main viewport draws only the UI and the layers render; off, the main viewport
## draws everything again (the old look) and the layers stop.
func _apply_visibility() -> void:
	get_viewport().canvas_cull_mask = 1 if visible else ALL_LAYERS
	if not visible:
		current_cell = 0.0
		_set_band(Band.DETAIL)
	for layer_viewport: SubViewport in _viewports.values():
		layer_viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS if visible else SubViewport.UPDATE_DISABLED


func _on_overlay_changed(overlay_id: StringName, shown: bool) -> void:
	if overlay_id == DayNight.OVERLAY_ID:
		_day_night = shown


func _exit_tree() -> void:
	current_cell = 0.0
	current_band = Band.DETAIL
