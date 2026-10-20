class_name RoadType
extends Resource
## Static description of a kind of road. One .tres per type in res://data/road_types/,
## so new road types are data, not code. Saves reference a type by its `id`.

const DIRECTORY := "res://data/road_types/"

@export var id: StringName
@export var display_name := ""
@export_range(1, 4) var lanes_per_direction := 1
@export var lane_width := 14.0  ## World pixels.
@export var speed_limit := 90.0  ## World pixels per sim second.
@export var surface_color := Color("4a5562")
@export var edge_color := Color("2b323b")


func get_total_width() -> float:
	return lane_width * lanes_per_direction * 2


static func load_by_id(type_id: StringName) -> RoadType:
	var path := DIRECTORY + String(type_id) + ".tres"
	if not ResourceLoader.exists(path):
		return null
	return load(path) as RoadType
