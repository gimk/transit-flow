class_name RoadType
extends Resource
## Static description of a kind of road. One .tres per type in res://data/road_types/,
## so new road types are data, not code. Saves reference a type by its `id`.

const DIRECTORY := "res://data/road_types/"

@export var id: StringName
@export var display_name := ""
## Position in the build palette (lowest first).
@export var palette_order := 0
## Lanes each way; on a one-way road, its number of lanes.
@export_range(1, 4) var lanes_per_direction := 1
## One-way roads only have lanes running from the segment's start node to its end node
## (the direction the player dragged), centered on the road.
@export var one_way := false
@export var lane_width := 14.0  ## World pixels.
@export var speed_limit := 90.0  ## World pixels per sim second.
@export var surface_color := Color("4a5562")
@export var edge_color := Color("2b323b")


func get_total_width() -> float:
	return lane_width * get_lane_count()


## Lanes across the whole road, both directions.
func get_lane_count() -> int:
	return lanes_per_direction if one_way else lanes_per_direction * 2


## Every road type in DIRECTORY, in palette order.
static func list_all() -> Array[RoadType]:
	var types: Array[RoadType] = []
	for file in ResourceLoader.list_directory(DIRECTORY):
		if file.ends_with(".tres"):
			var road_type := load(DIRECTORY + file) as RoadType
			if road_type != null:
				types.append(road_type)
	types.sort_custom(func(a: RoadType, b: RoadType) -> bool:
		return a.palette_order < b.palette_order if a.palette_order != b.palette_order else String(a.id) < String(b.id))
	return types


static func load_by_id(type_id: StringName) -> RoadType:
	var path := DIRECTORY + String(type_id) + ".tres"
	if not ResourceLoader.exists(path):
		return null
	return load(path) as RoadType
