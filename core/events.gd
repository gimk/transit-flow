extends Node
## Global signal bus (autoload "Events").
## Lets the view and UI react to the simulation without holding references to each other.

@warning_ignore_start("unused_signal")

## A new Simulation instance replaced the previous one (new game / load).
signal simulation_reset
signal speed_changed(speed_index: int, multiplier: int)
signal hour_changed(hour: int)
signal day_started(day: int)
