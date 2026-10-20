extends Node
## Global signal bus (autoload "Events").
## Lets the view and UI react to the simulation without holding references to each other.

@warning_ignore_start("unused_signal")

## A new Simulation instance replaced the previous one (new game / load).
signal simulation_reset
signal speed_changed(speed_index: int, multiplier: int)
signal hour_changed(hour: int)
signal day_started(day: int)

## Relayed from RoadNetwork.changed. Ids may refer to things that were just removed.
signal road_network_changed(node_ids: Array, segment_ids: Array)

## UI asks for a tool by id ("" = no tool); ToolManager answers with tool_changed.
signal tool_requested(tool_id: StringName)
signal tool_changed(tool_id: StringName)
## Contextual help or error text for the active tool ("" hides it).
signal tool_hint_changed(text: String)

## Short message shown briefly to the player (e.g. "Game saved").
signal notification_posted(text: String)
