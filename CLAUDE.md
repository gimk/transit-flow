# Transit Flow

2D top-down traffic & transit city builder in **Godot 4.7 / GDScript**. The player zones R/C/I and designs roads and transit; the city builds itself; the score is "Flow Efficiency %".

- Game design: [game_design_godot_architecture_concept.md](game_design_godot_architecture_concept.md)
- Architecture, roadmap, phase status: [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) — read it before starting a phase, and update its **Status** line and phase details when a phase is done or the design changes.

## Git rules

- **Never add Claude as co-author.** No `Co-Authored-By: Claude …` trailer, no "Generated with Claude Code" line, in commits or PR descriptions. (Also enforced by `.claude/settings.json` → `attribution`.)
- Commit identity is set in the repo's local config: `gimk <a.pouligny@gmail.com>`. Don't change it.
- Commit `.uid` files alongside their scripts/scenes. `.godot/` is ignored.
- Run the tests before committing.

## Commands

Godot is not on PATH. Console build:
`C:/Users/AntoinePOULIGNY/Downloads/Godot_v4.7.2-stable_win64.exe/Godot_v4.7.2-stable_win64_console.exe` (below: `$GODOT`).

```bash
"$GODOT" --headless --import                         # re-scan project; required after adding/renaming a class_name
"$GODOT" --headless -s addons/gut/gut_cmdln.gd       # run all tests (config: .gutconfig.json)
"$GODOT" --path . --write-movie <scratch>/f.png --fixed-fps 60 --quit-after 120   # run the game 2 s and dump frames to inspect visually
"$GODOT" --headless --script res://benchmarks/traffic_benchmark.gd   # traffic perf at 500/1k/2k/5k vehicles (~40 s)
```

In game: debug panel on the right (`ui/debug_menu.gd`, F1 folds it) has a button for every debug function below; add new ones there too. F3 lane overlay, F4 congestion heatmap, F5/F9 quicksave/quickload, F6 cycles stress traffic (0–2000 random-trip vehicles), F7 loads the Bottleneck scenario, F8 the Town scenario (a complete small city living through a day; `Scenarios`); the HUD top bar shows vehicles, stuck count and tick cost. Tool keys: R road, B bulldoze, H/C/I zone residential/commercial/industrial, X dezone, L traffic lights, T route inspector.

For a visual check of a specific setup, pass `--script <scratch>/x.gd` (a `SceneTree` subclass that instantiates `res://scenes/main.tscn`, builds state via `root.get_node("Game").simulation`, and toggles overlays) together with `--write-movie`.

Testing gotchas:
- **GUT reports "All tests passed" even when a test script fails to parse.** Always grep the output for `SCRIPT ERROR` / `Parse Error`, not just the summary.
- Integration tests (`tests/integration/`) run `main.tscn` inside a `SubViewport` and `push_input()` events into it. Don't send mouse input through the root window: GUT's on-screen output panel swallows clicks, and the headless window is tiny (64 px), so coordinates get stretched.
- GUT's `InputSender.key_down` sets only `keycode`, but our actions bind `physical_keycode`, so send `InputEventAction`s instead.
- `main.tscn` is hand-edited: when rewriting it, keep every node (`test_main_scene_has_its_key_nodes` guards this).

Python is not installed; use bash/PowerShell or the edit tools for file manipulation.

## Architecture rules (see the plan for details)

