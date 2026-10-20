class_name Lane
extends RefCounted
## One directed path a vehicle can follow, from `from_node_id` to `to_node_id`.
## Road lanes and the connector lanes that turn through a junction are both Lanes,
## so vehicles only ever need to know how to follow a Curve2D.

enum Turn { NONE, STRAIGHT, RIGHT, LEFT, U_TURN }

var id: int
## Owning segment, or -1 for a connector inside a junction.
var segment_id: int
## For connectors both are the junction node.
var from_node_id: int
var to_node_id: int
## 0 = the lane next to the road's center line, increasing toward the curb.
var index_from_center: int
var curve: Curve2D
var length: float
var speed_limit: float
## The movement a connector performs; NONE for road lanes.
var turn := Turn.NONE
## Lanes a vehicle may continue onto from this lane's end.
var outgoing_lane_ids: Array[int] = []
## Connectors only: the road lane feeding this connector.
var feeder_lane_id := -1
## Connectors only: other connectors at the same junction that cross or merge with
## this one, so must not be used at the same time.
var conflicting_lane_ids: Array[int] = []
## Connectors only: for each conflicting connector id, the stretch of this connector
## (Vector2(from, to), distances along it) that comes too close to that one.
var conflict_zones: Dictionary[int, Vector2] = {}
## Road lanes only: which traffic-light phase lets this lane into the junction at its end.
var signal_phase := 0

## Same as outgoing_lane_ids, as objects (hot path for vehicles).
var outgoing_lanes: Array[Lane] = []
## Runtime, owned by VehicleSystem: vehicles on this lane, front-most first.
var queue: Array = []

## Straight lanes (two points, no handles) are sampled with plain math, which is much
## cheaper than Curve2D sampling for the many vehicles on them.
var is_straight := false
var _start := Vector2.ZERO
var _direction := Vector2.ZERO
var _angle := 0.0


func _init(p_id: int, p_segment_id: int, p_from_node_id: int, p_to_node_id: int,
		p_index_from_center: int, p_curve: Curve2D, p_speed_limit: float) -> void:
	id = p_id
	segment_id = p_segment_id
	from_node_id = p_from_node_id
	to_node_id = p_to_node_id
	index_from_center = p_index_from_center
	speed_limit = p_speed_limit
	set_curve(p_curve)


func set_curve(p_curve: Curve2D) -> void:
	curve = p_curve
	length = p_curve.get_baked_length()
	is_straight = p_curve.point_count == 2 and not LaneBuilder.has_handles(p_curve)
	if is_straight:
		_start = p_curve.get_point_position(0)
		var delta := p_curve.get_point_position(1) - _start
		length = delta.length()
		_direction = delta / length if length > 0.0 else Vector2.RIGHT
		_angle = _direction.angle()


## Position and heading at `distance` along the lane.
func sample_pose(distance: float) -> Transform2D:
	if is_straight:
		return Transform2D(_angle, _start + _direction * clampf(distance, 0.0, length))
	return curve.sample_baked_with_rotation(clampf(distance, 0.0, length))


func is_connector() -> bool:
	return segment_id == -1


## True for another road lane of the same road going the same way (a lane a vehicle can
## change into, possibly several lanes over).
func is_beside(other: Lane) -> bool:
	return other != self and segment_id != -1 and other.segment_id == segment_id and other.from_node_id == from_node_id


## Free-flow time to drive the whole lane, in sim seconds.
func get_travel_time() -> float:
	return length / speed_limit
