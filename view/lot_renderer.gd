class_name LotRenderer
extends Node2D
## Draws each lot on its plot (the part of its cell the roads leave free, Lot.plot): the
## building at the back (BuildingPainter: a style per zone, varied per lot), a marked
## parking strip at the front, along the side facing the road it is reached from, with a
## driveway to its access point. Stalls: one per resident's car at homes, the lot's spaces
## at shops and factories (Parking.CAPACITY); the citizens' cars parked there are drawn in
## them on a layer of their own (from the tick snapshot, so they come and go as people
## drive, without redrawing the buildings). Lots without road access are dimmed with a
## red outline.

## Zone colours, for the zone tint and tools (buildings have their own styles).
const COLORS := {
	Lot.Type.RESIDENTIAL: Color("4caf50"),
	Lot.Type.COMMERCIAL: Color("42a5f5"),
	Lot.Type.INDUSTRIAL: Color("f9a825"),
}
const NO_ACCESS_COLOR := Color(1.0, 0.3, 0.25, 0.9)
const NO_ACCESS_DIM := Color(0, 0, 0, 0.55)
const PAVEMENT_COLOR := Color(0.24, 0.25, 0.27)
const STALL_LINE_COLOR := Color(1, 1, 1, 0.35)
## Depth of the parking strip (a car is 11 px long) and the least a building keeps.
const PARKING_DEPTH := 13.0
const MIN_BUILDING := 8.0
## Width of a stall (a car is 6 px wide), and the gap around buildings.
const STALL_WIDTH := 8.0
const BUILDING_MARGIN := 2.0
const DRIVEWAY_WIDTH := 4.0
## Parked cars come and go: redraw them this often (frames).
const REDRAW_FRAMES := 10

## Per lot, what to draw: see layout().
var _layouts: Array[Dictionary] = []
## Lots or roads changed: lay out again (once, next frame: changes come in bursts).
var _dirty := true
## Draws the parked cars, over the lots.
var _parked_layer := Node2D.new()


func _ready() -> void:
	_parked_layer.name = "ParkedCars"
	add_child(_parked_layer)
	_parked_layer.draw.connect(_draw_parked)
	Events.simulation_reset.connect(_mark_dirty)
	Events.lots_changed.connect(_mark_dirty)
	Events.road_network_changed.connect(func(_nodes: Array, _segments: Array) -> void: _mark_dirty())


## Where things go on a plot whose road access (door) is at `door`: { "front" (unit
## vector toward the road), "parking" (Rect2 of the strip), "building" (Rect2),
## "stalls" (Array of Rect2, one per stall that fits) }.
static func layout(plot: Rect2, door: Vector2, stall_count: int) -> Dictionary:
	var to_door := door - plot.get_center()
	var front := Vector2(signf(to_door.x), 0.0) if absf(to_door.x) > absf(to_door.y) else Vector2(0.0, signf(to_door.y))
	if front == Vector2.ZERO:
		front = Vector2.DOWN
	var horizontal := front.x != 0.0
	var depth := plot.size.x if horizontal else plot.size.y
	var strip := minf(PARKING_DEPTH, depth - MIN_BUILDING)
	var parking := Rect2()
	var building := plot
	if strip > 0.0:
		if front == Vector2.UP:
			parking = Rect2(plot.position, Vector2(plot.size.x, strip))
			building = Rect2(plot.position + Vector2(0, strip), plot.size - Vector2(0, strip))
		elif front == Vector2.DOWN:
			parking = Rect2(Vector2(plot.position.x, plot.end.y - strip), Vector2(plot.size.x, strip))
			building = Rect2(plot.position, plot.size - Vector2(0, strip))
		elif front == Vector2.LEFT:
			parking = Rect2(plot.position, Vector2(strip, plot.size.y))
			building = Rect2(plot.position + Vector2(strip, 0), plot.size - Vector2(strip, 0))
		else:
			parking = Rect2(Vector2(plot.end.x - strip, plot.position.y), Vector2(strip, plot.size.y))
			building = Rect2(plot.position, plot.size - Vector2(strip, 0))
	var stalls: Array[Rect2] = []
	if parking.has_area():
		var length := parking.size.y if horizontal else parking.size.x
		var count := mini(stall_count, floori(length / STALL_WIDTH))
		var start := (length - count * STALL_WIDTH) * 0.5
		for i in count:
			var along := start + i * STALL_WIDTH
			if horizontal:
				stalls.append(Rect2(parking.position + Vector2(0, along), Vector2(parking.size.x, STALL_WIDTH)))
			else:
				stalls.append(Rect2(parking.position + Vector2(along, 0), Vector2(STALL_WIDTH, parking.size.y)))
	return { "front": front, "parking": parking, "building": building.grow(-BUILDING_MARGIN), "stalls": stalls }


