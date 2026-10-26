class_name DotDisplay
extends CanvasLayer
## Shows the world as an amber LED dot matrix (dot_display.gdshader), the game's look
## (Phase D). Every renderer under World draws into one of four display layers by what
## it shows, not how it looks: the map (ground, roads, buildings), actors (vehicles,
## people), light (lamps, windows, beams, lit lamps) and information (transit, signals,
## overlays, tool previews). Each layer is a SubViewport sharing the world's World2D and
## keeping only its renderers (CanvasItem.visibility_layer against canvas_cull_mask);
## the composite reads all four and turns them into dots on a grid fixed to the world,
## with fixed priorities (information, light, actors, map) and the lighting done there
## (the world is drawn unlit: DayNight leaves it untinted while this is on).
##
## The grid's cell is a power of two in world px (1 to 16), picked by the zoom so dots
## stay 3-4 screen px apart (MapCamera zooms in matching steps). Under the HUD (layer
## 10). Toggle from the debug menu (overlay id &"dot_display"): off shows the old look.

const OVERLAY_ID := &"dot_display"
## Above the world, under the HUD.
const LAYER := 5
## Display layers, as visibility_layer bits (bit 1 is everything else: the UI).
const MAP := 1 << 1
const ACTORS := 1 << 2
const LIGHT := 1 << 3
const INFO := 1 << 4
const ALL_LAYERS := 0xFFFFF
## Which display layer each renderer under World draws into (with all its children).
const ROUTES: Dictionary[NodePath, int] = {
	^"GridRenderer": MAP,
	^"ZoneRenderer": MAP,
	^"NatureRenderer": MAP,
	^"RoadRenderer": MAP,
	^"ExternalRenderer": MAP,
	^"LotRenderer": MAP,
	^"VehicleRenderer": ACTORS,
	^"PedestrianRenderer": ACTORS,
	^"VehicleLights": LIGHT,
	^"NightLights": LIGHT,
	^"IntersectionRenderer": INFO,
	^"HeatmapOverlay": INFO,
	^"TransitRenderer": INFO,
	^"LaneOverlay": INFO,
	^"Tools": INFO,
}
## Screen px between dots (before UI scaling): the grid's cell is the largest power of
## two in world px that keeps the gap at most this.
const MAX_PITCH := 4.0
const MAX_CELL := 16.0

var _viewports: Dictionary[int, SubViewport] = {}
var _screen := ColorRect.new()
var _material := ShaderMaterial.new()
var _day_night := true


func _ready() -> void:
	layer = LAYER
	# After the camera has moved this frame, so the layers follow it without lag.
	process_priority = 100
	var world_2d := get_viewport().world_2d
	for bits: int in [MAP, ACTORS, LIGHT, INFO]:
		var viewport := SubViewport.new()
		viewport.name = "Layer%d" % bits
		viewport.world_2d = world_2d
		viewport.canvas_cull_mask = bits
		viewport.transparent_bg = true
		viewport.disable_3d = true
		viewport.gui_disable_input = true
		add_child(viewport)
		_viewports[bits] = viewport
	_material.shader = preload("res://view/dot_display.gdshader")
	_material.set_shader_parameter("map_layer", _viewports[MAP].get_texture())
	_material.set_shader_parameter("actor_layer", _viewports[ACTORS].get_texture())
	_material.set_shader_parameter("light_layer", _viewports[LIGHT].get_texture())
	_material.set_shader_parameter("info_layer", _viewports[INFO].get_texture())
	_screen.material = _material
	_screen.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_screen.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(_screen)
	Events.debug_overlay_toggle_requested.connect(_on_toggle_requested)
	Events.debug_overlay_changed.connect(_on_overlay_changed)
	# VehicleRenderer adds its light layer as a sibling once World is ready.
	_route_world.call_deferred()
	_apply_visibility()


## Puts every renderer under World into its display layer. A CanvasItem shows in a
## viewport only if it and all its parents share a bit with the viewport's mask, so the
## parents get every bit.
func _route_world() -> void:
	var world := get_node_or_null(^"../World") as CanvasItem
	if world == null:
		return
	(get_parent() as CanvasItem).visibility_layer = ALL_LAYERS
	world.visibility_layer = ALL_LAYERS
	for child in world.get_children():
		if child is CanvasItem and not child is CanvasModulate and not ROUTES.has(NodePath(child.name)):
			push_warning("DotDisplay: %s has no display layer; it won't show under the dot display" % child.name)
	for path: NodePath in ROUTES:
		var item := world.get_node_or_null(path) as CanvasItem
		if item != null:
			_route(item, ROUTES[path])


func _route(item: CanvasItem, bits: int) -> void:
	item.visibility_layer = bits
	if not item.child_entered_tree.is_connected(_on_child_entered):
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
	_material.set_shader_parameter("cell", cell_size(1.0 / world_per_px / ui_scale))
	_material.set_shader_parameter("night", DayNight.night_amount(Game.view_to.hours) if _day_night else 0.0)


## The grid's cell (world px) at camera `zoom`: the largest power of two that keeps dots
## at most MAX_PITCH screen px apart.
static func cell_size(zoom: float) -> float:
	return clampf(pow(2.0, floorf(log(MAX_PITCH / zoom) / log(2.0) + 0.001)), 1.0, MAX_CELL)


func _on_toggle_requested(overlay_id: StringName) -> void:
	if overlay_id == OVERLAY_ID:
		visible = not visible
		_apply_visibility()
		Events.debug_overlay_changed.emit(OVERLAY_ID, visible)


## On, the main viewport draws only the UI and the layers render; off, the main viewport
## draws everything again (the old look) and the layers stop.
func _apply_visibility() -> void:
	get_viewport().canvas_cull_mask = 1 if visible else ALL_LAYERS
	for layer_viewport: SubViewport in _viewports.values():
		layer_viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS if visible else SubViewport.UPDATE_DISABLED


func _on_overlay_changed(overlay_id: StringName, shown: bool) -> void:
	if overlay_id == DayNight.OVERLAY_ID:
		_day_night = shown