- **`sim/` is pure logic**: `RefCounted` classes, no `Node`, no autoload access, no rendering. Must be steppable headless from tests.
- **`view/` only reads sim state**; **`tools/`** mutate the sim only through command methods on sim systems (e.g. `RoadNetwork.add_road`).
- **Fixed tick**: the sim advances only via `Simulation.step()` (20 Hz, `Simulation.TICK_DT`), driven by the `Game` autoload. Never use frame `delta` inside the sim. Renderers interpolate with `Game.interpolation_alpha`.
- Pausing = `Game.set_speed(0)`, not `get_tree().paused`.
- Cross-system notifications go through the `Events` autoload signal bus.
- **Everything is a lane**: roads and intersection turns are `Lane`s holding a `Curve2D`; never move vehicles tile-by-tile, never use `TileMapLayer` for roads (zones only).
- Grid snapping is a UI aid only (`Grid.snap_to_point`); the sim works in world pixels.
- Road edits go through `RoadNetwork` commands (`add_road` / `add_road_pieces`, `remove_segment`), which emit `changed(node_ids, segment_ids)`. Adding a road splits existing segments where it meets them, so segment ids are not stable across edits; views update incrementally from those ids (ids may refer to removed things). Lanes are derived data: never saved, rebuilt from centerlines. Road lanes keep their ids for the segment's lifetime; connector lanes (junction turns, `segment_id == -1`) are regenerated with new ids whenever their node changes.
- Routing: `Simulation.router.find_route(from_lane, from_distance, to_lane, to_distance)` returns a `Route` (lane ids + start/end distances) or null. Trip endpoints are positions along road lanes, never nodes.
- Build tools extend `BuildTool`, live under the `ToolManager` node (node name = tool id), and read the pointer with `get_pointer_position()` / `get_snapped_pointer_position()`, never `get_global_mouse_position()` (which ignores synthetic test input).
- Saves: `Simulation.to_dict()` / `from_dict()`. Bump `Simulation.SAVE_VERSION` when the format changes incompatibly. Roads, lots, zones and external connections are saved; citizens, vehicles, light states and Flow history are not (people restart at home on load).
- **`dispose()` discarded simulations and road networks** (`Simulation.dispose()`, `RoadNetwork.dispose()`): lanes link to each other (`Lane.outgoing_lanes`) and to the vehicles in their queues, so without it they are never freed. `Game` does it when replacing or exiting; tests do it in `after_each`. A "resources still in use at exit" error means something wasn't disposed.
- Vehicles: only `VehicleSystem` creates, moves or removes them. Per-lane queues live on `Lane.queue` (front-most first). Junction entry is granted per connector using conflict zones (`Lane.conflicting_lane_ids` / `Lane.conflict_zones`) and timing; see the `VehicleSystem` class doc. Safety invariant (checked by the stress tests): never two vehicles inside the zone two conflicting connectors share. Hot paths hold `Lane` objects (`Lane.outgoing_lanes`, `Vehicle.next_lanes`) rather than ids.
- City: lots are buildings; `ZoneSystem` creates them on zoned cells (hand-placed lots via `LotSystem.add_lot` are for scenarios/tests). Citizens and freight decide trips; `VehicleSystem` drives them and reports `vehicle_arrived` / `vehicle_failed` (incl. patience give-ups), which `FlowMetric` scores. New maps get a highway via `Scenarios.add_highway`; `Game.new_simulation()` alone gives an empty map (used by tests).
- Junction control lives on `RoadNode.control` (set via `RoadNetwork.set_node_control`); traffic lights are vehicle-actuated by `Simulation.signals` (`TrafficLights`, state not saved). Congestion costs come from `CongestionMonitor` every 5 s and are shared with `router.graph.congestion`.
- **Never connect a lambda that captures `self` to a signal of another sim object** (e.g. `roads.changed.connect(func(): _dirty = true)`): the lambda keeps `self` alive through the signal, forming a reference cycle that leaks the whole `Simulation` ("resources still in use at exit"). Connect a bound method instead.
- Sim systems are wired in `Simulation._init` in dependency order (roads → router → lots → zones → externals → signals → vehicles → citizens → freight → stress → congestion → flow); `step()` runs zones (development), citizens, freight and stress traffic (departures), traffic lights, vehicles (movement, arrivals), then the congestion monitor (every 5 s).

## Layout

```
core/    autoloads (Game, Events) + Grid helpers
sim/     simulation state & systems: SimClock, Simulation, roads/, routing/, city/ (lots, zones, citizens, external connections, flow metric), traffic/ (vehicles, freight, lights, congestion, stress), debug/ (scenarios)
view/    renderers, camera, overlays
tools/   player tools (ToolManager + RoadTool, BulldozeTool, ZoneTool x4, SignalTool, RouteTool debug inspector); shortcuts in ToolManager.TOOL_SHORTCUTS
ui/      HUD and menus
scenes/  main.tscn (+ sandbox scenes)
benchmarks/  headless perf scripts (run with --script)
tests/   GUT tests: unit/ (pure sim) and integration/ (main scene + synthetic input); files test_*.gd extending GutTest; traffic_checks.gd = shared safety checks (no overlaps, no conflict-zone violations)
data/    Resources (.tres), e.g. data/road_types/
addons/gut/  third-party, don't edit
```

## Code style

- Tabs, static typing everywhere (`var x: int`, `:=`, typed returns), `class_name` for reusable classes.
- `##` doc comment under `extends` explaining the class's role; comment the *why*, not the *what*.
- Private members prefixed `_`. Constants `UPPER_SNAKE`. Signals in past tense (`speed_changed`).
- Input: add actions to `project.godot` `[input]` using `physical_keycode` so bindings follow key position on any layout (WASD = ZQSD on AZERTY).
- New sim logic gets unit tests in `tests/unit/`.
