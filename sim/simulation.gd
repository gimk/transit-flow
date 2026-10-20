class_name Simulation
extends RefCounted
## Root of the simulation state. Pure data + logic: no Node dependencies,
## so it can be stepped headless from unit tests.
## Systems (road network, vehicles, zones …) will be owned here and stepped in step().

const TICK_RATE := 20
const TICK_DT := 1.0 / TICK_RATE

var clock := SimClock.new()
var tick_count := 0


func step() -> void:
	tick_count += 1
	clock.advance(TICK_DT)


## Simulated seconds since the start of the game.
func get_sim_time() -> float:
	return tick_count * TICK_DT
