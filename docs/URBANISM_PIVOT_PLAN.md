# Urbanism Pivot — Long-Term Plan

**Status (2026-11-16):** approved by the user on 2026-11-16; it replaces [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) as the roadmap (the old plan stays as the record of the city builder). Phase 0 done (tag `city-builder-final`, docs updated, open points of section 12 on their defaults). Phase 1 done (space model, level and design files, test level drawn from its file; see the notes under Phase 1). Phase 2 done (cars: headless core, threaded runner, car view; see the notes under Phase 2). The replay hashes made on Windows are the same on macOS (checked 2026-11-16). Phase 3 done (2026-11-17: pedestrians, crossings, cars against pedestrians, the pedestrian view; see the notes under Phase 3). Direction changed by the user on 2026-11-17: realism first, fun later; each star of a level is a stage with more traffic (section 1, "Direction"). Phase 4 done and merged to `main` (2026-11-17: design, run, report, star stages, two playtests, two design measures; the user accepted the direction). Phase 5 done on branch `pivot-phase-5` (2026-11-17: the old city builder, its goldens and `migration/` are deleted; UI approach `bevy_ui` only). **Next: the last Phase 4 measure fixes (local street speed, yielding to intent), then Phase 6, cyclists.**

**Working title:** to be chosen (section 12). The crates keep the names `tf_sim` and `tf_game` until then.

## Progress notes

The newest entry is first. Each entry tells what was done, where, and what to do next. Keep only the entries that are still useful; the phase notes in section 8 are the record.

### 2026-11-17: Phase 5 done

**State.** Branch `pivot-phase-5` (from `main`, which holds Phase 4). The old city builder is deleted from `tf_sim` and `tf_game`, with its tests, its goldens and `migration/` (plan 2.3: retired in the same commit as their code). Lines of Rust 56 500 → 26 100. The street replay hashes did not change. Kept helpers moved to the street side: `geom::lanes` (lane offset and connector curves), `street::lanes::Turn`, `street::astar`. `cargo play` opens level 1. Bevy features `bevy_gizmos` and `bevy_gizmos_render` and the dependencies `indexmap` (tf_game), `approx`, `smallvec` are removed. `tf_game/src/bench.rs` is deleted; `cargo run -p tf_sim --release --bin street_benchmark` measures the sim cost per demand scale (scale 8 on level 1: 0.6 ms per tick, 85x real time). `scripts/check.sh` passes (17 min on a cold build). `cargo clean --profile dev` freed 39.6 GB.

**Measures for the checkpoint** (250 runs, 5 designs x 5 loads x 10 seeds; reports in the session scratchpad): the worse design (50 km/h, no zebras) is last at every load and breaks at stage 3; signals lower near misses at stages 1-2 and triple the walkers' wait. They found and fixed: the score did not count people who could not enter or finish; the walker near-miss band covered a whole road piece; a tiny speed step in the last metre before a junction was logged as a hard brake. Open: a car 1-3 m from a zebra brakes hard for a walker who only shows intent (fix in progress); the street speed is one value per lane (fix in progress: a local profile); a jam costs only delay points (Phase 8 decision).

### 2026-11-17: Phase 4 built on Windows, waiting for the user's playtest

**State.** Branch `pivot-phase-4` (from `pivot-phase-3`; not merged, not pushed). `scripts/check.sh` passes on Windows. Built by four agents in parallel (git worktrees), merged in the main session:
- Speed from the look of the street (§5.7 addition 1): `street/speed.rs` gives each road lane a street speed from width, side friction and obstacles near the edge. Wanted speed = `speed_factor * (c * min(limit, street) + (1 - c) * street)`, `c` = compliance, capped at 1.2 times the street speed. Replay hashes changed on purpose. A cause-chain gap was found and fixed (a driver kept only the first walker it had to yield to).
- Design commands `AddBollards` / `RemoveBollards` and `SetJunctionControl` with `SignalTemplate::TwoPhase` / `TwoPhaseWithWalk` (an all-pedestrian phase).
- Star stages in the level file (`stages`: demand scale and pass score), `street/score.rs` (score 0-100, hard fails, stage report with the incidents in words, `stars_for_design`), `run_level --stage n`.
- Play mode in `tf_game` (`cargo play -- --play carrefour-ecole`): Design / Check / Run / Report, tools zebra, speed limit, bollards, signals, undo, HUD, incident marks, stage choice, stars per design saved with the design.

**Measured** (empty design, 10 seeds, after the merge): stage 1 passes 10 of 10 (scores 74-95), stage 2 passes 5 of 10, stage 3 passes 3 of 10 and fails with the level seed. Over the 30 runs, most lost points are "serious near miss: car could not stop in time" (hard braking of 8 m/s², 92 of about 140 serious near misses), and at stage 3 drivers wait 95 s on average with priority to the right. The near-miss count looks too high for a 30 km/h school street: to check against reality before tuning (Phase 4 checkpoint: "is this credible?").

**After the first playtest (same day).** The near-miss excess had one cause: the braking applied (and logged) was the deceleration that reaches the target speed in one tick, cut at 8 m/s². Now each driver brakes with the deceleration needed over the real distance, each vehicle draws its own maximum (cars 7.5-9.5, vans 6.5-8.0 m/s², NCHRP 400 and ABS tests; `ROAD_GRIP` for weather later), braking builds up in 0.3 s, and one `stop_line()` (the painted zebra edge, found exactly) is used by drivers and by the careful walker's trust check. Over 30 runs: hard-brake near misses 158 → 60, mean stage scores 84 / 62 / 60 → 94 / 79 / 78; no injury before or after. The empty design now passes all three stages on most seeds; the user accepts it (section 6). Also: light Google-Maps palette, speed limits painted on the carriageway, tool previews, the agent inspector.

**After the second playtest (same day).** Rule-breaking walkers felt like a switch per person and crossed where nobody would. Now each rule is decided at each crossing from the traffic in view (section 5.3, "Pedestrian decisions"): everybody looks, attention (not risk) causes the bad gaps, red-light crossing depends on the cars in view and on others, zebra or not is chosen near the street, walkers cut the corner beside a zebra. Replay hashes changed on purpose. Over 30 runs (empty design): walker contacts 2 → 0, crossings off a zebra 15 → 3, corner cuts 7-16 % of crossings, mean stage scores 94 / 81 / 76 → 95 / 82 / 81. Tests: on red, 44 % of walkers go with no car and 15 % with cars; a zebra 40 m away is ignored by 26 % on an empty street and 3 % with cars. Also: the speed preview replaces the painted limit, a click on an incident line centres the map, arrows keep clear of zebras.

**Then:** the user plays level 1 several times (Phase 4 "done when").

### 2026-11-17: direction changed (realism first, stars are load stages)

**Decided by the user.** (1) The project is a simulation first and a game second: a believable result is more important than fun; the fun is tuned later. (2) A level is one specific crossing, from a simple street with one zebra to a complex roundabout and more. (3) The stars of a level are stages: each star adds more cars, pedestrians and cyclists, so the same design gets more and more load until it breaks. The plan is updated in sections 1, 3.4, 6, 8 (Phases 4 and 9), 10, 12 and 13.

**Also decided:** a star needs one design that passes every stage up to it (open point 10); an endless mode is only a later idea (section 11); the phase order stays as it is, so that the user can play with the simulation soon (Phase 4 next, calibration stays in Phase 8).

### 2026-11-17: Phase 3 done on the Mac

**State.** Branch `pivot-phase-3` (made from `pivot-phase-2`; not merged into `main`, not pushed). `scripts/check.sh` passes. The work was done in four stages (A walking, B crossings, C cars against pedestrians, D the view), each one reviewed by a second agent, and the review fixes are in their own commits. The record is under Phase 3 in section 8.

**Look at it:** `(caffeinate -u -t 120 &); sleep 1`, then `cargo play -- --level carrefour-ecole --run --frames 3300 --zoom 4 --screenshot <scratch>/run.png` (add `--population careless` for people who break rules). At the default framing a walker is 2 pixels wide: zoom in.

**Then:** Phase 4. Its two decisions are made (2026-11-17): addition 1 of section 5.7 (speed from the look of the street) is in Phase 4, and level 1 keeps its four default zebras.

### 2026-11-16: Phase 2 checked on the Mac

**State.** Branch `pivot-phase-2`. `scripts/check.sh` passes on the MacBook Air too (exit 0). The replay hashes made on Windows (`crates/tf_sim/tests/street_run.rs`, and the inline and threaded runner tests in `tf_game`) are the same on macOS, so no platform-independent math library is necessary now. If a hash differs between the platforms later, the cause is probably the float library (`sin`, `cos`, `exp`, `powf` come from the system): find the cause before you accept new values, and the choice of a library such as `libm` is a design decision for the user. `target/` was 7.1 GB after the check.