func _process(_delta: float) -> void:
	if _dirty:
		_dirty = false
		_rebuild()
	elif Engine.get_process_frames() % REDRAW_FRAMES == 0:
		_parked_layer.queue_redraw()


func _mark_dirty() -> void:
	_dirty = true


## Re-lays out every lot (on lot or road changes; reads the simulation, so not per frame).
func _rebuild() -> void:
	_layouts.clear()
	var lots := Game.simulation.lots
	var roads := Game.simulation.roads
	for lot: Lot in lots.lots.values():
		var access := lots.get_access(lot.id)
		var stall_count: int = Parking.CAPACITY.get(lot.type, CitizenSystem.RESIDENTS_PER_HOME)
		var entry := { "id": lot.id, "type": lot.type, "seed": hash(lot.cell), "plot": lot.plot, "door": Vector2.INF }
		if access.is_empty():
			entry.merge(layout(lot.plot, lot.plot.get_center() + Vector2.DOWN, stall_count))
		else:
			entry.door = roads.lanes[access.lane_id].curve.sample_baked(access.distance)
			entry.merge(layout(lot.plot, entry.door, stall_count))
		_layouts.append(entry)
	queue_redraw()
	_parked_layer.queue_redraw()


func _draw() -> void:
	for entry in _layouts:
		var building: Rect2 = entry.building
		if entry.door == Vector2.INF:
			BuildingPainter.paint(self, entry.type, building, entry.front, entry.seed)
			draw_rect(building, NO_ACCESS_DIM)
			draw_rect(building, NO_ACCESS_COLOR, false, 2.0)
			continue
		var parking: Rect2 = entry.parking
		if parking.has_area():
			draw_line(parking.get_center(), entry.door, PAVEMENT_COLOR, DRIVEWAY_WIDTH)
			draw_rect(parking, PAVEMENT_COLOR)
		else:
			draw_line(building.get_center(), entry.door, PAVEMENT_COLOR, DRIVEWAY_WIDTH)
		var stalls: Array[Rect2] = entry.stalls
		var horizontal: bool = entry.front.x != 0.0
		for stall in stalls:
			# Stall dividers run front to back, between neighbours.
			if horizontal:
				draw_line(stall.position, stall.position + Vector2(stall.size.x, 0), STALL_LINE_COLOR, 1.0)
			else:
				draw_line(stall.position, stall.position + Vector2(0, stall.size.y), STALL_LINE_COLOR, 1.0)
		BuildingPainter.paint(self, entry.type, building, entry.front, entry.seed)


func _draw_parked() -> void:
	var parked := Game.view_to.parked
	for entry in _layouts:
		var cars: int = parked.get(entry.id, 0)
		if cars == 0 or entry.door == Vector2.INF:
			continue
		var stalls: Array[Rect2] = entry.stalls
		var horizontal: bool = entry.front.x != 0.0
		var car := Vector2(Vehicle.CAR_LENGTH, Vehicle.CAR_WIDTH) if horizontal else Vector2(Vehicle.CAR_WIDTH, Vehicle.CAR_LENGTH)
		for i in mini(cars, stalls.size()):
			_parked_layer.draw_rect(Rect2(stalls[i].get_center() - car * 0.5, car), VehicleRenderer.CAR_COLOR)
