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
## Relayed from LotSystem.changed.
signal lots_changed
## Relayed from ZoneSystem.changed.
signal zones_changed
## Relayed from ExternalConnections.changed.
signal externals_changed

## UI asks for a tool by id ("" = no tool); ToolManager answers with tool_changed.
signal tool_requested(tool_id: StringName)
signal tool_changed(tool_id: StringName)
## UI asks for a tool in a given variant (e.g. RoadTool + a road type id; "" = the tool
## as it is). Unlike tool_requested it never toggles the tool off.
signal tool_variant_requested(tool_id: StringName, variant: StringName)
## A tool switched variant (from the UI or its shortcut).
signal tool_variant_changed(tool_id: StringName, variant: StringName)
## Contextual help or error text for the active tool ("" hides it).
signal tool_hint_changed(text: String)

## Debug views (overlay ids: &"lanes", &"heatmap"). UI asks with *_toggle_requested;
## the overlay answers with *_changed so buttons and shortcuts stay in sync.
signal debug_overlay_toggle_requested(overlay_id: StringName)
signal debug_overlay_changed(overlay_id: StringName, shown: bool)

## Ask the map camera to center on a world position.
signal camera_focus_requested(world_position: Vector2)

## Short message shown briefly to the player (e.g. "Game saved").
signal notification_posted(text: String)