**Optional:** look at the cars (`(caffeinate -u -t 120 &); sleep 1`, then `cargo play -- --level carrefour-ecole --run --frames 3600 --zoom 1 --screenshot <scratch>/run.png`).

### 2026-11-16: Phase 2 done on the Windows laptop

**State.** `scripts/check.sh` passes on the Windows laptop (fmt, clippy, every test of both crates, goldens and replay hashes included). A screenshot with `--run` shows the cars in their style colours, in their lanes, pointing the way they drive.

**Building on the Windows laptop** (GNU Rust toolchain in the user profile, no admin rights, so no MSVC). Before any cargo command: `source ~/rust-gnu-tools/env.sh`. That folder is outside the repo and holds:
- Rust's own `dlltool.exe` (binutils 2.44) and an assembler `as.exe`. Do not use the `dlltool.exe` 2.47 in `~/mingw-as`: its import libraries miss the `_head_*_dll_imports_lib` symbols, and every Bevy executable fails to link. After you change the `dlltool`, `cargo clean -p` the crates that use raw-dylib (`windows-sys`, `windows*`, `getrandom`, `libloading`, `parking_lot_core`).
- `CARGO_PROFILE_DEV_DEBUG=line-tables-only`: with full debug info, the GNU toolchain makes each Bevy test executable about 2.3 GB, and Windows cannot load an executable over 2 GB ("not a valid Win32 application").
- `CARGO_BUILD_JOBS=6`: more jobs make the laptop lag.

A clean `scripts/check.sh` takes about 20 min there, and `target/` is about 18 GB after it.

