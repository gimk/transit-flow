# Transit Flow: Migration from Godot to Bevy

**Status:** M0–M4 done (M4.9 optional) and R2 passed; the traffic matches Godot tick for tick and runs 11–16× faster. Next: **M5** (walking); **M8.1** (game shell) can run in parallel.

This plan moves Transit Flow from Godot 4.7 / GDScript to **Rust + Bevy**. Opus, Sonnet and Haiku agents carry it out, one **work package (WP)** per session. It is written for them: every WP says which model runs it, what to read, what to write, and the exact checks that mark it done.

The Godot project stays in the repository, untouched and runnable, until the cutover (M12). It is the **reference implementation**: when this plan and the GDScript disagree, the GDScript is right, unless `docs/migration/NOTES.md` records a deliberate deviation.

---

## 1. Why, and what does not change

- **Why:** the game is CPU-bound (≈7 µs per vehicle per tick in GDScript; 2 000 vehicles don't fit a frame, see `IMPLEMENTATION_PLAN.md` §5), and the author works code-first, not in the Godot editor.
- **What we keep, exactly:** game design, every sim rule and constant, the tick rate (20 Hz), the save format (JSON, `SAVE_VERSION` 1), the keyboard and mouse controls, and the architecture rules in `CLAUDE.md` (sim separate from view, fixed tick, everything is a lane, views read snapshots).
- **What we leave behind (user decision, 2026-11-11):** the generated art and the visual effects are **not** migrated. That covers the vehicle, building, tree and pedestrian shaders and painters, ground and asphalt grain, night lights and headlights, the day/night tint, and the whole dot display (Phase D: layers, composite shader, levels of detail, alerts). So is the UI beyond the basic commands: tool palette, HUD panels and debug panel. Everything is drawn with **simple flat shapes** (§2.3), and the art direction will be redone later in Bevy. The sim data those views read (snapshot fields such as indicators, lateness, parked cars) is still ported, since it's cheap and the future art will need it.
- **What this is not:** a redesign of the sim. No new features, no retuning, no "while I'm here" refactors. Behaviour changes go through the user after the cutover.

## 2. Target architecture

### 2.1 Workspace

A Cargo workspace at the repository root, next to the Godot project while both exist:

```
Cargo.toml                 workspace; [workspace.dependencies] pins every version
rust-toolchain.toml        pinned stable toolchain
crates/
  tf_sim/                  pure simulation library: NO bevy dependency
    src/  clock.rs, simulation.rs, geom/ (polyline, segment math), roads/, routing/,
          city/, traffic/, walking/, transit/, debug/ (scenarios), save.rs, rng.rs
    tests/                 integration tests, one file per GUT unit test file
    benches/ or src/bin/   traffic_benchmark
    data/road_types/*.ron  road types (embedded with include_str!)
  tf_game/                 the Bevy app (binary "transit-flow")
    src/  main.rs, runner.rs (threaded sim), snapshot.rs, events.rs, input.rs,
          camera.rs, draw.rs (shape helpers), view/ (renderers), tools/, ui/ (status line)
    tests/                 headless app tests (tools, UI wiring)
migration/
  export_golden.gd         Godot script that dumps reference data (M0.3)
  golden/*.json            the dumped reference data, committed
  baseline.md              Godot metrics the Rust port must match (M0.3)
docs/migration/NOTES.md    deviations, GDScript bugs found, decisions taken
scripts/check.sh           fmt + clippy + tests: the one command every WP runs
```

`crates/` and `target/` each get an empty `.gdignore` so Godot doesn't import them.

### 2.2 `tf_sim`: the simulation as a plain Rust library

- **Not ECS.** The sim is ported as ordinary structs with the same names and the same responsibilities as the GDScript classes (`RoadNetwork`, `Router`, `LotSystem`, `VehicleSystem`, ...), owned by `Simulation`. That keeps the port 1:1 and reviewable against the GDScript. Bevy's ECS is used only in `tf_game`.
- **Dependencies (only these, unless a WP says otherwise):** `glam` (same version Bevy uses, for `Vec2`), `serde` + `serde_json` (saves, golden files), `ron` (road types), `indexmap`, `smallvec` (the RNG is our own port of Godot's, `rng.rs`). Tests: `approx`.
- **Ids and storage.** Every GDScript `Dictionary[int, X]` becomes `IndexMap<i32, X>`. Removals use `shift_remove`, never `swap_remove`, so iteration order matches Godot (insertion order). Many results depend on that order. Ids keep the same types and counters as in GDScript (they're saved).
- **Object references become ids.** `Lane.outgoing_lanes`, `Lane.queue`, `Vehicle.next_lanes` and the like hold `LaneId` / `VehicleId` (newtypes over `i32`) or dense indices, never `Rc`/`RefCell`. No `dispose()` is needed. Hot paths may keep a dense `Vec` index next to the `IndexMap` if the benchmark asks for it (M4.9), not before.
- **Signals become explicit dispatch** (see the inventory in §2.4). `Simulation` owns every system, so it calls the handlers itself, in the order Godot connects them. Systems never hold references to each other: a system method takes the other systems it needs as parameters (`vehicles.step(dt, &mut TrafficCtx { roads, router, lots, signals, crosswalks })`).
- **Godot types → Rust:**

  | GDScript | Rust (`tf_sim`) |
  |---|---|
  | `float` | `f64` (GDScript floats are 64-bit); `Curve2D` values stay `f32` like Godot's (see `docs/migration/PORTING_GUIDE.md`) |
  | `Vector2` | `glam::Vec2` |
  | `Vector2i` | `glam::IVec2` |
  | `Curve2D` (always a polyline here) | `geom::Polyline`: points + cumulative lengths; `sample(d) -> (pos, angle)`, `closest_offset(p)`, `length()` |
  | `Geometry2D.get_closest_point_to_segment`, `segment_intersects_segment`, `get_closest_points_between_segments` | `geom::segment` functions, unit-tested against values dumped from Godot |
  | `AStar2D` (`LaneGraph`) | own A* over a CSR or adjacency `Vec`, with `BinaryHeap`; same costs and heuristic |
  | `RandomNumberGenerator` with a fixed seed | `rng::SimRng` with the **same seed constants**: bit-exact with Godot (verified in M0.2), so the same draws happen in the same order |
  | `RoadType` `.tres` | `RoadType` struct, `data/road_types/<id>.ron`, same fields and values |
  | `Dictionary` used as a record (saves) | `serde_json::Value` at the save boundary only; typed structs everywhere else |
  | `Array.sort_custom` | `sort_by` (stable; Godot's isn't, so ties may order differently: acceptable) |
  | `signal` | a method on `Simulation` that calls the listeners in connection order, or an event `Vec` drained at a defined point |

- **Determinism.** The same inputs give the same run on every machine: no `HashMap` iteration in sim logic, no wall-clock time, no threads inside `tf_sim` until M4.9.

### 2.3 `tf_game`: the Bevy app

- **Threaded sim runner** (port of `core/game.gd`). The `Simulation` moves by value between the main thread and a worker thread (ownership ping-pong over channels): the worker gets the sim, steps a batch of ≤ 4 ticks, captures a `ViewSnapshot` after each, and sends everything back. The main thread renders from `view_from` / `view_to` / `view_alpha`. When a tool needs the sim, it waits for the sim to come back (`SimHandle::get_mut()`), exactly like `Game.simulation`. Road geometry for the views is published as an `Arc<RoadsView>` after every edit, so views never wait. There are no locks in the sim. Speeds 0/1/2/4, `LEAD_TICKS` 2 and the lag detection stay the same.
- **Events** (port of `core/events.gd`): one Bevy `Event` type per Godot signal, same names.
- **Views** are Bevy systems that read only `ViewSnapshot`, `RoadsView` and events. They draw **simple flat shapes** with Bevy's built-in 2D meshes and `ColorMaterial`, with no custom shaders and no textures:
  - roads: a grey band per segment, with a thin line per lane;
  - lots: a rectangle on their plot, coloured by zone;
  - zones without a building: an outlined cell;
  - vehicles: one rectangle each (length × width, rotated), green for cars, brown for trucks, the line colour for buses;
  - pedestrians and waiting riders: small dots;
  - bus stops: a square; bus lines: a coloured polyline;
  - lights: a short bar per stop line in the light's colour;
  - highway connections: a marker.

  Static shapes (roads, lots, zones) are rebuilt only for the ids in a change event. Moving things (vehicles, people) use one instanced or batched mesh, updated each frame from the interpolated snapshots. Gizmos are fine for debug overlays (lanes, route inspector, tool previews).
- **UI: basic commands only.** Every keyboard shortcut and mouse action works: tools R / B / H / C / I / X / L / U / T / V and Esc, variant cycling, speed (space, 1–3), F2–F9, F11, and pan and zoom. One plain text line (`bevy_ui` `Text`) shows the time, speed, Flow %, vehicle count, the active tool and variant, its hint, and the last notification. No palette, no panels, no debug menu: debug functions that only had a button (rush hour, new empty map, the bus-lanes scenario) get keys, listed in `CLAUDE.md`.
- **Input:** a `KeyBindings` resource mapping actions to `KeyCode` (physical keys, as in `project.godot`). Tools read the pointer from a `Pointer` resource (world position, snapped position, buttons), never from the window directly, so tests can drive them.
- **Headless tooling:** `transit-flow --scenario town --frames 120 --screenshot out.png` runs the game N frames and saves a screenshot (the counterpart of `--write-movie`). Agents use it for visual checks.

### 2.4 Signal inventory (dispatch order to preserve)

Godot calls listeners in the order they connected, and that is `Simulation._init` order. `Simulation` must call them in the same order:

| Signal | Listeners, in order | Rust |
|---|---|---|
| `RoadNetwork.changed(nodes, segs)` | router, lots, zones, signals, vehicles, walk network, transit, stress (plus `Game` → views) | Every road edit goes through `Simulation::{add_road_pieces, add_path_pieces, remove_segment, set_segment_type, set_node_control, ...}`, which runs the `RoadNetwork` command and then calls `on_roads_changed` on each listener, synchronously |
| `LotSystem.changed` | vehicles (reroute), citizens, freight (+ views) | same, via `Simulation` wrappers around lot and zone commands, and after `zones.step` when it developed lots |
| `ZoneSystem.changed`, `ExternalConnections.changed`, `SimClock.hour_changed` / `day_started` | views only | flags or counters in `Simulation`, read by the runner and turned into Bevy events |
| `VehicleSystem.vehicle_arrived` / `vehicle_failed` | transit, citizens, stress, flow | `VehicleSystem` pushes `VehicleEvent`s into a `Vec` during its step. Wherever the GDScript emits them, check whether a listener's effect is visible to the rest of the same `step` (M0.2 decides per emit site: drain right after `vehicles.step`, or split the step) |
| `PedestrianSystem.pedestrian_arrived` | citizens | same pattern |
| `TransitSystem.rider_arrived` / `rider_stranded` | citizens | same pattern |
| `CitizenSystem.trip_ended` | flow | same pattern |

Before dispatch, check the exact connection order of each signal in the code, not just this table.

## 3. How parity is proven

Three layers, from strict to statistical:

1. **Golden data (exact, tolerance 1e-3 px).** `migration/export_golden.gd` (M0.3) runs in headless Godot and dumps JSON for pure, RNG-free functions: lane geometry, connectors, conflict zones, signal phases, `plan_road` / `plan_path` results, splitting outcomes, lot plots and access points, walk network, routes and their costs. It runs on a fixed list of scripted layouts: the scenarios in `sim/debug/scenarios.gd` (Bottleneck, Town, bus-lanes variant) plus the layouts built in `test_road_*`, `test_curves`, `test_junction*`, `test_router`. The Rust tests rebuild the same layouts and compare. Expect float noise; a mismatch beyond 1e-3 px is a bug in the port.
2. **Ported tests (exact assertions).** Every GUT file in `tests/unit/` becomes a Rust test file with the same test names (`test_town.gd` → `crates/tf_sim/tests/town.rs`, `fn town_uses_every_feature`). Assertions are ported 1:1. If an assertion depends on RNG draws (a specific citizen, an exact count), widen it into a range and log it in `NOTES.md`. `tests/traffic_checks.gd` becomes `tests/common/traffic_checks.rs` and runs every tick in the stress and Town tests: no overlaps, never two vehicles in a shared conflict zone, no pedestrian on a crosswalk while a car is on a turn over it.
3. **Behavioural metrics (statistical).** `migration/baseline.md` (M0.3) records Godot's numbers: Town for one day (Flow %, failed trips, peak vehicles, mode shares, the time 95% of drivers are at work), Bottleneck ± bypass ± bus lanes, the stress grid (arrivals in 90 s per road mix, stuck count). The Rust port must land within the bands set there (default: Flow ±5 points, failed trips ≤ Godot + 3, arrivals ±15%, stuck 0 where Godot has 0). The RNG is bit-exact (M0.2), so runs start identical and only drift apart through float rounding and Godot-specific details: compare single runs with the same seeds. Where a metric is out of its band, compare the early ticks of both runs to find where they first diverge.

## 4. Rules for every agent (read before any WP)

1. **One WP per session.** Read this plan's §2–§4, `CLAUDE.md` and your WP. Read **every GDScript file listed in "Read" in full** before writing Rust. Don't port from memory or from this plan's summaries.
2. **Port faithfully.** Same names (snake_case already matches Rust), same constants and values, same order of operations, same comments on the *why* (as `///` / `//`). Don't optimise, simplify or "fix" anything. If the GDScript looks wrong, port it as is and add an entry to `docs/migration/NOTES.md` (file, line, what looks wrong). Only M4.9 optimises.
3. **Don't touch Godot files** (`*.gd`, `*.tscn`, `*.gdshader`, `project.godot`, `addons/`, `data/*.tres`) except `migration/` and the docs. Godot stays the reference until M12.
4. **Done means** `scripts/check.sh` passes: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Plus your WP's own "Done when". Never weaken or delete a ported assertion to make it pass. If you can't pass it, stop and report: the WP isn't done.
5. **No `unsafe`.** `unwrap()` only where the GDScript would also fail (an invariant); `expect("why")` there.
6. **Commits:** on branch `bevy-migration`, one or more commits per WP, message `M<x.y>: <what>`. The commit identity is set in the repo (`gimk`); don't change it. **Never add Claude as co-author** or a "Generated with" line. Work in parallel only on WPs marked parallel-safe, each in its own git worktree, and merge into `bevy-migration` when done.
7. **When finished,** tick the WP in §6, set the **Status** line at the top, and add measured numbers and anything surprising under the WP. Keep it short.
8. **Stop and ask the user** if a WP needs a new dependency not listed in §2, a deviation in behaviour, or a design choice this plan doesn't make.

### Model assignment

- **Opus:** foundations other WPs copy (M0), the hardest ports (`RoadNetwork`, `VehicleSystem`, `TransitSystem`), threading, and the **review gates** (R1–R4). Opus also takes over any WP that fails twice with another model.
- **Sonnet:** the default for porting a system with its tests, renderers, tools and UI.
- **Haiku:** mechanical, fully specified work: data conversion, small data classes, porting a test file whose API already exists, running benchmarks and filling tables, doc updates. Haiku WPs list exact outputs. If a Haiku WP turns out to need judgement, hand it to Sonnet.

## 5. Godot reference commands (this Mac)

```bash
GODOT=/Applications/Godot.app/Contents/MacOS/Godot     # 4.7.2
"$GODOT" --headless --import
"$GODOT" --headless -s addons/gut/gut_cmdln.gd          # grep output for "SCRIPT ERROR" / "Parse Error"
"$GODOT" --headless --script res://migration/export_golden.gd
"$GODOT" --headless --script res://benchmarks/traffic_benchmark.gd
```

## 6. Work packages

Format: **ID, title** · model · depends on · parallel-safe with. *Read* = GDScript to read in full. *Write* = files to create. *Done when* = checks beyond `scripts/check.sh`.

### M0: Toolchain, conventions, reference data

- [x] **M0.1 Toolchain and workspace** · Opus · none
  - *Done (2026-11-11):* Rust 1.99.1 via rustup (in `~/.cargo`, shell profiles not edited), Bevy `=0.20.1`, glam `=0.33.12` (Bevy's own; 0.34 exists but would give two `Vec2` types), every other dependency pinned exactly. The first full build takes ~7.5 min on this M1, and `check.sh` is fast after that. The window opens on Metal. Godot still imports, and GUT passes 251/252: the one failure is a pre-existing case-mismatch warning on this Mac's user-data folder (see `CLAUDE.md`), not caused by this WP.
  - Rust is not installed on this machine: ask the user to run `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` (or approve it), then pin the current stable release in `rust-toolchain.toml`.
  - Create the workspace (§2.1): `tf_sim` (lib, empty modules), `tf_game` (Bevy binary opening a window with a 2D camera and a cleared background). Pin Bevy to the newest released 0.x (`=0.x.y`), `glam` to the version that Bevy release uses, and every other dependency in `[workspace.dependencies]`. Add `[profile.dev.package."*"] opt-level = 3` (Bevy is unusable at opt-level 0) and a `release-lto` profile.
  - `scripts/check.sh`; `.gdignore` in `crates/` and `target/`; `.gitignore` gets `target/`.
  - Add a short "Migration in progress" section to `CLAUDE.md` pointing at this plan and `scripts/check.sh`.
  - *Done when:* `cargo run -p tf_game` opens a window; `scripts/check.sh` passes; Godot still imports cleanly and its tests still pass.
- [x] **M0.2 Porting conventions and skeleton types** · Opus · M0.1
  - *Done (2026-11-11):* ids, `SimRng`, `geom` (Polyline, segment functions), `events` (`SimEvent`, `Outbox`), the `Simulation` skeleton with depth-first dispatch, `SimClock` with its 6 ported tests, one stub per system. Two finds raise the bar for parity: `SimRng` is **bit-exact** with Godot's `RandomNumberGenerator`, and `geom::segment` matches `Geometry2D` on 48 cases Godot printed (`migration/probes/`). Rust now uses `f64` wherever GDScript has a `float` (it's 64-bit) and `f32` for `Vector2` and `Curve2D` values. Open for M0.3 and M1.2: Godot's `Curve2D` baking (see `NOTES.md`, Geometry).
  - In `tf_sim`: id newtypes (`NodeId`, `SegmentId`, `LaneId`, `VehicleId`, `LotId`, `CitizenId`, `PedestrianId`, `StopId`, `LineId`), `SimRng`, `geom::Polyline` and segment helpers (with tests), the `Simulation` struct with every system as an empty stub, and the `step()` order from `simulation.gd`.
  - Read every `emit(` site of the signals in §2.4, decide for each where its event is drained, and record the decisions in `NOTES.md` under "Event dispatch". This sets the pattern every later WP follows.
  - Write `docs/migration/PORTING_GUIDE.md`: a worked example porting one small class (`sim_clock.gd` → `clock.rs` with its test file), showing doc comments, constants, signal handling and test naming. Every later agent reads it.
  - *Done when:* `clock.rs` passes the ported `test_sim_clock.gd`; the guide exists; `NOTES.md` has the dispatch decisions.
- [x] **M0.3 Golden exporter and baseline** · Sonnet · M0.1 · parallel with M0.2
  - *Done (2026-11-11):* `migration/golden/` (4 MB, deterministic): five layouts (`bottleneck`, `bottleneck_bypass`, `bottleneck_bus_lanes`, `town`, `junctions`: every junction kind the Town lacks) and `edits.json` (14 road commands, with the network after each). Change from the text below: layouts are stored as their **save** plus everything derived from the *reloaded* save, not as a command log. Lanes are derived from the saved centrelines, so Rust needs only `from_dict` (M1.3) to rebuild them, and ids match; the command log lives in `edits.json` for the splitting, upgrade and removal rules. Until M1.3, M1.2 can test lane building per segment from the dumped centrelines. It confirms the baking question: Godot resamples curves (a 96-point curve bakes into 190 points), so M1.2 must port the baking. `migration/baseline.md`: Town day (Flow 60.3%, 2 failed, peak 165, 95% of drivers at work 12.4 h), Bottleneck mornings and Flow (50.2% plain, 54.3% with bus lanes), 5 stress grids (0 stuck, 0 violations), and the GDScript benchmark on this M1 (2 000 vehicles: 26.5 ms per tick). Both scripts run in about 30 s.
  - *Read:* `sim/debug/scenarios.gd`, `benchmarks/traffic_benchmark.gd`, `tests/unit/test_town.gd`, `test_bottleneck.gd`, `test_traffic.gd`, `test_road_splitting.gd`, `test_curves.gd`, `test_router.gd`, `test_lots.gd`, `test_walking.gd`.
  - *Write:* `migration/export_golden.gd` (a `SceneTree` script; builds each layout and writes `migration/golden/<layout>.json`. Each file starts with the **command log** that built the layout: every `RoadNetwork` / `LotSystem` / `ZoneSystem` / `TransitSystem` call in order, with its arguments. Rust tests replay it, so M1–M6 can check golden data long before `Scenarios` is ported in M7.3. Record it by wrapping those calls in the exporter, not by editing `sim/`. After the log come the results: nodes, segments, lanes with their control points and handles, Godot's `get_baked_points()`, `get_baked_length()` and `sample_baked_with_rotation()` at a few offsets (M1.2 needs them to reproduce Godot's baking), outgoing ids, conflicting ids and zones, signal phases, plots, access points, walk network, and a fixed list of routes with costs). Then `migration/run_baseline.gd`, which runs the metric scenarios of §3 and writes `migration/baseline.md` with the numbers and the tolerance bands, and the benchmark table re-run on this Mac.
  - *Done when:* both scripts run headless without errors and the JSON and `baseline.md` are committed. The JSON schema is documented at the top of `export_golden.gd`.

### M1: Roads (the foundation everything else stands on)

- [x] **M1.1 Road types and data classes** · Haiku · M0.2
  - *Done (2026-11-11, with M1.2):* `RoadType` (6 `.ron` files, every field tested against the `.tres` values), `RoadNode` (`JunctionControl`), `RoadSegment`, `Lane` (`Turn`, straight-lane fast path; `segment_id` and `feeder_lane_id` are `Option`s; `outgoing_lanes` objects dropped, ids are enough). Road types are `&'static` (a registry built once).
  - *Read:* `sim/roads/road_type.gd`, `road_node.gd`, `road_segment.gd`, `lane.gd`, `data/road_types/*.tres`.
  - *Write:* `roads/road_type.rs` (struct, `list_all()` sorted like Godot, `load_by_id`, helpers), the 6 `.ron` files with identical values (colours as `[r, g, b, a]` floats), `roads/road_node.rs`, `roads/road_segment.rs`, `roads/lane.rs` (fields as in §2.2; `sample_pose` and the straight-lane fast path included).
  - *Done when:* a test loads all 6 types and checks every field against the `.tres` values; lane sampling tests pass.
- [x] **M1.2 LaneBuilder** · Sonnet · M1.1
  - *Done (2026-11-11):* `geom::Curve2D` ports Godot 4.7's `Curve2D` baking (control points plus recursive Bézier midpoints, a tangent per baked point, headings slerped when sampling). **All 1 893 golden curves** match, with a worst position error of 1.5e-5 px and the same headings (`tests/curve_baking.rs`). `geom::gd` has Godot's `Vector2` maths (`normalized`, `lerp`, `slerp`, Bézier). `lane_builder` is ported with its 4 tests, and every golden connector equals `build_connector_curve(feeder, exit)` (`tests/lane_builder.rs`). Road lanes against the golden files need the junction trims, so they are checked in M1.3. `geom::Polyline` was dropped: `Curve2D` replaces it.
  - *Read:* `sim/roads/lane_builder.gd`, `tests/unit/test_lane_builder.gd`.
  - Also `geom::bake`: Godot's `Curve2D` baking (`Curve2D::_bake` in Godot 4.7's `scene/resources/curve.cpp`, MIT), for polylines and cubic Béziers, and `Polyline::sample_with_rotation` interpolating headings as Godot does. See `NOTES.md` (Geometry). This makes M1.2 an **Opus** WP if the baked points don't match on the first try.
  - *Done when:* those tests are ported and pass; lane points match the golden files.
- [x] **M1.3 RoadNetwork** · Opus · M1.2
  - *Done (2026-11-11):* full port. Commands take an `Outbox` (see NOTES); removed lanes are retired, not dropped, so vehicles can finish them (NOTES; M4.2 must prune). **Golden:** all five layouts load from their saves into exactly Godot's network (every id in order, lane curves, speeds, turns, conflict zones, signal phases, within 1e-3), `to_dict` re-saves the same JSON, and the 14 recorded edits give Godot's result, `changed` ids and network at every step (`tests/road_network_golden.rs`). A deliberate 1 px change to `CONFLICT_DISTANCE` makes them fail. **Ported:** `test_road_network.gd` (13), `test_road_splitting.gd` (14, the fuzz with Godot's exact random roads), `test_junctions.gd` (12), `test_curves.gd` (4), `test_road_types.gd` (network parts). The 10 tests that need later systems are `#[ignore]` stubs, listed in NOTES. Workspace: 73 tests pass.
  - *Read:* `sim/roads/road_network.gd` (837 lines: planning, splitting, curves, upgrades, connectors, conflict zones, signal phases, saves), plus `test_road_network.gd`, `test_road_splitting.gd`, `test_road_types.gd`, `test_curves.gd`, `test_junctions.gd` (the network parts).
  - The command API returns what `changed` would carry (`ChangeSet { node_ids, segment_ids }`), and `Simulation` dispatches it (§2.4). `to_dict` / `from_dict` read and write the same JSON as Godot.
  - *Done when:* those test files are ported (the network parts) and pass; every golden layout matches (topology exactly, geometry within 1e-3); a Godot quicksave JSON loads and re-saves to identical JSON; the 60-road fuzz test passes.
- [x] **R1 Review gate** · Opus · M1.3
  - *Done (2026-11-11):* the golden comparisons cover every node, segment and lane field in five layouts and 14 edit steps, so a line-by-line review adds little. I re-read the spots golden data can't reach (unknown road types and old saves in `from_dict`, `control_from_save`, `set_node_control` on small nodes); they match the GDScript.
  - Review `tf_sim/roads` against the GDScript line by line, looking for skipped branches, changed constants and wrong iteration order. Check `NOTES.md`. Fix or send back before M2 starts.

### M2: Routing

- [x] **M2.1 LaneGraph, Route, Router** · Sonnet · R1
  - *Done (2026-11-11):* Godot 4.7's A* ported (see NOTES), so ties break as in Godot. **Every golden route matches lane for lane**, with the same travel and free-flow times: 530+ samples over six layouts, including lot-to-lot routes through Godot's access points and a new `grid` layout of equally quick routes. `test_router.gd` (12 tests) and the two routing tests deferred from M1 are ported. Workspace: 88 tests pass, 8 waiting for lots and vehicles.
  - *Read:* `sim/routing/*.gd`, `test_router.gd`.
  - A* with the same cost (`length / speed_limit × (1 + congestion)`, `LANE_CHANGE_COST`), the same heuristic, the second bus graph, the path cache and its invalidation, `find_route`, `find_route_onward`, `find_best_route`.
  - *Done when:* `test_router` is ported and passes; golden routes match (same lane lists, cost within 1e-3; where two routes cost the same, either is accepted and logged).

### M3: City foundations · parallel-safe with M4.1–M4.3 once M2 is in

- [x] **M3.1 Lot, LotSystem, Parking** · Sonnet · M2.1
  - *Done (2026-11-11):* every lot of the six golden layouts, restored from its save, has Godot's plot and access points (lanes, set-back distances, far-side penalty; `tests/lots_golden.rs`). `test_lots.gd` is ported (12; the avenue test needs zones and citizens, M7.1), and so are the two lot tests deferred from M1. `Grid` (and `test_grid.gd`) moved into the sim. `Parking` is ported; its tests come with citizens (M7.1).
  - *Read:* `sim/city/lot.gd`, `lot_system.gd`, `parking.gd`, `test_lots.gd`.
  - *Done when:* `test_lots` is ported and passes; plots and access points match the golden files.
- [x] **M3.2 ZoneSystem, ExternalConnections** · Sonnet · M3.1
  - *Done (2026-11-11):* both ported (RNG seed 7, as Godot's, so the same development delays). `test_zones.gd` is ported (5; the save test waits for `Simulation` saves, M7.3), plus a check that every golden layout's zones and external connections restore and save back unchanged, and a test of the connection points (inbound and outbound lanes).
  - *Read:* `sim/city/zone_system.gd`, `external_connections.gd`, `test_zones.gd`.
  - *Done when:* `test_zones` is ported and passes.

### M4: Traffic (the big one; the performance payoff)

`vehicle_system.gd` is 1 666 lines and is ported in slices, all by Opus, in order, each slice compiling and tested before the next. Split the Rust into modules under `traffic/vehicle_system/` (`mod.rs` core and step, `following.rs`, `junctions.rs`, `lane_changes.rs`, `driveways.rs`, `buses.rs`, `crosswalks.rs`), keeping function names.

- [x] **M4.1 Vehicle, TrafficLights** · Sonnet · M2.1
  - *Done (2026-11-11):* `Vehicle`, `TrafficLights` and (early, from M5.1) `Crosswalks` ported. From `test_signals.gd`, the four tests without moving vehicles pass; the four with them are stubs for M4.3. Three more tests drive the light controller directly, with a car placed in a queue by hand and pedestrians waiting: resting green, minimum green, amber, then the waiting phase; walk light only early in the green. Workspace: 121 tests pass, 12 stubs.
  - *Read:* `sim/traffic/vehicle.gd`, `traffic_lights.gd`, `test_signals.gd`.
  - *Done when:* `Vehicle` is complete; `TrafficLights` passes the ported `test_signals` assertions that don't need moving vehicles (mark the others `#[ignore = "needs M4.3"]` and remove the ignore in M4.3).
- [x] **M4.2 VehicleSystem: spawning, following, movement, reroute** · Opus · M4.1, M3.1
  - *Done (2026-11-11), together with M4.3–M4.6:* the whole of `vehicle_system.gd` ported at once (its slices call into each other every tick), then each slice's tests. See NOTES ("VehicleSystem"). **The traffic is Godot's, tick for tick:** all five stress grids give exactly Godot's arrivals, gaps and stuck counts. Ported: `test_vehicles.gd` (7), `test_traffic.gd` (9), `test_junction_priority.gd` (5), `test_lane_changes.gd` (6), `test_driveways.gd` (7), and the 9 tests waiting for vehicles (signals, curves, road types). One GDScript test turned out not to test what it says (NOTES, "GDScript bugs found"). Workspace: 164 tests pass, 3 wait for citizens and saves.
  - The two-pass step, queues, `spawn`, car-following, stop lines, arrivals and failures, patience, reroute after road and lot changes, re-snap onto rebuilt lanes.
  - *Done when:* `test_vehicles.gd` and `test_traffic.gd` (the parts without junction control) are ported and pass, with `traffic_checks` every tick.
- [x] **M4.3 Junctions** · Opus · M4.2 (with M4.2)
  - Conflict zones and timing (`_conflicts_clear`), gap acceptance, `_must_yield`, look speed, don't-block-the-box, exit fairness, `_yield_in_junction`, lights (sneakers, turn window).
  - *Done when:* `test_junctions.gd`, `test_junction_priority.gd`, the rest of `test_signals.gd` and `test_traffic.gd` are ported and pass.
- [x] **M4.4 Lane changes** · Opus · M4.3 (with M4.2)
  - *Done when:* `test_lane_changes.gd` is ported and passes.
- [x] **M4.5 Driveways (depart, pull out, turn in)** · Opus · M4.4 (with M4.2)
  - *Done when:* `test_driveways.gd` is ported and passes.
- [x] **M4.6 StressTraffic, CongestionMonitor** · Sonnet · M4.3 (with M4.2; the congestion check on the Bottleneck comes with its citizens, M7.3)
  - *Read:* `stress_traffic.gd`, `congestion_monitor.gd`, `test_bottleneck.gd` (congestion parts).
  - *Done when:* congestion values on the Bottleneck fall within the baseline bands.
- [x] **M4.7 FreightSystem** · Sonnet · M4.5, M3.2
  - *Read:* `freight_system.gd`, `test_freight.gd`.
  - *Done (2026-11-11):* `test_freight.gd` ported (the save round-trip waits for M7.3). Same trucks as Godot: 25 in five midday game hours, 4 at night. `Scenarios.add_highway` ported early (`debug::scenarios`) for these tests.
- [x] **M4.8 Benchmark** · Haiku · M4.6
  - *Done (2026-11-11):* `cargo run -p tf_sim --release --bin traffic_benchmark`, unoptimised, on this M1, against GDScript (`baseline.md`): 500 vehicles 0.45 ms per tick (GDScript 4.93), 1 000 0.86 (10.53), **2 000 1.65 ms (26.50): 16× faster**, 5 000 3.27 (48.91). Same arrivals per second at every size. Go.
  - Port `benchmarks/traffic_benchmark.gd` to `cargo run -p tf_sim --release --bin traffic_benchmark` (same 20×20 grid, 500 / 1k / 2k / 5k vehicles, the same columns). Add the results table under this WP next to Godot's from `baseline.md`.
  - *Done when:* the table is filled in. This is the first go / no-go point: **if the Rust tick isn't at least 5× faster than GDScript at 2 000 vehicles, stop and ask the user** before going on.
- [ ] **M4.9 Optimisation pass (optional)** · Opus · M4.8, R2
  - Only if the user asks after M4.8: dense lane and vehicle arrays, parallelise the read-only prepare pass with `rayon`. Every ported test and the baseline bands must still pass.
- [x] **R2 Review gate** · Opus · M4.5
  - *Done (2026-11-11):* the five stress grids now assert Godot's exact arrivals and vehicle counts (`traffic.rs`, `road_type.rs`). A new probe, `migration/probes/traffic_trace.gd`, traces what they don't reach: a block removed, another retyped to an avenue and the first re-added under 250 vehicles with lights, and a game day of freight through driveways and the highway gate (`traffic_trace.json`). Every arrival, failure and give-up lands on the same tick as in Godot, and every vehicle ends on the same lane, distance and speed (`tests/traffic_trace.rs`; mutation-checked: an edit one tick late, another freight seed and a tripled import rate all fail it). Buses (bays, pulling out round a stopped bus) and the vehicle-side crosswalk rules can't run before M5 and M6, so I re-read them and the pass order of `step` against the GDScript: they match.
  - Same line-by-line review for `traffic/`. Run the stress grids for all road mixes from `baseline.md` and compare.

### M5: Walking · parallel-safe with M4.6–M4.8

- [ ] **M5.1 Crosswalks, WalkNetwork** · Sonnet · M3.1, M4.3
  - *Read:* `sim/walking/crosswalks.gd`, `walk_network.gd`.
  - *Done when:* the walk network matches the golden files.
- [ ] **M5.2 PedestrianSystem and the vehicle-side crosswalk rules** · Sonnet · M5.1, M4.5
  - *Read:* `pedestrian.gd`, `pedestrian_system.gd`, and the crosswalk parts of `vehicle_system.gd` (`_crosswalks_clear`, `is_crosswalk_busy`, `CROSSWALK_PATIENCE`), `test_walking.gd`.
  - *Done when:* `test_walking` is ported and passes, including the crosswalk safety check.

### M6: Transit

- [ ] **M6.1 BusStop, BusLine** · Haiku · M2.1
  - *Read:* `sim/transit/bus_stop.gd`, `bus_line.gd`. Data classes only.
- [ ] **M6.2 TransitSystem and buses in VehicleSystem** · Opus · M6.1, M5.2
  - *Read:* `transit_system.gd` (810 lines), the bus parts of `vehicle_system.gd` (dwelling, `set_route`, bays, `_leave_bays`, `_let_buses_out`, `_pass_stopped_buses`), `test_buses.gd`, `test_bus_lanes.gd`.
  - *Done when:* both test files are ported and pass; mid-block crosswalks match the golden files.

### M7: Citizens, Flow, scenarios, saves: the sim is complete

- [ ] **M7.1 Citizen, CitizenSystem** · Sonnet · M6.2, M4.7
  - *Read:* `citizen.gd`, `citizen_system.gd`, `test_trips.gd`, `test_commute.gd`, `test_daily_pulse.gd`.
- [ ] **M7.2 FlowMetric** · Sonnet · M7.1
  - *Read:* `flow_metric.gd`, `test_flow.gd`.
- [ ] **M7.3 Scenarios, Simulation save/load, Town** · Sonnet · M7.2
  - *Read:* `sim/debug/scenarios.gd`, `simulation.gd`, `test_simulation.gd`, `test_town.gd`, `test_bottleneck.gd`.
  - *Done when:* every file in `tests/unit/` that tests the sim has a Rust counterpart that passes (list them in a table under this WP: GUT file → Rust file → status); the Town runs a full day with safety checks every tick; every metric in `baseline.md` is inside its band. Report each metric next to Godot's.
- [ ] **R3 Sim parity gate** · Opus · M7.3
  - Review the complete `tf_sim` against `sim/`: every GDScript function has a Rust counterpart or a `NOTES.md` entry explaining why not. Re-run the benchmark with citizens (Town, plus stress 2 000). **The user signs off before the cutover work in M12.**

### M8: Game shell · can start after R1 and run in parallel with M2–M7 (only `tf_game`)

- [ ] **M8.1 Runner, snapshot, events** · Opus · R1 (use stub systems until M7 lands)
  - *Read:* `core/game.gd`, `core/view_snapshot.gd`, `core/events.gd`, `core/grid.gd`, `test_view_snapshot.gd`, `test_grid.gd`.
  - The threaded runner of §2.3, `ViewSnapshot::capture`, the Bevy events, `Grid`, speed controls, quicksave / quickload (F5 / F9, same JSON, to the platform data dir), headless inline stepping for tests.
  - *Done when:* ported snapshot and grid tests pass; a test steps a stub sim through the runner on a thread and gets interpolated snapshots; F5 then F9 round-trips.
- [ ] **M8.2 Camera and input** · Sonnet · M8.1
  - *Read:* `view/map_camera.gd`, the `[input]` section of `project.godot`, `tools/tool_manager.gd` (`TOOL_SHORTCUTS`).
  - Stepped zoom (`ZOOM_LEVELS`), pan (WASD / arrows / middle drag / trackpad), `KeyBindings` from physical keys, F11 fullscreen, `Pointer` resource.
- [ ] **M8.3 Headless screenshot mode** · Sonnet · M8.2
  - `--scenario <empty|bottleneck|bottleneck-bus|town> --frames N --screenshot path [--hour h] [--zoom z] [--overlay heatmap|lanes]`.
  - *Done when:* it writes a PNG of the Town; documented in `CLAUDE.md`'s migration section.

### M9: Simple-shape views · parallel-safe with each other once M8.3 is in (they need R3 only for real data; use a scenario from the stubs until then)

Scope: §2.3's flat shapes, nothing more. Read the GDScript renderer only to learn **what** it shows and **when** it updates (which events, which snapshot fields, incremental rebuilds), not how it paints. **Don't port** painters, shaders, grain, shadows, night lighting or the dot display. *Done when:* a Town screenshot shows the elements, readable at 0.5×, 1× and 3× zoom, and the Town at 2 000 stress vehicles holds 60 FPS on this Mac (state the frame time in the WP notes).

- [ ] **M9.1 Shape helpers and static map** · Sonnet · M8.3
  - `tf_game::draw`: build a mesh from a polyline with a width, a filled polygon, a rectangle, a circle.
  - Static views, rebuilt from change events by id: ground (one flat-coloured rectangle for the map), the grid while a snapping tool is active, roads (band + lane lines), lots (rectangles by zone colour), zones without buildings (outlines), highway markers.
  - *Read for behaviour only:* `view/road_renderer.gd`, `lot_renderer.gd`, `zone_renderer.gd`, `grid_renderer.gd`, `external_renderer.gd`.
- [ ] **M9.2 Moving things** · Sonnet · M9.1
  - Vehicles as rotated rectangles in one batched mesh, interpolated between `view_from` / `view_to`. Kind colours as in §2.3, plus F2 lateness colours (green → red, from `ViewSnapshot.lateness`). Pedestrians and waiting riders as dots.
  - *Read for behaviour only:* `view/vehicle_renderer.gd` (interpolation, `color_for_vehicle`), `pedestrian_renderer.gd`.
- [ ] **M9.3 Transit, lights, overlays** · Sonnet · M9.1 · parallel with M9.2
  - Bus lines as coloured polylines, stops as squares, a bar per stop line in its light colour. Debug overlays with gizmos: F3 lanes (`lane_overlay.gd`), F4 congestion heatmap (`heatmap_overlay.gd`).
  - *Read for behaviour only:* `view/transit_renderer.gd`, `intersection_renderer.gd`, `lane_overlay.gd`, `heatmap_overlay.gd`.

Not migrated (§1): `building_painter.gd`, `nature_renderer.gd`, `night_lights.gd`, `day_night.gd`, `road_markings.gd`, `alert_renderer.gd`, `dot_display.gd`, every `.gdshader`, and their tests (`test_nature`, `test_night_lights`, `test_road_corners`, `test_alert_renderer`, `test_dot_display`). List them in `NOTES.md` under "Left behind".

### M10: Tools and basic commands · parallel-safe with M9 (different files)

- [ ] **M10.1 ToolManager, BuildTool, RoadTool, BulldozeTool** · Sonnet · M8.2
  - *Read:* `tools/tool_manager.gd`, `build_tool.gd`, `road_tool.gd` (straight + curved, V; road type cycling with R), `bulldoze_tool.gd`, `tests/integration/test_build_tools.gd`.
  - Tools edit the sim only through the `Simulation` command wrappers (§2.4), and their previews come from `plan_road` / `plan_path`. Previews are drawn with gizmos (green / red).
  - *Done when:* the road and bulldoze tests of `test_build_tools.gd` are ported as headless Bevy app tests that drive `Pointer` and the actions.
- [ ] **M10.2 Zone, signal, bus line, route tools** · Sonnet · M10.1 (`zone_tool.gd`, `signal_tool.gd`, `bus_line_tool.gd`, `route_tool.gd`; the rest of `test_build_tools.gd`, except the palette, top bar, hint-position and debug-menu tests, which go away with the UI)
- [ ] **M10.3 Status line and debug keys** · Haiku · M10.1
  - The one text line of §2.3, fed by `ViewSnapshot` and the tool events. Every function the debug menu had (`ui/debug_menu.gd`) gets a key: keep the existing F-keys, and give the button-only ones free keys. List them all in `CLAUDE.md`'s migration section.
  - *Done when:* a headless app test presses each debug key and checks its effect (port the intent of `test_debug_menu_buttons_drive_the_debug_functions`).

- [ ] **R4 Game parity gate** · Opus · M9.3, M10.3
  - Play through `IMPLEMENTATION_PLAN.md`'s playtest instructions in both builds (Bottleneck, Town, every tool, every key) and list the differences, **ignoring the visuals and the UI left behind**. Fix or log each one. Then hand the game to the user to play.

### M12: Cutover (only after the user has played the Rust build and agreed)

- [ ] **M12.1 Remove Godot** · Sonnet · R4 + user sign-off
  - Tag the last Godot commit `godot-final`. Delete the Godot project (`*.gd`, `*.tscn`, `*.gdshader`, `*.uid`, `project.godot`, `addons/`, `data/*.tres`, `icon.svg` if unused, `.godot` ignore rules), plus `migration/export_golden.gd` and `run_baseline.gd`. Keep `migration/golden/*.json` as regression data used by the tests.
- [ ] **M12.2 Rewrite the docs** · Sonnet · M12.1
  - `CLAUDE.md`: commands (`cargo`, screenshot mode, benchmark), the architecture rules translated (§2.2–§2.4 replace the Godot-specific ones: `dispose()`, signal lambdas, `TileMapLayer`, GUT gotchas, `.uid` files), layout, code style (rustfmt, clippy, doc comments). `IMPLEMENTATION_PLAN.md`: engine line, §2.1 layers, §5 risks (performance resolved, with M4.8's numbers), "Defaults"; the polish track's art items and Phase D marked "left behind in the migration, to redo in Bevy". Update the design doc's architecture sections. Then mark this plan **Done**.

## 7. Dependency graph (summary)

```
M0.1 ─┬─ M0.2 ── M1.1 ── M1.2 ── M1.3 ── R1 ─┬─ M2.1 ─┬─ M3.1 ── M3.2 ─────────────┐
      └─ M0.3 (parallel)                     │        ├─ M4.1 ── M4.2 ── M4.3 ─┬─ M4.4 ── M4.5 ─┬─ M4.7 ── M7.1 ── M7.2 ── M7.3 ── R3
                                             │        │                         ├─ M4.6 ── M4.8 │
                                             │        │                         └─ M5.1 ── M5.2 ┴─ M6.2 ┘
                                             │        └─ M6.1 ───────────────────────────────────┘
                                             └─ M8.1 ── M8.2 ─┬─ M8.3 ── M9.1 ─┬─ M9.2 ─┐
                                                              │                 └─ M9.3 ─┤
                                                              └─ M10.1 ── M10.2, M10.3 ──┴─ R4 (also needs R3) ── M12
```

Critical path: M1.3 → M4.2–M4.5 → M6.2 → M7.3 (all Opus or close to it). The game shell (M8–M10) is small now and runs in parallel with the sim work.

## 8. Risks

| Risk | Mitigation |
|---|---|
| Behaviour drifts in a 1 666-line port | Sliced WPs, ported tests at every slice, `traffic_checks` every tick, golden data for everything RNG-free, review gates R1–R3 |
| RNG sequences differ, so tests tied to specific draws fail | Statistical bands (§3), assertions widened only with a `NOTES.md` entry |
| Godot signal timing (synchronous, mid-step) vs explicit dispatch | Decided once per emit site in M0.2 and recorded; listener order kept (§2.4) |
| Bevy breaking changes during the migration | Exact version pin; no upgrade until M12 is done |
| Without the art, problems are harder to see while testing | F2–F4 overlays and the status line are kept; screenshot mode (M8.3) for every visual check |
| Compile times slow agents down | `opt-level = 3` for dependencies only, `tf_sim` independent of Bevy (fast tests), optional `dynamic_linking` feature for dev runs |
| Agents "improve" things | Rule 2 in §4, review gates, a `NOTES.md` entry for every deviation |
