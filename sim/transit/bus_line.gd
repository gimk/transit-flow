class_name BusLine
extends RefCounted
## A bus line: its buses go round its stops in order, from the last back to the first.
## TransitSystem keeps the timings riders plan with: the free-flow drive between stops,
## and the time buses actually take, measured as they run.

var id: int
var stop_ids: Array[int] = []
var color := Color.WHITE
## How many buses run the line.
var bus_count := 1
## Per stop i, from stop i to the next: the drive on empty roads, and the time from arriving
## at stop i to arriving at the next one as buses really do it (a moving average, dwell
## included). INF while the next stop can't be reached.
var free_flow_times := PackedFloat32Array()
var segment_times := PackedFloat32Array()
## The buses' way round, for drawing.
var path := PackedVector2Array()


func _init(p_id: int, p_stop_ids: Array[int]) -> void:
	id = p_id
	stop_ids = p_stop_ids


## Time for a bus to go once round the line (INF if it can't).
func get_loop_time() -> float:
	var total := 0.0
	for time in segment_times:
		total += time
	return total


## Expected time between two buses at a stop.
func get_headway() -> float:
	return get_loop_time() / maxi(bus_count, 1)


## Index of the stop after index `i`.
func next_index(i: int) -> int:
	return (i + 1) % stop_ids.size()