**Lighter builds (done, 2026-11-16, user's decision).** Bevy has `default-features = false` and only the features the code uses (root `Cargo.toml`, one reason per feature); add a feature only when code needs it. No 3D, audio, glTF, gamepads, scenes, picking or Linux backends (`x11` / `wayland` must be added for a Linux build). All `tf_game` integration tests are one test executable, `crates/tf_game/tests/app/` (run one group with `cargo test -p tf_game --test app tools::`), so Bevy is linked once, not 12 times; tests that use the quicksave share `common::data_dir()` and `common::save_lock()`. On the Mac, the first build after this pull recompiles Bevy once.

**Then:** Phase 3 (pedestrians and crowds). Section 5.2 asks to confirm the 20 Hz tick there, with the crowd forces.

---

## 1. The new game

**Pitch.** You are an urban designer (*concepteur d'espace public*) in Paris. Each level is one specific crossing: from a simple street with one zebra to a complex roundabout and more. The buildings and the kerb-to-kerb space are fixed. You decide how the space is shared between cars, cyclists and pedestrians. Then you start the simulation and you can only watch. The people decide if your design is good, and each star puts more of them on your design, until it breaks.

**Direction (user, 2026-11-17): a simulation first, a game second.** For now the project is a realistic simulation of a street more than a game. When realism and fun do not agree, realism wins: behaviour, speeds, flows and injuries must be believable to a person who knows the street. The fun (pace, score, rewards) is tuned later, on top of a credible simulation, and must not make the simulation less true.

**The loop of one level:**

1. **Brief.** The level shows the place, the flows (how many cars, cyclists, pedestrians, from where to where, at which time of day), the constraints (for example: the bus line must pass, deliveries must be possible) and the stages of its stars.
2. **Design.** The player changes the use of the space and places equipment (section 4). The simulation is stopped. Ghost flows show the demand.
3. **Run.** The simulation plays the current star stage: one compressed day in four time slices, with the traffic of that stage. The player can pan, zoom, change the speed and pause. The player cannot edit.
4. **Report.** Result of the stage, list and map of incidents, replay of each incident with its cause. If the design holds, the star is won and the next stage (more traffic) opens on the same design. If it breaks, back to step 2.

**Stars are load stages (user, 2026-11-17).** Each star of a level is a difficulty stage. Star 1 has the base traffic. Each next star adds more cars, pedestrians and cyclists (and can change the mix), so the system that the player designed carries more and more load until it breaks. The number of stars tells how much load the design holds (section 6).

**Decisions made by the user (2026-11-15):**

| Subject | Decision |
|---|---|
| The old city builder | Replaced. It stays in git history behind a tag. |
| Loop | Free iteration: design, run, report, design again. |
| Player power on roads | Reallocation of space: the asphalt is fixed, its use is not (lane to bicycle lane or bus lane, one-way, turn bans, speed limit, wider sidewalk). No street closure in the first version. |
| Levels | Hand-made, simplified copies of real Paris places. One level is one specific crossing, from a simple street with a zebra to a complex roundabout (2026-11-17). |
| Realism or fun | Realism first; fun is tuned later on top of it (2026-11-17). |
| Stars | Load stages: each star adds traffic on the same design until it breaks (2026-11-17). |

**Score rules given by the user:** annoyed people, conflicts and light injuries decrease the score. A death is a game over. Free space, rest areas and green areas give bonus points with time.

**Design pillars:**

1. **Readable cause.** Each incident has a cause chain that the game can show ("the driver did not look, the cyclist was in the blind spot, the speed was 46 km/h"). Without this, a random death feels unfair and the player learns nothing.
2. **Design controls risk.** The player cannot control people, only what the space lets them do. A good design makes a fatal error almost impossible (low speed at conflict points, separation, short crossings). A bad design lets one careless person kill.
3. **Same design, same seed, same run.** A run is deterministic. The player can replay it, and a change of result comes only from a change of design.
4. **Real proportions.** Metres, seconds, real speeds. The old game compressed time and distance; this one cannot, because injury depends on real speed.
5. **Realism before fun.** A behaviour is added or tuned because real people do it, with a source when one exists, not because it makes the run more exciting. Game rules (score, stars) read the simulation; they never change how agents behave.

---

## 2. What the audit of the current code found

Four agents read the code (roads and routing, vehicles, people and city, the Bevy app). Their conclusions agree. Verdicts: **KEEP** as it is, **ADAPT** (keep the shape, change the content), **REWRITE** (keep the ideas and formulas, new code), **DROP**.

### 2.1 Five facts that shape the plan

1. **Collisions are impossible by construction.** `VehicleSystem::advance` clamps each step to `leader_gap - MIN_GAP` and `stop_gap` (`vehicle_system/mod.rs:673`). Junction entry needs `conflicts_clear`, `gap_accepted`, `crosswalks_clear` and `exit_has_room` together (`junctions.rs:173`), and `yield_in_junction` stops a car again inside the junction. Each safety rule is applied two times: by intent and by a physical guard. A "runs the red light" flag alone gives no collision; the car only waits without the light.
2. **All agents are 1D.** A vehicle is `lane + distance`. `lateral_offset` is for drawing only. A pedestrian is one point on a polyline, at one constant speed, with no radius. Pedestrians do not see each other: two people on the same path stay at the same coordinates.
3. **No agent has a personality, and the traffic uses no randomness.** Speed is the lane limit, braking and headway are global constants, gap acceptance depends on the turn only. `TrafficCtx` has no clock, so the time of day cannot change behaviour.
4. **The road model is closed.** `RoadType` is a fixed list of 6 embedded files with one `lanes_per_direction`, one `lane_width` and one `bus_lanes` flag. Lane access is one boolean (`Lane.bus_only`). Junction turns are generated from geometry (`classify_movement`), not authored. Signals are one mode for the full node, with two axis phases and optional left phases (`assign_signal_phases`), so a place with 5 or 6 arms cannot have a correct plan. Crossings are derived (junctions of 3+ roads) or owned by bus stops; the player cannot place one. There is no polygon, no obstacle, no area.
5. **The shell is in good condition.** The threaded runner, the snapshot interpolation, the batched meshes, the `BuildTool` framework, the camera, the screenshot mode, the benchmark mode and the headless test harness all fit the new game. The UI is one text line: all menus and screens are new work.

### 2.2 Verdict per module

**`tf_sim`**

| Module | Verdict | Notes |
|---|---|---|
| `geom/` (`Curve2D`, `gd`, `segment`, `rect`) | KEEP | Add polygons (areas, footprints). `geom/sort.rs` (Godot unstable sort) goes with the goldens. |
| `rng.rs` (`SimRng`, PCG32) | KEEP | Add one stream per agent, seeded from `(run seed, agent id)`, so that one more agent does not shift the draws of the others. |
| `events.rs`, `Simulation::call`, `Outbox` | KEEP | Good seam. New events: conflict, contact, injury, annoyance, run ended. |
| `clock.rs` | ADAPT | Real time scale. Time slices come from the level. |
| `routing/astar.rs`, `route.rs` | KEEP | Used for lane routes and the walk graph. |
| `routing/lane_graph.rs`, `router.rs` | ADAPT | `bus: bool` becomes a vehicle class. Routes are fixed per origin-destination pair in a level, so the congestion rerouting goes. |
| `roads/lane.rs`, `lane_builder.rs` | ADAPT | Keep offset, trim, connector curves and conflict-zone extraction. Lanes come from strips with their own width and access mask. |
| `roads/road_network.rs`, `road_type.rs`, `road_node.rs` | REWRITE as the space model (section 3) | Keep nodes, segments, connectors and the command + `ChangeSet` pattern. Drop splitting, planning and grid rules: levels are authored. |
| `traffic/traffic_lights.rs` | REWRITE | Keep the actuation ideas (minimum green, extension, skip of empty phases). New: authored signal plans with signal groups, per approach, for pedestrians and cyclists too. |
| `traffic/vehicle_system/*`, `vehicle.rs` | REWRITE | Keep the formulas: following speed `sqrt(v_lead² + 2·b·gap)` with a time headway, critical gaps per turn, gap shrink with waiting time, priority order, "do not block the box". They become what a *careful* driver does, not what the world enforces. |
| `walking/*` | REWRITE | Keep the walk graph idea for global paths and `MidBlockCrossing` as the shape of a placed crossing. Movement becomes a 2D crowd model. |
| `debug/checks.rs` | ADAPT | The three detectors become the base of the live conflict detection. |
| `debug/scenarios.rs` | ADAPT | Becomes micro-scenarios for tests. Levels are data. |
| `save.rs` | ADAPT | A save is `level id + design + progress`. New format, new version. |
| `city/*` (lots, zones, citizens, parking, externals, Flow), `transit/*`, `traffic/freight_system.rs`, `stress_traffic.rs`, `congestion_monitor.rs`, driveways, bus bays | DROP | `StressTraffic::try_spawn` is the model for the spawner. `FlowMetric` is the model for "score at the end of a trip". Buses come back later as scripted vehicles on a fixed path (section 11). |

**`tf_game`**

| Module | Verdict | Notes |
|---|---|---|
| `runner.rs` | ADAPT | Relay the new events (today it drops arrivals and failures). The run phase needs a clock that does not depend on the frame rate (section 7.4). |
| `snapshot.rs` | ADAPT (largest change) | Per agent: kind, pose, body, state (waiting, annoyed, breaking a rule, hurt). Incident markers. Score. |
| `draw.rs`, `camera.rs`, `views/mod.rs`, `views/tool_preview.rs`, `main.rs`, `bench.rs`, `scripts/`, `.cargo/` | KEEP | Camera: frame the level bounds, not the 128×128 map. |
| `views/moving.rs`, `roads.rs`, `lights.rs`, `ground.rs`, zebra of `transit.rs` | ADAPT | Batching stays. People need a body and a heading, not a dot. |
| `tools/mod.rs` (`BuildTool`, `ToolManager`) | KEEP | New tools use it. Templates: `bus_line.rs` (click to place, variants), `zone.rs` (dragged area), `bulldoze.rs` (remove), `signal.rs` (pick a junction). |
| `tools/road.rs`, `route.rs`, `bus_line.rs`; `views/grid.rs`, `lots.rs`, `zones.rs`, `externals.rs`, `overlays.rs` | DROP | The lane overlay can stay as a debug aid. |
| `input.rs`, `events.rs`, `plugin.rs`, `game.rs`, `status.rs`, `capture.rs` | ADAPT | New actions, messages, a `Phase` state, levels in place of scenarios. |
| `tests/` | ADAPT | Keep `common::Harness`. `playtest_parity` and the tests of dropped tools go. |

### 2.3 Golden tests

The Godot goldens (`migration/golden/*.json`) pin the behaviour of the old game. The pivot changes that behaviour on purpose, so they are retired **with the code they pin, in the same commit, and the commit says so**. They are not weakened and not re-pinned. The new safety net is in section 9: replay hashes made by our own simulation.

---

## 3. The space model (replaces road types)

The level is in **metres**. One level is about 150 m × 150 m.

### 3.1 Streets are cross-sections of strips

A street segment has a centerline (as today) and a **profile**: an ordered list of strips from one building line to the other.

```
building | sidewalk 3.0 | parking 2.0 | car lane 3.0 → | car lane 3.0 ← | bike lane 1.5 ← | sidewalk 2.5 | building
```

Each strip has: a **use** (sidewalk, car lane, bus lane, bicycle lane, bicycle track, parking, delivery bay, median, planting, terrace), a **width**, a **direction** for lanes, an **access mask** (which agent classes can use it by the rules), and a **separator** on each side (none, paint, kerb, bollards, planter).

- The total width between the building lines is fixed by the level. **Reallocation is editing this list**: change a use, move a boundary, change a direction. This is the core mechanic.
- `lane_builder` makes a `Lane` for each travel strip (offset = sum of the widths before it). Lanes now have their own width.
- A separator is what makes a bicycle *track* different from a painted *lane*: paint can be crossed by a careless driver, a kerb cannot.
- The level can lock strips (for example a bus lane that must stay).

### 3.2 Junctions are authored

- A junction has a **polygon** (the conflict area) and **movements**: from an incoming lane to an outgoing lane, with a class mask. The level gives the default movements. The lane builder still draws the connector curves and extracts the conflict zones.
- When the player changes strips, the movements are generated again from rules (a helper based on the current `classify_movement`), then the turn bans of the player are applied.
- **Default rule: priority to the right**, as in France. Signs (yield, stop) and signals change it per approach.
- A roundabout is a ring of one-way segments with yield on entry (modern rule) or priority to the right (the rule of Place de l'Étoile).

### 3.3 Walkable space and areas

- **Walkable polygons**: sidewalks (from the strips), plazas and islands (from the level), crossings (from the player). Pedestrians move freely inside them.
- **Obstacles**: circles and polygons (bollards, planters, trees, benches, terraces, parked vehicles). They block bodies and, for the tall ones, the line of sight.
- **Areas**: green areas and rest areas are polygons with a kind. They use space that agents cannot use, and they give the bonus.

### 3.4 Level file

One RON file per level in `crates/tf_sim/data/levels/`, embedded with `include_str!` like the road types today:

- metadata (name, place, short history text, difficulty), bounds, building blocks (drawing only);
- nodes, segments (centerline + default profile + locks), junctions (polygon, default movements, default control);
- fixed furniture (metro exits, existing trees, a bus stop);
- **gates** (where agents enter and leave) and the **demand**: per time slice, per mode, an origin-destination table with counts, plus the mix of archetypes;
- constraints, the allowed tools, the seed;
- the **star stages**: for each star, the load on top of the base demand (a scale per mode, and optionally a different archetype mix, for example more careless people) and the pass conditions of that stage (section 6). The headless `run_level --demand-scale` is the first form of this.

The **design** of the player is a separate, small file: the list of edit commands on top of the level. Loading a design replays its commands. This also gives undo and redo.

---

## 4. The player toolbox

Each item must have a visible effect in the simulation, or it does not ship. Items are unlocked level by level.

| Family | Items | Simulation effect |
|---|---|---|
| **Reallocate** | Strip use (car, bus, bicycle lane, bicycle track, parking, delivery bay, planting, terrace); strip width; direction (one-way, contraflow bicycle lane); kerb extension at a corner | Capacity per mode, separation, crossing length, turn speed |
| **Rules** | Speed limit (50 / 30 / 20 "zone de rencontre"); turn ban; yield and stop signs; bicycle box at lights (*sas vélo*); "cyclists can pass the red light to turn right" sign (M12) | What careful agents do; where conflicts move |
| **Signals** | Signal per approach from plan templates (2, 3, 4 phases, protected left, all-pedestrian phase); green times in steps; pedestrian and bicycle signals; push button | Delay against safety; red-light running grows with long waits |
| **Crossings** | Zebra; raised crossing; refuge island; mid-block crossing | Where pedestrians cross by the rules; car speed; crossing in two steps |
| **Obstacles** | Bollards, barriers, lane separators, speed cushion, planters | Stops illegal parking and corner cutting; decreases speed; can block sight and the path of wheelchairs |
| **Life** | Trees, green area, benches, fountain, bicycle racks | Bonus points; rest for slow walkers; uses space |

**Limits on the player:** the fixed width is the main budget. Each level adds constraints (minimum sidewalk width, a path for the bus, access for emergency vehicles, delivery capacity). A money budget is not planned (section 12).

**Paris-specific trouble that the toolbox must answer** (they come from archetypes, section 5): delivery vans that double-park when there is no bay, cars that stop on the bicycle lane, scooters, tourists in groups, cyclists who do not stop at red lights, pedestrians who cross anywhere when the detour to the zebra is long.

---

## 5. The simulation core

The old model moves queues along lanes and makes errors impossible. The new model simulates **bodies that make decisions with incomplete information**. It is new code in `tf_sim` (module `street/`), written next to the old systems, which stay frozen until the switch (section 8).

### 5.1 One loop for all agents

Each tick, for each agent, in id order, reading the state of the previous tick (so the order of the agents does not change the result):

1. **Perceive.** What is in the field of view and not hidden by an obstacle or a large vehicle. A percept arrives after the reaction time of the agent. Attention can fail (a look not made, a phone).
2. **Decide.** The careful behaviour gives a target (speed, path, wait or go). Then the personality can override it: accept a shorter gap, go at amber or red, cross out of the zebra, exceed the limit.
3. **Act.** Acceleration and steering inside the physical limits of the mode.
4. **Integrate** the bodies. Then **detect** conflicts and contacts with swept shapes (a car moves 0.7 m per tick at 50 km/h, so a simple overlap test is not sufficient).
5. **Report** events with their cause chain.

### 5.2 Bodies and movement per mode

| Mode | Body | Movement |
|---|---|---|
| Car, van, bus, truck | Oriented rectangle | Follows a lane path. Real lateral position inside the lane (it gives room to a cyclist, it cuts a corner). Car-following from the old formulas with per-driver parameters. Looks at all agents near its path, not only the leader in a queue. |
| Cyclist, scooter | Narrow rectangle | Follows a path with a free lateral position in a corridor (the lane, or the edge of a car lane). Can overtake, filter between stopped cars, wait in the bicycle box. A careless one can leave the corridor: sidewalk, wrong way, red light. |
| Pedestrian | Circle | Free 2D movement in walkable polygons. Global path from the walk graph (A*). Local movement from a social-force model with anticipation: goal force, avoidance of people and obstacles, groups that stay together. Density decreases speed. Outside the walkable space (jaywalking) the same model applies with gap acceptance against vehicles. |

A uniform spatial grid gives the neighbours. Target: 400 agents at the same time.

**Tick rate:** 20 Hz stays as the default. Phase 2 measures if contacts and crowd forces are stable at 20 Hz; if not, the tick goes to 30 Hz. This is decided one time, before any tuning.

### 5.3 Personality

Each agent gets a profile at spawn from its archetype and its own random stream.

| Trait | Effect |
|---|---|
| Desired speed | Factor on the limit (cars) or own speed (others) |
| Patience | How quickly waiting becomes frustration |
| Compliance | Probability to obey a rule when it costs time |
| Attention | Probability that a check is made; phone use |
| Reaction time | Delay between percept and action |
| Risk tolerance | Size of the accepted gap |
| Courtesy | Lets others go, keeps crossings clear |

**Dynamic state:** *hurry* (set by the time slice and the archetype) and *frustration* (grows while the agent waits or is blocked, decreases slowly). Frustration decreases compliance and the accepted gap. This is the link between "annoyed" and "dangerous", and it is the reason why a design with very long red lights is not safe.

**Pedestrian decisions (Phase 4 playtest, user 2026-11-17).** Breaking a rule is not a switch per person: a person decides each rule at each crossing, from what they see then. All random values are still drawn at spawn; only the chance they are compared with depends on the situation.
- *Everybody looks.* The accepted gap is never below the crossing time; risk and frustration only make the margin smaller. A small chance that grows as attention falls makes a walker judge from a view 1.5 s old at that kerb (phone, talk, children): this, not risk, is the main cause of a walker stepping in front of a car.
- *Red light.* The chance to go depends on the person's red habit, on whether a vehicle comes and how soon (with no car in view most Parisians go), on the time waited, and on the people nearby who already cross (people follow others; groups wait more: Rosenbloom 2009, Transportation Research Part F 12(5)).
- *Zebra or not* is decided near the street, from the detour, the zebra habit and the traffic in view: on an empty street more people cross directly, with cars coming more walk to the zebra. With no crossing within 50 m, crossing anywhere is legal (R412-37).
- *Corner cutting.* A walker may step onto the road up to 2.5 m beside a zebra and cross on a diagonal; it still counts as crossing at the zebra, and drivers yield to it.
- *One habit per rule* (red, zebra, corner): compliance plus a small shift per archetype, so one person can follow one rule and bend another. All values are start values for Phase 8.

**Archetypes (first list, tuned later):**

- Cars: resident, commuter, taxi / VTC, delivery van (stops where it can), bus (scripted path, careful), scooter.
- Cyclists: Vélib tourist (slow, hesitant), commuter (fast, knows the place), delivery rider (fast, low compliance), cargo bike or parent with child (wide, slow, careful).
- Pedestrians: commuter, tourist group, old person (slow, needs rest), child, parent with stroller, wheelchair user (needs width and lowered kerbs), runner, person on the phone.

### 5.4 Time of day

A run is one day in **four slices**, each about 60 to 90 s of play at speed 1:

| Slice | Flows | Behaviour |
|---|---|---|
| Morning rush | Maximum commuters, school children | High hurry, medium compliance |
| Midday | Deliveries, tourists, lunch crowds | Double-parking, slow groups |
| Evening rush | Maximum everything | Hurry and frustration, low light in winter |
| Night | Few agents | High speeds, low compliance, decreased visibility, decreased attention |

The level gives the flows and the modifiers per slice. A design must hold in all four.

### 5.5 Conflicts, contacts, injuries

- **Conflict (near miss).** Two agents on crossing paths with a time to collision below a threshold, or a post-encroachment time below a threshold (these are the standard measures of traffic conflict studies). An emergency brake or a swerve counts too. Graded: light, serious.
- **Contact.** Swept bodies touch. Severity comes from the impact speed and the two modes. Pedestrian against pedestrian is a bump (annoyance). Cyclist against pedestrian is usually a light injury. A motor vehicle against a person: the risk of death grows steeply with speed between 30 and 50 km/h; the curve will be set from published road-safety data during Phase 2.
- **Outcome** is drawn from the severity with the run seed: no injury, light injury, serious injury, death. A serious injury is a large penalty. **A death stops the run.**
- After a contact, the agents stop for some time and block the space. An injured person is removed after a delay (a simple marker, no gore, see section 12).
- **Cause chain.** Each event records who broke which rule, who did not perceive whom and why (hidden by a van, no look, too fast to stop). The report and the replay use it.

### 5.6 Annoyance

One unit for all modes, so that the score can add them:

- delay against the free trip time, weighted by the patience of the agent;
- blocked or squeezed: a pedestrian in a dense crowd, a cyclist behind a parked van, a wheelchair with no possible path (a strong penalty);
- forced detours;
- an agent who gives up its trip.

### 5.7 Realism improvements (proposed 2026-11-16)

The personality of 5.3 is in the code since Phase 2 (seven traits per agent, drawn from an archetype, plus frustration). These additions would make behaviour depend on the street and on the situation, not only on the person. Each one changes behaviour, so each one is the user's decision when its phase starts. The order is the order of value for the game.

| # | Addition | What it changes | Phase |
|---|---|---|---|
| 1 | **Speed from the look of the street** (accepted by the user on 2026-11-17) | The wanted speed is today a share of the speed limit. Real drivers go slower in a narrow lane, between parked cars, near trees and obstacles, on a raised crossing, and faster on a wide, empty street, whatever the sign says. The wanted speed gets a factor from the lane width, the side friction (what is next to the lane) and the sight distance. This gives the player a second tool for speed: geometry, not only the sign. | 4 |
| 2 | **Attention with a direction** (accepted by the user on 2026-11-17, "with a realistic field of view") | Attention is today one probability that a look is made. In real crashes the driver looks, but at the wrong place: a driver who turns left looks at the oncoming cars and does not see the pedestrian on the exit crossing. A driver gets a gaze direction that its task sets, and sees an unexpected kind of road user (a cyclist, a child) less well than a car. The cause chain can then say "looked left for a gap, did not look at the zebra". | 6 |
| 3 | **Reaction time that depends on surprise** (accepted by the user on 2026-11-17) | The reaction time is today one fixed value per agent. A driver reacts in about 0.7 s to an expected event and in about 1.5 s to a surprise (a person who comes out from behind a van). The reaction time gets a factor for unexpected events. | 6 |
| 4 | **Imitation** | A pedestrian crosses on red much more often when another person goes first. A driver follows the vehicle in front through an amber light. Compliance gets a factor from what the neighbours do. One careless person can then lead a group into danger. | 8 |
| 5 | **Linked traits** | The seven traits are drawn independently, so a driver can be very fast and also very careful with gaps. One hidden "aggressiveness" value per agent pulls the speed, the risk, the compliance and the courtesy in the same direction. | 8 |
| 6 | **Hurry** | Section 5.3 has it (set by the time slice and the archetype), but the code does not yet. | 8 |
| 7 | **Hearing** (asked by the user on 2026-11-17) | Agents perceive only by sight today. People also hear traffic: a walker hears a car that a parked van hides, or a car that comes from behind; a driver hears a horn. Each vehicle makes a sound that grows with its speed and depends on its kind (a diesel van is loud; an electric car below about 20 km/h, a bicycle and a scooter are almost silent, a known danger for pedestrians); the street has a background noise that grows with the traffic and hides the quiet ones; some walkers do not hear (headphones, a phone call, hearing loss). The cause chain can then say "did not see the car behind the van and did not hear it: electric, 15 km/h". Later, the noise level of the street can also count in the serenity part of the score (section 6), so that calm streets get a bonus. | 6 |

Smaller ideas, not planned: familiarity with the place (a tourist hesitates; a resident does not look because nobody ever comes from there), and vehicle differences (the braking distance of a van against a car).

**Field of view (user, 2026-11-17).** Addition 2 uses a realistic field of view, not a cone that sees all or nothing: the human visual field is about 180° wide, but a thing far from where the person looks is noticed less often and later (the useful field of view, smaller with age, load and hurry); eyes and head move in glances of a fraction of a second, so a person sees one direction at a time; a vehicle has its own blind spots (pillars, and the large blind spot on the right of a van or a bus that mirrors do not cover), so a driver or a cyclist must turn the head to see them. Hearing (addition 7) covers what the eyes do not.

Additions 2, 3 and 7 are with the cyclists because the blind-spot conflict of Phase 6 needs them, and silent bicycles are the main case for hearing. Addition 4 is partly in Phase 4 since the second playtest: walkers who see others cross on red go more often (section 5.3). Additions 4 to 6 are with the calibration because they need real figures. All of them change the replay hashes.

---

## 6. Score

The numbers below are start values for tuning.

- **Safety.** Each conflict and each injury removes points by severity. A death: run failed, no stars.
- **Serenity.** Mean annoyance per mode. The worst mode counts double, so the player cannot get a good score by sacrificing one group.
- **Life (bonus).** Points per second of the run for: green area, free pedestrian space per person at the busy moments, rest places that people use, shade. The bonus cannot be more than a set part of the total, so that a park with a deadly road next to it does not win.
- **Constraints.** A broken level constraint (the bus cannot pass) fails the run with a clear message, before or during the run.
- **Stars (load stages, user decision 2026-11-17).** Each star is a stage of the level with more traffic than the one before (section 3.4). A stage is passed when the run has no death, no serious injury, no broken constraint, and its score is at or above the threshold of that stage. Passing a stage gives its star and opens the next stage on the same design. A failed stage ends the progress of that design at the stars already won: the stages show the load at which the design breaks, and the report tells why (which conflict point, which mode, which slice).
  - A star belongs to a design (user, 2026-11-17): to have 3 stars, one design must pass stages 1, 2 and 3. When the player changes the design, the lower stages are run again headless before the current stage, so the stars always describe the design that is on the map.
  - The number of stages per level is level data (3 by default).
  - Difficulty is not tuned yet (user, 2026-11-17): with real braking, the empty design of a simple crossing passes all its stages on most seeds, and that is realistic. How stages become hard (load, mix, time of day, several seeds per verdict) is decided later; the stage scales are not adjusted to force failures.
  - Jams (measured 2026-11-17): every person of the demand is scored; one who cannot enter or does not finish counts as a give-up. A jam therefore costs only delay points: on level 1 the empty design still passes at x6 load while about 60 % of the cars never get through. Whether a jam is a hard fail (for example more than 10 % of the demand not served) or a heavier cost is decided in Phase 8 (user, 2026-11-17).

The score is a measure of the simulation, not a reward system that changes it: with realism first (section 1), the weights are set to agree with what a road-safety expert would say about the design, and tuned for fun only later.

**Fairness.** A run is deterministic, so a death is always reproducible and explained. From Phase 9, an **impact study** runs the design many times headless with different seeds (the headless sim must be fast, section 9) and gives a risk estimate ("1 run in 20 has a serious injury at the north crossing"). The stars can then ask for a risk level, not for one lucky run.

---

## 7. The game shell (`tf_game`)

### 7.1 Phases
A `Phase` state: `Menu`, `Brief`, `Design`, `Run`, `Report`. Tools work only in `Design`. The sim steps only in `Run`. `SimHandle::replace` already swaps a simulation and resets the views; a run builds a new `Simulation` from `level + design + seed`.

### 7.2 Editing
New tools on the `BuildTool` framework: strip editor (click a strip, cycle its use; drag a boundary), direction, speed zone, signal plan, signs, crossing, obstacle (point or line), area (dragged polygon), eraser. All edits are commands on the design, so undo / redo and the save come for free. A validation pass shows broken constraints in the design phase.

### 7.3 Views
Flat shapes stay (the art direction is a later decision). **Target look (user, 2026-11-17):** the Google Maps street style: light palette (blue-grey carriageways, white zebra bars, light kerb-side sidewalks, beige blocks, green planting), rounded kerb corners, islands and detailed crossings. The palette comes in Phase 4; rounded kerbs and islands need street geometry and come with kerb extensions (Phase 7) and the art pass (Phase 10). Every rule the player sets must be visible on the street: a speed limit shows as a marking on its segment. New: strips and separators, junction markings, signal heads per approach with pedestrian and bicycle lamps, people with a body and a heading, cyclists, agent state (a waiting pedestrian, a frustrated driver, an agent that breaks a rule), incident markers that stay on the map, ghost flows in the design phase, a heat map of conflicts in the report.

### 7.4 Run clock
The threaded runner stays. The result must not depend on the frame rate: the sim is already tick-based, so the rule is that **nothing from the frame (real delta, batch size) enters the sim**, and a test proves that a run through the threaded runner and a headless run give the same event log.

### 7.5 UI
All new (today: one status line). Needed: level select, brief panel, tool palette with unlocks, run HUD (time slice, live score, counters, speed), incident feed (click to go there), report screen with stars, incident list and replay, pause menu, settings. Decided (user, 2026-11-17, start of Phase 5): **`bevy_ui` only**, no added UI library. The play mode already uses it (panels, clickable incident rows); it adds no dependency to keep in step with each Bevy version, and it can look like a game, not a form.

### 7.6 Replay
A replay is `level + design + seed` and a tick range. To replay an incident, the game runs the sim again headless to a few seconds before the event and plays from there. No recording format is necessary.

---

## 8. Roadmap

Each phase ends with a state that can be played or measured, and with `scripts/check.sh` at 0. Sizes: S (days), M (one to two weeks), L (more), for one developer with an assistant.

### Phase 0 — Preparation (S)
- Tag the last commit of the city builder (`city-builder-final`).
- Approve this plan. Update `CLAUDE.md` (goal, rules that change: units, goldens, determinism) and set the status line of the old plan to "replaced".
- Decide the open points of section 12 that block Phase 1 (title is not one of them).

**Done when:** the docs describe the new goal and the tag exists.

### Phase 1 — Space model and first level (M)
- `street/space`: strips, profiles, junctions with movements, walkable polygons, obstacles, areas; polygons in `geom`.
- Level file format and loader; design file as a command list; save and load of a design.
- Lane builder on strips (own width, access mask). Conflict zones as today.
- A test level "Carrefour école": a 4-arm crossing of two small streets with a school (fictional, for tests and the tutorial).
- Views: draw a level from its data. Camera framed on the level. `--level <id>` in screenshot mode.

**Done when:** a screenshot shows the test level from its file; a strip change through a command rebuilds lanes and movements; a round trip of level + design is identical; the old game still runs.

**Done (2026-11-16).** Code: `tf_sim::street` (`space`, `level`, `design`, `lanes`, `model`), `geom::polygon`, `data/levels/carrefour_ecole.ron`; `tf_game::level` and `views::street`; `--level <id>` in screenshot mode. Tests: `tf_sim/tests/street_space.rs`, `tf_game/tests/app/street_view.rs`. Choices made where this plan was open or did not fit the freeze:

- The old `roads/lane.rs` and `lane_builder.rs` (ADAPT) are not edited while the old game is frozen. The street model has its own `StreetLane` (with width, access mask and lateral position) and calls the `lane_builder` helpers as they are. The adaptation in place waits for Phase 5.
- Movements are always generated from rules, also for the level's defaults: the level gives only default **turn bans** (and signs, control). Rules per lane group (bicycle, bus, general): straight keeps its position, right from the rightmost lane, left from the leftmost; a group with no lane on the next arm falls back (bicycle to general or bus lane, bus to general, general to a bicycle lane for its cyclists only). The access of a movement is what both lanes allow, minus the bans. No U-turns.
- Lanes stop where the centerline leaves the junction polygon. The walkable corners between sidewalks are authored as plazas in the level file (no automatic corner geometry yet).
- The `Street` is not in `Simulation` yet (no tick, no `Outbox`): `Street::apply` returns a `StreetChange` and bumps `version`. Phase 2 builds the run sim around it.
- Views draw the level at `PX_PER_M` = 8 world pixels per metre, so the camera's zoom steps stay as they are. The screenshot of a level still shows the old status line (on an empty old map).
- The level format has gates and demand per slice, but no archetype mix yet (Phase 2 / 8); gates are not drawn.

### Phase 2 — Agent core: cars (L)
- Bodies, the perceive / decide / act loop, per-agent random streams, the spatial grid.
- Careful car behaviour: following, priority to the right, signs, signals (new signal plans), gap acceptance, no blocking of the junction.
- Personality traits and overrides for cars. Frustration.
- Spawner from the level demand. Conflict and contact detection with swept shapes, severity, cause chain, events.
- Decide the tick rate (5.2). Set the severity curve from road-safety data.
- Headless binary `run_level` (level, design, seed → event log and score parts).

**Done when:** (a) a population with full compliance and attention has zero contacts over 50 seeds on the test level; (b) a careless population produces conflicts, and each contact has a cause chain; (c) the same seed gives the same event log hash, in two runs and on the threaded runner; (d) cars visibly drive with different styles.

**Done (2026-11-16).** All four criteria hold.

- Code: `tf_sim::street::run` (`agent`, `drive`, `spawn`, `conflicts`, `detect`, `events`, `route`, `grid`), `street::signals`, and the binary `run_level` (`cargo run -p tf_sim --release --bin run_level -- <level> [--design f.json] [--seed n] [--population careful|mixed|careless] [--demand-scale x] [--events]`). Tests: `tf_sim/tests/street_run.rs` and unit tests next to the code.
- In `tf_game`: `street_runner.rs` (`StreetRunner`, a Bevy resource) runs a `StreetRun` on one worker thread with the design of `runner::SimHandle` (the run moves by value over channels, batches of at most `MAX_TICKS_PER_BATCH`, `LEAD_TICKS` ahead, a `StreetSnapshot` after each tick, `view_from` / `view_to` / `view_alpha`, `threaded: false` runs inline). The frame delta only moves the render clock, which sets how many whole `step()` calls run (section 7.4). `views/agents.rs` draws each car as an oriented rectangle with a dark front mark, in one batched mesh, coloured by archetype (careful blue, resident green-grey, commuter grey, taxi yellow, delivery white, careless red; darker while waiting, magenta when crashed). Screenshot mode: `--level <id> --run [--seed n] [--population careful|mixed|careless]`. The old game's code got wiring lines only.
- Done when:
  - (a) holds: a careful population has zero contacts over 50 seeds on the test level, also at 4 times the demand and under signals.
  - (b) holds: a careless population has conflicts and contacts, and every contact seen has a cause chain (30 seeds at 3 times the demand).
  - (c) holds: the same inputs give the same event log hash; four replay hashes are stored in `street_run.rs`; an inline and a threaded `StreetRunner`, with uneven frame deltas and speed changes, give the same hash as a headless run (`tf_game` test `street_runner`).
  - (d) holds: top speeds differ by more than 1 m/s in a mixed run, and the car view shows each style in its own colour.
- How it works: each agent decides from a snapshot of the world as old as its reaction time, and from its own current state. Decisions are collected before any agent moves. A driver follows with the old following-speed formula, with the reaction time taken out. It stops at the line unless the junction check says go, and it brakes for any body in its path. The junction check uses the old critical gaps (2.0 s straight, 1.5 s right, 2.5 s left), then signs, then priority to the right, then "do not block the box". The personality bends these rules: not looking at an approach, not yielding, running a late red, accepting a shorter gap, or looking away for 1 to 2.5 s. When the front of a car passes the line, the careful check runs again. What it would have refused becomes the causes.
- Conflicts: post-encroachment time under 1.5 s where two movements cross (serious under 0.75 s), and braking harder than 4.5 m/s² for someone (serious from 7 m/s²). Time to collision is not measured yet. Contacts: oriented rectangles tested at three moments of each tick.
- Signals: fixed-time plans (`SignalPlan`, with phases of approach segments, amber and all-red), taken from the level's junction or from a default made from the arms. No actuation, and no pedestrian or bicycle lamps yet.
- Severity: the pedestrian death curve (Rosén & Sander 2009) and the serious injury curve (Tefft 2011) are in `events.rs` for Phase 3. For car against car, no published table by speed was found for city speeds. The table is anchored on three points of Lubbe et al. 2024, and the other values are our own starting values.
- Tick rate: 20 Hz holds for cars (contacts and zero-contact tests are stable). The decision of section 5.2 also needs the crowd forces, so it is confirmed in Phase 3.
- Speed: the default run of the test level takes about 20 ms headless (about 13 000 times real time, 25 cars). Not measured at 400 agents. Contact detection tests every pair.
- Open points and limits:
  - With priority to the right and 4 times the demand, careful drivers from the north wait more than 120 s and give up (41 in 20 seeds), because the west flow leaves no gap. Under signals nobody gives up. The user accepted this on 2026-11-16: it is a problem for the player to solve, and a test pins it.
  - Careful drivers still brake hard sometimes (about 0.7 events per run).
  - Agents have no lateral position inside the lane. Sight is not blocked by obstacles or large vehicles. The level's bicycle and pedestrian flows are not simulated. The level file has no archetype mix: a run picks a population (careful, mixed, careless).
  - The run is not inside `Simulation` and does not use the `Outbox`: `StreetRun` keeps its own event log.
  - The replay hashes were made on Windows and are not yet checked on macOS. A difference in the float library can change them.

### Phase 3 — Pedestrians and crowds (L)
- Walk graph on walkable polygons; social-force movement, groups, density effect, obstacles.
- Crossings (zebra, signal), waiting at the kerb, crossing out of the zebra by compliance and detour length.
- Car against pedestrian: perception, yielding, hidden pedestrians (a van hides a child).
- Annoyance for pedestrians (delay, density, no path).

**Done when:** two opposite crowds on a 2 m sidewalk form lanes and slow down, with no overlap; a zebra placed 40 m from the desire line is ignored by a measurable part of the people; a hidden pedestrian can be hit by an inattentive driver and never by a careful one at 30 km/h.

**Done (2026-11-17).** All three criteria hold. The figures below are the ones the implementing agents measured and reported; the tests assert them with bands.

- Code, in `tf_sim::street::run`: `walk` (walkable space, walk graph), `walker` (the pedestrian and its archetypes), `crowd` (forces and constraints), `kerb` (waiting, gaps, lights, the choice of a way), `sight` (lines of sight), `yielding` (drivers and people on foot), `meet` (car-against-walker contacts and near misses). Crossings are in `street::space` / `model` / `design` (`CrossingDef`, `DesignCommand::AddCrossing` / `RemoveCrossing`). Tests: `tf_sim/tests/street_walk.rs`, `street_cross.rs`, `street_meet.rs`. In `tf_game`: `WalkerPose` in `street_runner.rs`, `views/walkers.rs`, the zebras in `views/street.rs`.
- Done when:
  - Crowds: two opposite crowds on a 2 m sidewalk form lanes (correlation between the side and the direction 0.86 to 0.90; the test asks for more than 0.6) and slow down to 75 to 82 % of the free speed. Two bodies overlap by 0.02 m at most (asserted on every tick).
  - Zebra 40 m from the desire line: 28 % of a mixed population cross out of it, 0 % of a careful one, 92 % of a careless one; 0 % when the zebra is on the desire line.
  - Hidden pedestrian (a parked van hides the kerb of a zebra, 30 km/h): careful drivers, 788 crossings over 20 seeds, 0 contacts; careless drivers, 5 contacts, each one with a cause that names the hidden walker, the look not made, a distraction, the speed too high for the sight, or the yield not made.
  - Also: careful drivers and careful walkers have 0 contacts over 50 seeds on the test level, at 1 and 4 times the demand, with and without signals. The severity shares agree with the curves at 20, 30 and 50 km/h. A death ends the run.
- How it works:
  - **Two collections.** Vehicles are in `StreetRun::agents`, pedestrians in `StreetRun::walkers`, with ids from `WALKER_ID_BASE`, so that a walker never shifts the id or the random stream of a vehicle. All the draws of a walker are made at spawn, in a documented order.
  - **Walking.** The walkable space is the sidewalk and terrace strips and the level's plazas, less the obstacles. A path is a chain of areas (A* on the kept `routing::astar`); the walker steers through it with a funnel. The crowd model is a goal force, an anticipation of the closest approach, a step to the right when two people meet, body and wall forces, a detour around obstacles, group cohesion, and Weidmann's density-speed curve; then position constraints remove the overlaps. Only square roots and polynomials are used, so every platform gives the same bits.
  - **Groups.** A spawn can bring a pair or a group of three, which walks at the pace of its slowest member. `per_hour` in the level counts people, not groups.
  - **Crossings are data.** A crossing (a zebra for now) has a segment, a position and a width. The level gives the default ones and the player's design adds and removes them. `carrefour-ecole` has one zebra on each arm.
  - **Where to cross.** Each walker has a rule way (streets are crossed on crossings only) and a free way (anywhere). It takes the free way when that is legal (no crossing within 50 m, Code de la route R412-37) and saves 5 m or more; otherwise with a probability that grows with the detour and falls with its compliance. The choice is made at spawn.
  - **At the kerb.** A walker looks at the vehicles it can see, in the snapshot as old as its reaction time, and goes when the gap is enough for its crossing time. Risk and frustration make the gap shorter. On a zebra it trusts a driver who can still stop comfortably. At signals it has a walk light (on while no conflicting approach has green, and only if its own crossing fits before the conflicting green); a walker with low compliance can go on red.
  - **Drivers.** A driver brakes for any walker it sees in its path, and yields at a zebra to people on it or about to step on it (at signals only while they have walk; a turning driver on a permissive green yields too). When it cannot see a kerb of a zebra ahead, it slows to the speed from which it can stop (about 10 km/h at the van of the test), and never below a creep of 1 m/s. It does not stop on a zebra, and it does not enter the junction while the zebra past its exit is busy. The personality bends this with two draws per zebra (yields, looks).
  - **Sight.** A sight line is cut by tall obstacles (trees, pillars, metro exits, bus shelters, a parked van) and by vans and buses. Cars do not hide people.
  - **Contacts.** A swept walker circle against the swept car rectangle; the outcome is drawn from the pedestrian curves (Rosén & Sander 2009, Tefft 2011). Causes come from both sides: for the driver `Distracted`, `RanRed`, `DidNotYieldToWalker`, `DidNotLook`, `TooFastForSight`, `HiddenWalker`, `Speeding`, `TooFastToStop`; for the walker `CrossedOutOfCrossing`, `WalkedOnRed`, `AcceptedShortGap`. Near misses use the post-encroachment time where a route cuts a road area, and hard braking for a walker.
  - **Annoyance** of a walker: its delay against the free walk (the wait at the kerb is in it), the time in a dense crowd, the bumps and the forced detour, divided by its patience.
  - **The view.** A walker is a circle of its real radius with a dark mark on the side it walks to, one colour per archetype, darker while it waits, magenta when hit, with a red ring when it crosses out of a crossing or on red. A zebra is drawn as bars 0.5 m wide, long in the direction of the traffic.
- Tick rate (open point 5): **20 Hz stays.** A walker moves 0.12 m per tick at most, so it cannot pass through a wall. In dense contact the force changes direction often, at 20 Hz and at 40 Hz alike, so the step size is not the cause; the position constraints keep the result bounded (no walker held back, no reversal of the motion, both asserted in the counterflow test).
- Speed (release, `run_level`): the default run of the test level is about 1 370 times real time (25 cars and about 30 walkers), and 220 to 330 times at 4 times the demand. Not measured at 400 agents. Each walker scans all the walls every tick: a wall grid is the next speed-up.
- Replay hashes: they changed in each stage, each time with the cause in the commit message (pedestrians spawn; the fixes of the reviews; kerbs and crossings; cars react to walkers).
- Decisions of the user (2026-11-16):
  - The two Phase 2 tests of careful drivers (`careful_drivers_never_touch_and_everybody_arrives`, `careful_drivers_cope_with_signals_on_a_busy_street`) run without the pedestrian flows, with their assertions as they were: their subject is drivers among themselves. With pedestrians, `careful_drivers_and_walkers_never_touch` asserts no contact and at most 1 % of give-ups.
  - A few careful drivers give up when steady streams of walkers and the priority to the right leave them no gap: 6 of 1 549 at the normal demand without signals, 0 with signals, 16 of 5 762 at 4 times the demand with signals. This is a problem for the player to solve.
- Open points and limits:
  - A car that entered the junction with its exit zebra free can be held inside it when walkers then step onto that zebra (seen one time in 50 seeds).
  - The choice of a way is made at spawn from the compliance only; a walker does not change its mind at the kerb. Groups split at kerbs. People do not cross a junction area diagonally.
  - Parked vehicles are obstacles (`ObstacleKind::ParkedVan`), not agents. A parking strip is part of the carriageway that a walker crosses.
  - A walker whose centre is found inside a standing vehicle after the constraints is logged as a contact at 0 km/h, so that the invariant "nobody inside a vehicle without a contact" holds. It did not occur in the test runs.
  - Curved streets: the funnel needs convex areas, and strips are convex only on straight segments.
  - The crowd density is a count in a 1 m circle, not corrected near walls. Lane formation is helped by the keep-right rule; the test does not separate the rule from the crowd effect.
  - At the default framing a walker is 2 pixels wide. A minimum drawn size is a question for the art direction.
  - Bicycle flows are still not simulated (Phase 6). The run is still not inside `Simulation`.

### Phase 4 — Vertical slice: design, run, report (M) ⭐ "is this credible?" checkpoint
- `Phase` state, run clock rule (7.4), minimal HUD, minimal report (score parts, incident list, markers on the map).
- First tools: signal plan from templates, zebra, speed limit, bollards. First score (section 6).
- Star stages (section 6): the level file holds the stages, a run plays the current stage, the report says passed or broken and why, and the next stage opens on the same design.
- Decided (user, 2026-11-17; section 5.7, addition 1): the wanted speed depends on the look of the street, so that the speed limit and the bollards have a believable effect. It changes the replay hashes; the commit says so.
- Level 1 keeps its four default zebras (decided 2026-11-17): a real school crossing in Paris has them, so they are the believable starting state. The player can still move or remove them with the zebra tool; the "simple street with one zebra" level (section 10, level 0) is the place to teach that tool.
- The test level becomes level 1, playable through all its star stages.
- After the first playtest (user, 2026-11-17): the light palette of the target look (7.3), the speed limit drawn on its segment, a preview of each tool under the pointer, an inspector that shows the traits and state of the car or pedestrian under the pointer, and braking that is more real (a maximum deceleration per vehicle and a short brake build-up, instead of one 8 m/s² for all), with a check of why careful drivers have so many hard-brake near misses.
- After the second playtest (user, 2026-11-17): pedestrians decide each rule at each crossing from what they see (section 5.3), and they cut corners beside zebras.

**Done when:** the user plays level 1 several times and finds the behaviour believable; a better design holds more stages for reasons that the report explains; the load at which a design breaks is plausible. **Stop and decide** here if the direction is right before the rest. Fun is not the test of this checkpoint (section 1, "Direction").

### Phase 5 — Remove the old game (S)
- Delete the dropped modules of section 2.2, their tests and their goldens, in commits that say so. Delete `migration/` when nothing reads it.
- Clean `CLAUDE.md`, the keys, the screenshot scenarios, the benchmarks (retarget to a level).
- Choose the UI approach (7.5).

**Done when:** the workspace contains only the new game; `check.sh` is faster; `cargo clean --profile dev` was done and the freed space is reported.

### Phase 6 — Cyclists (L)
- Corridor movement, overtaking, filtering, bicycle box, bicycle lanes and tracks, contraflow.
- Cyclist archetypes and rule-breaking (red lights, sidewalk, wrong way).
- Classic conflicts: right-turning vehicle against a cyclist going straight (blind spot), door of a parked car, pedestrian against cyclist at a crossing, van stopped on the bicycle lane.
- Accepted (section 5.7, additions 2, 3 and 7, user 2026-11-17): attention with a gaze direction and a realistic field of view, a reaction time that depends on surprise, and hearing.
- Work packages (2026-11-17): 1 cyclist core (agent, side position, following, passing, filtering, archetypes); 2 bicycle design (painted lane, track, contraflow, bicycle box, bicycle movements at junctions); 3 perception (field of view, gaze, blind spots, surprise, hearing); then 4 classic conflicts, 5 swerving, 6 views. 1-3 run in parallel.
- **Evasive steering** (user, 2026-11-17): a driver can swerve as well as brake to avoid someone, using the lateral position in the lane that this phase adds. A swerve brings its own risks, so it needs contacts with fixed things too (parked cars, kerbs, bollards, the other lane, people on the sidewalk). Near-crash studies show that most drivers only brake and that swerving is rarer and less reliable: it must stay a minority reaction, chosen by time to collision, free space and the driver's traits.

**Done when:** on a test street, a protected track gives measurably fewer serious conflicts than a painted lane, and a painted lane fewer than none; a right-turn conflict can be removed by a design change (bicycle box, separate phase, kerb).

### Phase 7 — Full toolbox and reallocation (L)
- Strip editor (use, width, direction), kerb extensions, turn bans, signs, all crossings, all obstacles, areas, parking and delivery bays (with double-parking when there is none).
- Undo / redo, validation of constraints, ghost flows.
- The Life bonus.

**Done when:** each item of section 4 has a test that shows its effect, and one level can be solved in two clearly different ways.

### Phase 8 — Time of day and calibration (M)
- The four slices, modifiers per slice, night visibility, all archetypes.
- Calibration pass with real figures (walking speed, cycling speed, reaction times, braking, gap acceptance, share of red-light running). A table in this document records the source of each value.
- Score tuning.
- Proposed (section 5.7, additions 4 to 6): imitation, linked traits, hurry.

**Done when:** the same design gives a different, plausible picture in each slice, and the reference values are inside their bands in a statistical test.

### Phase 9 — Paris levels and progression (L)
- Level authoring workflow (section 10) and 5 to 7 levels with rising difficulty: from a simple street with one zebra to a complex roundabout.
- Star stages for each level (load per stage, set so that a good design for that place holds them and a weak one breaks), unlocks, progress save, tutorial steps, the impact study (section 6).

**Done when:** a new player can go from level 1 to the last level without help.

### Phase 10 — Interface, art, sound (L)
- All screens of 7.5, replay of incidents, art direction (to be decided then: the amber dot display of the old plan is one candidate), sound (traffic, bells, horns as a frustration signal).

### Later
Section 11.

---

## 9. Tests and quality

- **Unit tests** next to the code (geometry, strips, perception, forces, severity).
- **Micro-scenarios** in `tests/` (one behaviour each: "a careful driver stops for a pedestrian on a zebra", "a cyclist filters to the bicycle box").
- **Statistical tests**: a measure over N seeds must stay in a band (as `run_baseline` does today). They protect the tuning.
- **Replay hashes** (the new goldens): `level + design + seed` → hash of the event log, stored in the repo. A change of hash means that behaviour changed; the commit must say that it is deliberate.
- **Invariants on every tick in tests**: no body inside a building or an obstacle; no contact without a cause chain; no agent lost; a population with full compliance and attention has no contact.
- **Performance**: 400 agents at 60 FPS; headless at 200 times real time or better on the MacBook Air (the impact study needs it). A benchmark binary measures both.
- **Rules that stay** from `CLAUDE.md`: `tf_sim` pure and without Bevy, fixed tick, `Outbox` events, `IndexMap` and `shift_remove`, no `HashMap` iteration in sim logic, views read snapshots only, `scripts/check.sh` before each commit.

---

## 10. Paris levels

Candidates, from simple to hard. Each level is one specific crossing; the progression goes from a simple street with one zebra to a complex roundabout and more (user, 2026-11-17). Inside a level, the star stages add load (section 6). The final list and order are decided in Phase 9; each level teaches one subject.

| # | Place | Subject |
|---|---|---|
| 0 | A simple street with one zebra (fictional) | First contact: one crossing, cars and pedestrians, speed against yielding |
| 1 | "Carrefour école" (fictional, small streets, 11e style) | Tutorial: zebra, 30 km/h, bollards, children |
| 2 | Rue de Rivoli × Boulevard de Sébastopol | Two major bicycle axes cross; cyclists against pedestrians |
| 3 | Carrefour de l'Odéon | Irregular shape, metro exits, terraces, crowds |
| 4 | Carrefour Barbès – Rochechouart | Very dense pedestrians, market, elevated metro pillars as obstacles |
| 5 | Place de Clichy | 5 arms, buses, deliveries, night |
| 6 | Place de la Bastille | Very large space to reallocate; a plaza is possible |
| 7 | Place de l'Étoile | Final level: a large roundabout, 12 avenues, priority to the right, no signals by default |

**Authoring.** Geometry is drawn by hand as a simplified plan, with public maps as the visual reference. If map data (for example OpenStreetMap) is imported and not only looked at, its licence and attribution rules apply: decide this before the first real level. Flows are plausible, not measured, unless open count data is used. A small level tool (a debug mode of the game that edits a level file) is planned in Phase 9 if hand-written RON becomes too slow.

---

## 11. Later ideas (not planned)

- Buses and trams as scripted vehicles with stops and riders who cross.
- Street closure (pedestrian street, school street) and bigger areas (a district of several junctions).
- Weather (rain: longer braking, fewer cyclists, umbrellas).
- Events: market day, demonstration, strike day with heavy car traffic.
- A sandbox mode with free flows; a level editor for players; sharing of designs by seed.
- Emergency vehicles that must cross the place.
- An "endless mode" after the last star: the load grows until the design breaks, and the report gives the load reached. Nice to have only, because no real street gets unbounded traffic (user, 2026-11-17).

---

## 12. Open points

| # | Question | Needed before | Default if no answer |
|---|---|---|---|
| 1 | Title of the game and of the crates | Phase 5 | Keep `tf_*` |
| 2 | How injuries and deaths are shown | Phase 4 | Abstract: a marker and a stopped scene, no blood |
| 3 | Money budget in addition to the space budget | Phase 7 | No |
| 4 | Is one death always a game over, or can the impact study replace it with a risk limit | Phase 9 | One death in the played run is a game over |
| 5 | Tick rate 20 or 30 Hz | Phase 2 (measured) | 20 Hz |
| 6 | UI library | Phase 5 | `bevy_ui` only |
| 7 | Source and licence of map references | Phase 9 | Hand-drawn simplifications |
| 8 | Art direction | Phase 10 | Flat shapes |
| 9 | Game language (French terms, French and English UI) | Phase 10 | English UI with French place names |
| 10 | When the design changes, are the stars already won kept, or must the new design pass the lower stages again | Phase 4 | **Decided (2026-11-17):** one design must pass every stage up to its star; the lower stages run again headless after a change |
| 11 | After the last star, an open stage that adds load until the design breaks and reports the load reached | — | **Decided (2026-11-17):** not planned; an "endless mode" is a nice-to-have in section 11, because it is not realistic |

**Phase 0 (2026-11-16):** no open point blocks Phase 1. Every point uses its default until the user decides otherwise.

---

## 13. Main risks

| Risk | Answer |
|---|---|
| **The simulation is not credible** (people act like robots or like fools) | Calibration phase with real figures; micro-scenarios per behaviour; visible agent state so that odd behaviour can be diagnosed. |
| **A random death feels unfair** | Determinism, cause chain, replay; severity depends on speed, which the design controls; the impact study shows risk before the run. |
| **Watching is boring** | Accepted for now: realism comes first (section 1). Short slices, speed control, incident feed and visible personalities help; the fun is tuned after the simulation is credible. |
| **Load stages break for the wrong reason** (a design fails at star 3 because the sim jams, not because the street would fail) | Gridlock and give-ups are checked against real capacity figures (Phase 8); the report must name a believable cause for each broken stage. |
| **One best design per level** | Constraints and the "worst mode counts double" rule force trade-offs; Phase 7 asks for two different solutions per level. |
| **The crowd model is unstable or slow** | Tick rate decided by measurement in Phase 2; spatial grid; agent count per level is limited by design. |
| **Scope**: three movement models, an editor, a full UI | Vertical slice first with cars and pedestrians only; old code deleted early to cut maintenance; each phase can ship a playable state. |
| **Two simulations in the tree** during Phases 1 to 4 | The old systems are frozen (no change, tests keep running) and removed in one phase. New code does not depend on them, only on the kept modules. |
