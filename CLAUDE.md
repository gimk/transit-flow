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
```

For a visual check of a specific setup, pass `--script <scratch>/x.gd` (a `SceneTree` subclass that instantiates `res://scenes/main.tscn`, builds state via `root.get_node("Game").simulation`, and toggles overlays) together with `--write-movie`.

Testing gotchas:
- **GUT reports "All tests passed" even when a test script fails to parse.** Always grep the output for `SCRIPT ERROR` / `Parse Error`, not just the summary.
- Integration tests (`tests/integration/`) run `main.tscn` inside a `SubViewport` and `push_input()` events into it. Don't send mouse input through the root window: GUT's on-screen output panel swallows clicks, and the headless window is tiny (64 px), so coordinates get stretched.
- GUT's `InputSender.key_down` sets only `keycode`, but our actions bind `physical_keycode`, so send `InputEventAction`s instead.

Python is not installed; use bash/PowerShell or the edit tools for file manipulation.

## Architecture rules (see the plan for details)

- **`sim/` is pure logic**: `RefCounted` classes, no `Node`, no autoload access, no rendering. Must be steppable headless from tests.
- **`view/` only reads sim state**; **`tools/`** mutate the sim only through command methods on sim systems (e.g. `RoadNetwork.add_road`).
- **Fixed tick**: the sim advances only via `Simulation.step()` (20 Hz, `Simulation.TICK_DT`), driven by the `Game` autoload. Never use frame `delta` inside the sim. Renderers interpolate with `Game.interpolation_alpha`.
- Pausing = `Game.set_speed(0)`, not `get_tree().paused`.
- Cross-system notifications go through the `Events` autoload signal bus.
- **Everything is a lane**: roads and intersection turns are `Lane`s holding a `Curve2D`; never move vehicles tile-by-tile, never use `TileMapLayer` for roads (zones only).
- Grid snapping is a UI aid only (`Grid.snap_to_point`); the sim works in world pixels.
- Road edits go through `RoadNetwork` commands, which emit `changed(node_ids, segment_ids)`; views update incrementally from those ids (ids may refer to removed things). Lanes are derived data: never saved, rebuilt from centerlines.
- Build tools extend `BuildTool`, live under the `ToolManager` node (node name = tool id), and read the pointer with `get_pointer_position()` / `get_snapped_pointer_position()`, never `get_global_mouse_position()` (which ignores synthetic test input).
- Saves: `Simulation.to_dict()` / `from_dict()`. Bump `Simulation.SAVE_VERSION` when the format changes incompatibly.

## Layout

```
core/    autoloads (Game, Events) + Grid helpers
sim/     simulation state & systems (SimClock, Simulation, roads/; later routing/traffic/city)
view/    renderers, camera, overlays
tools/   player build tools (ToolManager, RoadTool, BulldozeTool)
ui/      HUD and menus
scenes/  main.tscn (+ sandbox scenes)
tests/   GUT tests: unit/ (pure sim) and integration/ (main scene + synthetic input); files test_*.gd extending GutTest
data/    Resources (.tres), e.g. data/road_types/
addons/gut/  third-party, don't edit
```

## Code style

- Tabs, static typing everywhere (`var x: int`, `:=`, typed returns), `class_name` for reusable classes.
- `##` doc comment under `extends` explaining the class's role; comment the *why*, not the *what*.
- Private members prefixed `_`. Constants `UPPER_SNAKE`. Signals in past tense (`speed_changed`).
- Input: add actions to `project.godot` `[input]` using `physical_keycode` so bindings follow key position on any layout (WASD = ZQSD on AZERTY).
- New sim logic gets unit tests in `tests/unit/`.
