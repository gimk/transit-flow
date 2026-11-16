# Urbanism Pivot — Long-Term Plan

**Status (2026-11-16):** approved by the user on 2026-11-16; it replaces [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) as the roadmap (the old plan stays as the record of the city builder). Phase 0 done (tag `city-builder-final`, docs updated, open points of section 12 on their defaults). Phase 1 done (space model, level and design files, test level drawn from its file; see the notes under Phase 1). Phase 2 done (cars: headless core, threaded runner, car view; see the notes under Phase 2). The replay hashes made on Windows are the same on macOS (checked 2026-11-16). **Next: Phase 3.**

**Working title:** to be chosen (section 12). The crates keep the names `tf_sim` and `tf_game` until then.

## Progress notes

The newest entry is first. Each entry tells what was done, where, and what to do next. Keep only the entries that are still useful; the phase notes in section 8 are the record.

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

**Pitch.** You are an urban designer (*concepteur d'espace public*) in Paris. Each level is one real, complex intersection. The buildings and the kerb-to-kerb space are fixed. You decide how the space is shared between cars, cyclists and pedestrians. Then you start the simulation and you can only watch. The people decide if your design is good.

**The loop of one level:**

1. **Brief.** The level shows the place, the flows (how many cars, cyclists, pedestrians, from where to where, at which time of day), the constraints (for example: the bus line must pass, deliveries must be possible) and the star thresholds.
2. **Design.** The player changes the use of the space and places equipment (section 4). The simulation is stopped. Ghost flows show the demand.
3. **Run.** The simulation plays one compressed day in four time slices. The player can pan, zoom, change the speed and pause. The player cannot edit.
4. **Report.** Score, list and map of incidents, replay of each incident with its cause. Then back to step 2 with the same design. The best run gives the stars.

**Decisions made by the user (2026-11-15):**

| Subject | Decision |
|---|---|
| The old city builder | Replaced. It stays in git history behind a tag. |
| Loop | Free iteration: design, run, report, design again. |
| Player power on roads | Reallocation of space: the asphalt is fixed, its use is not (lane to bicycle lane or bus lane, one-way, turn bans, speed limit, wider sidewalk). No street closure in the first version. |
| Levels | Hand-made, simplified copies of real Paris places. |

**Score rules given by the user:** annoyed people, conflicts and light injuries decrease the score. A death is a game over. Free space, rest areas and green areas give bonus points with time.

**Design pillars:**

1. **Readable cause.** Each incident has a cause chain that the game can show ("the driver did not look, the cyclist was in the blind spot, the speed was 46 km/h"). Without this, a random death feels unfair and the player learns nothing.
2. **Design controls risk.** The player cannot control people, only what the space lets them do. A good design makes a fatal error almost impossible (low speed at conflict points, separation, short crossings). A bad design lets one careless person kill.
3. **Same design, same seed, same run.** A run is deterministic. The player can replay it, and a change of result comes only from a change of design.
4. **Real proportions.** Metres, seconds, real speeds. The old game compressed time and distance; this one cannot, because injury depends on real speed.

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
- constraints, the allowed tools, the star thresholds, the seed.

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

---

## 6. Score

The numbers below are start values for tuning.

- **Safety.** Each conflict and each injury removes points by severity. A death: run failed, no stars.
- **Serenity.** Mean annoyance per mode. The worst mode counts double, so the player cannot get a good score by sacrificing one group.
- **Life (bonus).** Points per second of the run for: green area, free pedestrian space per person at the busy moments, rest places that people use, shade. The bonus cannot be more than a set part of the total, so that a park with a deadly road next to it does not win.
- **Constraints.** A broken level constraint (the bus cannot pass) fails the run with a clear message, before or during the run.
- **Stars.** 1 star: no death and no serious injury. 2 and 3 stars: score thresholds of the level.

**Fairness.** A run is deterministic, so a death is always reproducible and explained. From Phase 9, an **impact study** runs the design many times headless with different seeds (the headless sim must be fast, section 9) and gives a risk estimate ("1 run in 20 has a serious injury at the north crossing"). The stars can then ask for a risk level, not for one lucky run.

---

## 7. The game shell (`tf_game`)

### 7.1 Phases
A `Phase` state: `Menu`, `Brief`, `Design`, `Run`, `Report`. Tools work only in `Design`. The sim steps only in `Run`. `SimHandle::replace` already swaps a simulation and resets the views; a run builds a new `Simulation` from `level + design + seed`.

### 7.2 Editing
New tools on the `BuildTool` framework: strip editor (click a strip, cycle its use; drag a boundary), direction, speed zone, signal plan, signs, crossing, obstacle (point or line), area (dragged polygon), eraser. All edits are commands on the design, so undo / redo and the save come for free. A validation pass shows broken constraints in the design phase.

### 7.3 Views
Flat shapes stay (the art direction is a later decision). New: strips and separators, junction markings, signal heads per approach with pedestrian and bicycle lamps, people with a body and a heading, cyclists, agent state (a waiting pedestrian, a frustrated driver, an agent that breaks a rule), incident markers that stay on the map, ghost flows in the design phase, a heat map of conflicts in the report.

### 7.4 Run clock
The threaded runner stays. The result must not depend on the frame rate: the sim is already tick-based, so the rule is that **nothing from the frame (real delta, batch size) enters the sim**, and a test proves that a run through the threaded runner and a headless run give the same event log.

### 7.5 UI
All new (today: one status line). Needed: level select, brief panel, tool palette with unlocks, run HUD (time slice, live score, counters, speed), incident feed (click to go there), report screen with stars, incident list and replay, pause menu, settings. Decide the UI approach (`bevy_ui` only, or an added library) at the start of Phase 5.

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

### Phase 4 — Vertical slice: design, run, report (M) ⭐ "is this fun?" checkpoint
- `Phase` state, run clock rule (7.4), minimal HUD, minimal report (score parts, incident list, markers on the map).
- First tools: signal plan from templates, zebra, speed limit, bollards. First score (section 6).
- The test level becomes level 1, playable from start to stars.

**Done when:** the user plays level 1 several times, and a better design gives a better score for reasons that the report explains. **Stop and decide** here if the direction is right before the rest.

### Phase 5 — Remove the old game (S)
- Delete the dropped modules of section 2.2, their tests and their goldens, in commits that say so. Delete `migration/` when nothing reads it.
- Clean `CLAUDE.md`, the keys, the screenshot scenarios, the benchmarks (retarget to a level).
- Choose the UI approach (7.5).

**Done when:** the workspace contains only the new game; `check.sh` is faster; `cargo clean --profile dev` was done and the freed space is reported.

### Phase 6 — Cyclists (L)
- Corridor movement, overtaking, filtering, bicycle box, bicycle lanes and tracks, contraflow.
- Cyclist archetypes and rule-breaking (red lights, sidewalk, wrong way).
- Classic conflicts: right-turning vehicle against a cyclist going straight (blind spot), door of a parked car, pedestrian against cyclist at a crossing, van stopped on the bicycle lane.

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

**Done when:** the same design gives a different, plausible picture in each slice, and the reference values are inside their bands in a statistical test.

### Phase 9 — Paris levels and progression (L)
- Level authoring workflow (section 10) and 5 to 7 levels with rising difficulty.
- Unlocks, stars, progress save, tutorial steps, the impact study (section 6).

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

Candidates, from simple to hard. The final list and order are decided in Phase 9; each level teaches one subject.

| # | Place | Subject |
|---|---|---|
| 1 | "Carrefour école" (fictional, small streets, 11e style) | Tutorial: zebra, 30 km/h, bollards, children |
| 2 | Rue de Rivoli × Boulevard de Sébastopol | Two major bicycle axes cross; cyclists against pedestrians |
| 3 | Carrefour de l'Odéon | Irregular shape, metro exits, terraces, crowds |
| 4 | Carrefour Barbès – Rochechouart | Very dense pedestrians, market, elevated metro pillars as obstacles |
| 5 | Place de Clichy | 5 arms, buses, deliveries, night |
| 6 | Place de la Bastille | Very large space to reallocate; a plaza is possible |
| 7 | Place de l'Étoile | Final level: 12 avenues, priority to the right, no signals by default |

**Authoring.** Geometry is drawn by hand as a simplified plan, with public maps as the visual reference. If map data (for example OpenStreetMap) is imported and not only looked at, its licence and attribution rules apply: decide this before the first real level. Flows are plausible, not measured, unless open count data is used. A small level tool (a debug mode of the game that edits a level file) is planned in Phase 9 if hand-written RON becomes too slow.

---

## 11. Later ideas (not planned)

- Buses and trams as scripted vehicles with stops and riders who cross.
- Street closure (pedestrian street, school street) and bigger areas (a district of several junctions).
- Weather (rain: longer braking, fewer cyclists, umbrellas).
- Events: market day, demonstration, strike day with heavy car traffic.
- A sandbox mode with free flows; a level editor for players; sharing of designs by seed.
- Emergency vehicles that must cross the place.

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

**Phase 0 (2026-11-16):** no open point blocks Phase 1. Every point uses its default until the user decides otherwise.

---

## 13. Main risks

| Risk | Answer |
|---|---|
| **The simulation is not credible** (people act like robots or like fools) | Calibration phase with real figures; micro-scenarios per behaviour; visible agent state so that odd behaviour can be diagnosed. |
| **A random death feels unfair** | Determinism, cause chain, replay; severity depends on speed, which the design controls; the impact study shows risk before the run. |
| **Watching is boring** | Short slices, speed control, incident feed, visible personalities. Checkpoint at Phase 4 before the large phases. |
| **One best design per level** | Constraints and the "worst mode counts double" rule force trade-offs; Phase 7 asks for two different solutions per level. |
| **The crowd model is unstable or slow** | Tick rate decided by measurement in Phase 2; spatial grid; agent count per level is limited by design. |
| **Scope**: three movement models, an editor, a full UI | Vertical slice first with cars and pedestrians only; old code deleted early to cut maintenance; each phase can ship a playable state. |
| **Two simulations in the tree** during Phases 1 to 4 | The old systems are frozen (no change, tests keep running) and removed in one phase. New code does not depend on them, only on the kept modules. |
