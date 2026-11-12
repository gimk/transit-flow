# Transit Flow — Implementation Plan

Companion to [game_design_architecture_concept.md](../game_design_architecture_concept.md).
That document says *what* the game is. This one says *how we build it, in what order, and how we know each step is done*.

**Engine:** Rust + Bevy 0.20 (ported from Godot 4.7 / GDScript at the end of 2026-11; the phase history below was written for Godot and is kept as it was; the last Godot commit is the git tag `godot-final`, and `docs/BEVY_MIGRATION_PLAN.md` and `docs/migration/NOTES.md` record the port). Everything below up to the Polish track is built and runs in the Bevy build.

**Status (2026-11-12):** the port to Rust + Bevy is done and plays like the Godot build (same simulation tick for tick, every tool and key), drawn with flat shapes and one status line. Built: Phases 0–9 (roads, routing, junctions and lights, the living city, multi-lane and one-way roads, driveways, pedestrians, buses with bays and bus lanes, mode choice, curved roads) and the Town and Bottleneck scenarios. Left behind in the port, to redo in Bevy: the Polish track's art and UI items and Phase D (dot display). **Next: to be chosen by the user**, between redoing the art direction in Bevy and Phase 10+.

---

## 1. Guiding Principles

1. **Simulation is separate from presentation.** All traffic, road and zone logic lives in the `tf_sim` crate: plain Rust, no Bevy, no rendering. The Bevy app (`tf_game`) only *observes* the sim (through snapshots and events) and draws it. This gives headless tests and deterministic replays, and lets the sim run on its own thread.
2. **Fixed-tick simulation.** The sim advances in fixed steps of 20 Hz (`Simulation::step`), driven by the runner in `tf_game`, a couple of ticks ahead of the screen on a worker thread; sim code never sees frame `delta`. Rendering interpolates between ticks. Speed controls (pause / 1× / 2× / 4×) just change how many ticks run per frame.
3. **"Everything is a lane."** The concept's "a grid road is a 2-point curve" idea, pushed one step further: *both* road lanes *and* the turning paths inside intersections are `Lane` objects holding a `Curve2D`. A vehicle's route is just a list of lanes. Curves, ramps, roundabouts and bus lanes all reuse the same movement code.
4. **Debug views from day one.** Lane IDs, route lines, per-lane occupancy, and trip times are visible via toggleable overlays. A traffic game is impossible to tune blind.
5. **Data is serializable from day one.** The road network, zones, lots and transit serialize to JSON (`Simulation::to_dict` / `from_dict`). Saving/loading test layouts is also how we build repeatable test scenarios.

---

## 2. Architecture

### 2.1 Layers

```
┌──────────────────────────────────────────────────────────────┐
│ UI (status line: time, speed, Flow %, vehicles, tool, hint)  │  ← bevy_ui text; the HUD panels were left behind
├──────────────────────────────────────────────────────────────┤
│ Tools  (ToolManager: Road, Bulldoze, Zone x4, Signal, Bus    │  ← read Pointer + ActionInput, grid snapping,
│         line, Route)                                         │    previews; tf_game
├──────────────────────────────────────────────────────────────┤
│ Views  (roads, lots, zones, moving things, transit, lights,  │  ← flat shapes; read ViewSnapshot, RoadsView and
│         overlays: Bevy systems)                              │    messages, never the sim; tf_game
├──────────────────────────────────────────────────────────────┤
│ Runner (threaded sim, SimHandle, snapshots, messages)        │  ← tf_game
├──────────────────────────────────────────────────────────────┤
│ Sim    RoadNetwork → LaneGraph → Router                      │  ← tf_sim: pure Rust, no Bevy, unit-testable
│        VehicleSystem, TrafficLights, PedestrianSystem,       │
│        TransitSystem, ZoneSystem, CitizenSystem, FlowMetric  │
└──────────────────────────────────────────────────────────────┘
```

Two crates: `tf_sim` (the simulation, plain structs, stepped by `Simulation::step()` at 20 Hz) and `tf_game` (the Bevy app). Sim events go through an `Outbox` and are dispatched by `Simulation::call` in a fixed listener order; the views get them as Bevy messages. See `CLAUDE.md` for the rules.

Tools mutate the sim only through `RoadNetwork` / `ZoneSystem` commands (`add_road`, `remove_segment`, `paint` ...). This is also the natural hook for undo/redo later.

### 2.2 Road data model

| Class | Holds |
|---|---|
| `RoadNode` | id, position, `elevation_layer`, connected segment ids, intersection control type |
| `RoadSegment` | id, start/end node ids, **centerline `Curve2D`**, `RoadType`, `elevation_layer`, lane ids |
| `RoadType` (`Resource`) | lanes per direction, lane width, speed limit, allowed vehicle classes, texture — `.tres` files so new road types need no code |
| `Lane` | id, owning segment *or* node, `Curve2D`, length (cached), speed limit, allowed vehicle classes, **ordered list of vehicles on it**, outgoing lane ids |

**Lane curves from the centerline.** Offsetting a Bézier curve does not produce a Bézier curve, so lane curves are *not* built by offsetting control points. Instead: sample the centerline's baked points, push each one sideways along the normal by the lane offset, and store the result as a dense polyline `Curve2D`. Identical code for straight grid roads now and sweeping curves later.

**Intersection connectors.** When a node has ≥ 2 segments, generate one connector `Lane` per allowed movement (incoming lane end → outgoing lane start) as a short quadratic curve. U-turns off by default. Turn rules (which incoming lane may go where) are a per-node table, which is exactly what "dedicated turning lanes" will edit later.

**Graph editing operations** (all on `RoadNetwork`, all emit `Events.network_changed(dirty_ids)`):
- `add_road(a, b, type, layer)` → finds crossings with segments **on the same layer only**, splits them, inserts nodes, merges with existing nodes at the same grid point.
- `remove_segment(id)` → removes lanes; vehicles on removed lanes are despawned (counted as failed trips, not silently lost).
- Lane graph and connectors are rebuilt **only for dirty nodes/segments**, not the whole map.

### 2.3 Pathfinding

- **Graph:** one A* point per **lane** (positioned at the lane's end), one directed connection per allowed lane → lane transition. Routes are therefore lists of lane ids, which is exactly what vehicles consume.
- **Costs:** subclass `AStar2D` and override `_compute_cost` / `_estimate_cost` so the cost of entering a lane is its travel time:
  `length / speed_limit × (1 + congestion_penalty)`, with the heuristic = straight-line distance / max speed (admissible).
- **Congestion** is refreshed every few sim seconds from lane occupancy + average speed, not every tick.
- **Route caching:** most trips share a handful of (origin access point, destination access point) pairs. Cache routes per pair and invalidate on network change or congestion refresh. This is the main defence against pathfinding cost when we reach thousands of vehicles.
- **Trip endpoints are positions on lanes, not nodes.** Each zone lot has an *access point* = (lane id, distance along lane) on the nearest road. Vehicles spawn and despawn there.

### 2.4 Vehicles

- Stored as **struct-of-arrays** inside `VehicleSystem` (`PackedInt32Array` lane ids, `PackedFloat32Array` distances/speeds, etc.) rather than one object per vehicle. Start simple in Phase 1 (a small class is fine) but keep it behind the `VehicleSystem` API so the switch is internal.
- **Car-following:** each lane keeps its vehicles ordered by distance. A vehicle looks at the one ahead on its lane; **if it is near the lane end it also looks at the last vehicle on the next lane in its route**. Speed is adjusted with a simple IDM-like rule (accelerate toward lane limit, brake based on gap and closing speed). No physics bodies, no `Area2D`.
- **Entering an intersection** requires (a) the controller to allow the movement and (b) **space on the exit lane** for the whole vehicle ("don't block the box"). Without (b), grids deadlock within minutes. This rule is non-negotiable from Phase 2.
- **Rendering:** one `MultiMeshInstance2D` per vehicle class (car, truck, bus), transform + color per instance, updated once per frame with interpolated positions.
- **Patience:** each vehicle carries its trip start time and the free-flow ideal time for its route. Above a threshold it turns red; above a hard limit it despawns as a failed trip.

### 2.5 Intersection control

`IntersectionController` interface with `can_enter(connector_lane_id, vehicle) -> bool` and `tick(dt)`:
- `Uncontrolled` — yield to vehicles already inside / conflicting connectors (needs a precomputed **conflict table**: which connector curves cross).
- `StopSign` — first-come reservation with a stop delay.
- `TrafficLight` — timed phases; each phase is a set of allowed connectors.
- `Roundabout` comes later and is really a ring of segments + yield nodes, not a special controller.

### 2.6 Zones, demand and the daily pulse

- Zones **are** grid-based, so here a `TileMapLayer` *is* the right tool (for painting and display only). `ZoneSystem` mirrors the painted cells into lots grouped by block, each with type, density level and access point.
- **Citizens are lightweight records**, not simulated entities: `{home_lot, work_lot}`. They only become vehicles while travelling. Trucks are pure trips (origin → destination with a cargo type).
- `TripScheduler` reads the `GameClock` and spawns trips according to time-of-day curves (morning rush home→work, midday freight, evening work→shop→home, night trickle). Curves are data (`Curve` resources) so they can be tuned in the editor.
- `FlowMetric`: per completed trip, `efficiency = ideal_time / actual_time` (failed trip = 0). Flow % = rolling average over the last in-game day. This is the single number the player optimises.

### 2.7 Elevation

`elevation_layer` lives on nodes and segments from Phase 1 even though only layer 0 is buildable. Crossing detection already filters by layer, so bridges/tunnels later need no data-model change. Rendering sets `z_index` from the layer.

---

## 3. Project Layout

The Rust workspace (the Godot layout of the early phases is in the `godot-final` tag):

```
Cargo.toml, rust-toolchain.toml, .cargo/config.toml (cargo play), scripts/check.sh
crates/tf_sim/     the simulation library: clock, simulation, events, rng, geom/, roads/, routing/,
                   city/, traffic/, walking/, transit/, debug/ (scenarios, checks), save, bin/ (benchmarks)
  data/road_types/ one .ron file per road type
  tests/           integration, golden and trace tests
crates/tf_game/    the Bevy app: runner, snapshot, input, camera, draw, tools/, views/, status line, capture, bench
migration/         golden/*.json and baseline.md (Godot reference data), README.md
docs/              this plan, the migration plan and its notes
```

---

## 4. Roadmap

Each phase ends with a **playable checkpoint** and explicit acceptance criteria. The concept's 4 phases are kept, with a Phase 0 added in front and Phase 1–2 split into smaller steps because they carry most of the technical risk.

### Phase 0 — Foundation
- `git init`, Godot `.gitignore` (already present), install GUT for tests.
- `Game` autoload (owns the `Simulation`, fixed-tick accumulator, speed control, pause) and `Events` bus.
- `SimClock` (in-game time, day counter, traffic period) stepped by `Simulation.step()`.
- `main.tscn` with a camera that pans (WASD/arrows/middle-mouse drag/trackpad) and zooms (wheel/pinch), and a 64 px grid drawn underneath.
- HUD top bar: day, clock, period, speed buttons.

**Done when:** you can pan/zoom around a grid, a clock runs on screen, and space/1/2/3 change sim speed.

### Phase 1a — Road network data + drawing ✅
- `RoadType` (`.tres` in `data/road_types/`), `RoadNode`, `RoadSegment`, `Lane`, `LaneBuilder` (centerline → offset-polyline lanes, right-hand traffic).
- `RoadNetwork` commands `add_road` / `remove_segment` + `validate_road`, `changed` signal relayed as `Events.road_network_changed`.
- `ToolManager` + `RoadTool` (drag between grid points, axis-locked, live red/green preview with reason) + `BulldozeTool` (click or sweep). Keys: R / B / Esc; toolbar buttons.
- `RoadRenderer` (edge + surface `Line2D` layers so junctions merge), `LaneOverlay` debug view (F3).
- Save/load via `Simulation.to_dict/from_dict` → JSON; F5 quicksave / F9 quickload (`user://quicksave.json`).
- Unit tests for network rules, lanes and save round-trip; integration tests drive the real tools with synthetic input.

**Done when:** roads can be drawn and removed, connecting at shared endpoints; the lane overlay shows two opposite lanes per road; a saved layout reloads identically. Unit tests cover `add_road` / `remove_segment` / merge-at-shared-node.

*Notes carried forward:*
- Roads may only meet at endpoints; crossings, T-junctions onto a road's middle and passing through a node are **rejected** with a hint until Phase 2a splits segments.
- Lanes currently run node-to-node and overlap at corners. Phase 1b must trim lanes back from junction nodes (by about half the widest connected road) so connector lanes have room; that makes a segment's lanes depend on its nodes' other segments, so rebuild lanes for all segments touching a dirty node.
- Node/segment lookups are linear scans. Fine for now; add a spatial hash when maps get large (profile in Phase 2b).

### Phase 1b — Lane graph + routing ✅
- Road lanes are trimmed back from junction nodes by half the widest connected road (dead ends untrimmed). Road lane ids are stable for a segment's lifetime; only their geometry is refreshed.
- Connector lanes per node (`Lane.segment_id == -1`, `Lane.turn`): straight / right / left, never U-turns at junctions; **dead ends get a U-turn connector** so no lane is a trap. Multi-lane rule: straight keeps its lane, right turns curb→curb, left turns center→center. Connector speed = slower lane × turn factor.
- `Lane.outgoing_lane_ids` wires road lane → connectors → next road lane.
- `LaneGraph` (AStar2D, one point per lane, cost = travel time × (1 + congestion)), `Route`, `Router` (lazy graph rebuild on network change, path cache per lane pair, same-lane-behind handled by looping).
- `RouteTool` debug inspector (T): click origin and destination on any road lane, see route + free-flow time.

**Done when:** routes are correct on a hand-built test map (including "longer but faster" choices once a second road type exists), with unit tests on a few fixed layouts.

*Notes carried forward:*
- The graph is rebuilt in full on any network change. Fine at this scale; make it incremental if profiling in Phase 2b says so.
- Connector ids change whenever their junction is rebuilt. Vehicles on a connector at that moment must be handled in Phase 2b (re-route or despawn).
- Left turns at 4-ways cross each other's paths; conflicts between connectors are Phase 2b's conflict table.

### Phase 1c — A single car ✅
- `Lot` / `LotSystem`: single-cell lots (Residential green, Industrial yellow) placed with `LotTool` (H / I, click or drag); bulldozer removes lots too. Access point = nearest road-lane position within 48 px of the lot center (= the lane on the lot's side of the road), cached, recomputed after road changes. Lots are saved.
- `Citizen` / `CitizenSystem`: one citizen per home, working at the nearest industrial lot, alternating home → work → home with an 8 s dwell. No route / no work → retry every 2 s. Citizens aren't saved; they're recreated from homes on load.
- `Vehicle` / `VehicleSystem`: follows its route lane by lane, accelerates to the lane limit and brakes in time for slower lanes ahead and for its stop (`v = sqrt(v_next² + 2·decel·gap)`). After any road/lot change every vehicle re-routes from where it is; a vehicle on a just-replaced connector finishes it and continues from the lane it feeds; no route → trip fails and the citizen is sent home.
- `VehicleRenderer`: one `MultiMeshInstance2D`, interpolated between ticks, one color per citizen. `LotRenderer` draws a "driveway" to the access point, or a red outline when there's no access.

**Done when:** the car loops home↔work forever, turns smoothly through connectors, and reroutes correctly if a road on its path is bulldozed (or despawns cleanly if no route exists).

*Notes carried forward:*
- Vehicles drive through each other: car-following and intersection control are Phase 2b.
- Everyone leaves at the same moment, so early convoys render as one car. The Phase 3 schedule will spread departures.
- Access always uses the lane on the lot's side, so reaching a lot on the far side of a road means driving on to a junction or dead end and turning around. Revisit if it looks silly once traffic is dense (e.g. allow left-in at low traffic).
- A vehicle finishing a replaced connector can jump a few px if the junction's trim radius changed.
- Re-routing everyone on every edit is O(vehicles) A* queries (helped by the path cache). Profile in Phase 2b.

### Phase 2a — Crossings & splitting ✅
- `RoadNetwork.plan_road()` (pure, used by both the preview and the build) finds every point where a new straight road meets existing roads or nodes on its layer; `add_road_pieces()` splits existing segments there (`_split_segment`, keeping direction and type), creates the junction nodes and builds the new road as one segment per piece. Pieces running along an existing road are skipped (extending a road works; a road entirely on top of one is rejected). Pieces shorter than `MIN_SEGMENT_LENGTH` (16 px) are rejected.
- Covers crossings (4-way), T-junctions (ending or starting on a road), passing through existing nodes, crossing many roads at once, and collinear overlaps. Other elevation layers are ignored.
- Road tool preview marks the junctions the road will create or join.
- Vehicles on a lane that gets replaced by a split "re-snap" to the lane now under them (same heading, within 2 px) and re-route, so building a crossing under traffic doesn't kill trips.
- Fuzz test: 60 random grid roads with random bulldozing, then all graph invariants + save/load round-trip.

**Done when:** arbitrary grid layouts produce a valid graph (tested by fuzzing random road placements and asserting graph invariants).

*Notes carried forward:*
- Curved segments can't be split yet ("Can't join a curved road in the middle yet"). Phase 9 (curves) needs a proper Bézier split (de Casteljau).
- `add_road` returns only the first new segment id; use `add_road_pieces` when you need all of them.
- Removing a segment never merges the two segments left at a now-degree-2 straight node. Harmless (a straight connector joins them), but a "merge collinear segments" clean-up would reduce node count.

### Phase 2b — Many vehicles & queuing ✅ (performance decision pending)
- Each lane keeps its vehicle queue (`Lane.queue`, front-most first). Each tick: a prepare pass computes every vehicle's gap to the vehicle ahead (looking along its route for the front-most one) and its stop line; then a move pass picks speeds (lane limits, turns, own stop, `sqrt(v_lead² + 2·b·gap)` following plus a 0.8 s headway) and moves, clamped so nobody gets closer than `MIN_GAP` (4 px) or crosses a stop line. Gaps are taken at the start of the tick and vehicles only move forward, so overlaps are impossible by construction.
- Junctions (uncontrolled): per-node conflict table (connectors whose paths come within 8 px, or that feed the same lane). A front-most vehicle near its stop line asks to enter; it is granted only if no conflicting connector is in use or promised, the exit lane has room for the whole vehicle including others already granted ("don't block the box"), and no longer-waiting vehicle at the same junction wants a conflicting move (first come, first served). Grants are never revoked.
- Spawning checks the lane is free (with braking room for the vehicle behind); citizens retry after 0.5 s.
- `StressTraffic` (debug load: random lane-to-lane trips), F6 in game cycles 0/250/500/1000/2000; HUD shows vehicles, stuck (stopped 30 s+) and smoothed tick cost.
- `benchmarks/traffic_benchmark.gd`: headless, 20×20 street grid.
- Not done as planned: vehicles are still objects, not struct-of-arrays. Moving queues onto `Lane`, holding lanes as objects instead of ids, and a math-only pose for straight lanes gave ~30%. Real struct-of-arrays in GDScript would mostly trade object access for packed-array access, which isn't clearly faster.

**Done when:** 500 cars run on a grid without overlapping, without permanent deadlock, at 60 FPS. Record a perf baseline at 1k / 2k / 5k cars → **this decides whether the sim core stays in GDScript** (see §5).

**Benchmark (2026-10-20, GDScript, this laptop):**

| vehicles | avg tick | worst tick | render fill | stuck |
|---|---|---|---|---|
| 500 | 3.3 ms | 7.9 ms | 0.3 ms | 0 |
| 1 000 | 6.9 ms | 13.8 ms | 0.6 ms | 0 |
| 2 000 | 14.2 ms | 22.6 ms | 1.1 ms | 0 |
| 5 000 | 32.9 ms | 41.2 ms | 3.1 ms | 0 |

Roughly 7 µs per vehicle per tick, almost all of it GDScript overhead in the per-vehicle passes (routing is cheap: ~1.4 ms per uncached A*, rarely needed). Budget: at 1× a tick runs every third frame and must fit in one frame with rendering (≲ 10 ms); at 4× ~1.3 ticks run per frame (≲ 7 ms each). So: **500 cars OK at every speed, ~1 000 OK at 1×, 2 000+ not OK** in GDScript. Per §5 this triggers moving `VehicleSystem` out of GDScript (options in §5).

**Decision (2026-10-20): deferred.** Stay in GDScript and continue with gameplay; 500–1 000 vehicles is enough until zones create real demand. Re-run the benchmark once Phase 3 shows realistic city sizes, and try the cheap options (10 Hz tick, threaded tick) before a C#/GDExtension port.

*Other notes carried forward:*
- Vehicles on a junction's old connectors after a rebuild aren't counted against the new connectors' conflicts for the moment they take to clear it.
- Gridlock around a block (A waits on B waits on … A) is still possible under heavy load; the 30 s "stuck" counter makes it visible. Patience-based despawn (Phase 3) is the release valve.
- Lanes and vehicles reference each other: discarded networks/simulations must be `dispose()`d (see CLAUDE.md).

### Phase 2c — Signals & the first jam ✅ (playtest pending)
- Junction control per node (`RoadNode.control`: none / traffic lights, saved by name; only for 3+ roads, reset when a junction loses roads). `RoadNetwork.set_node_control()` command; `SignalTool` (L) toggles it.
- Stop signs existed in this phase and were removed after playtesting (2026-10-20): next to an uncontrolled junction with conflict-zone timing they added nothing, and actuated lights cover busy junctions. Old saves load their stop signs as no control.
- Traffic lights (`TrafficLights`, vehicle-actuated): two phases by road axis (`Lane.signal_phase`). A light rests on green while nobody waits elsewhere; otherwise min green 3 s, extended while its vehicles keep moving, handed over when its approach is empty or blocked (front vehicle standing 1 s+ on green) or after max 12 s; 1 s clearance; phases without waiting vehicles are skipped. Only green lanes may enter; left turns still yield via the conflict zones.
- Junction entry uses conflict *zones* (the stretch where two connector paths come within 8 px) with timing: a vehicle may go if every vehicle on a conflicting connector will have left the shared zone 0.5 s before it could get there. Non-meeting movements run in parallel and waiting vehicles start before the junction is empty. Safety net: inside the junction a vehicle stops short of a zone an earlier entrant hasn't cleared.
- Playtest feedback round 1 (stop signs / lights too slow, one-by-one, start-up delay, lights too long and blind) addressed by the two bullets above. Grid stress test, 250 vehicles, arrivals in 2 sim-minutes: none 674 → 942, stop signs 534 → 623, lights 442 → 703; no zone overlaps or stuck vehicles under any control.
- `CongestionMonitor`: every 5 s, per road lane, `speed_limit / average speed − 1` (smoothed, capped at 10) becomes the lane's routing penalty; the path cache is cleared and up to 50 vehicles that have been held up (below 30% of the limit for 3 s+) re-route from where they are.
- `HeatmapOverlay` (F4): congested lanes from yellow to red, under the vehicles. `IntersectionRenderer`: stop bars / live light colours at each stop line.
- `Scenarios.build_bottleneck` (F7 in game, also used by tests): 28 commuters through one stop-sign T-junction; `build_bypass` adds a 3-road detour. (Since Phase 3: 56 commuters through a light-controlled T-junction.)
- Measured (tests): queue of 17 on the approach, approach congestion 4.0; with the bypass the morning finishes in 43.4 s instead of 65.7 s (−34%), 12 of 28 commuters re-route onto it. After two-sided lot access (Phase 3) the jam mostly vanished, since half the suburb no longer drove to the dead end and back; departures were compressed to 15 s (queue 27, congestion 5.8) and the bypass now skips the lights and reaches the factories from the east end. It draws a few held-up drivers and breaks even, because every factory sits on the one main road.

**Done when:** a single bottleneck intersection visibly jams, adding a parallel road visibly relieves it, and rerouting is observable. ⭐ **First "is this fun?" checkpoint** — play it before going further.

**How to playtest:** run the game, press **F7** (Bottleneck scenario), **F4** (heatmap), **3** (4× speed). Watch the queue form at the lights, then try: build your own bypass (R), toggle the lights (L), zone more (H/C/I), F6 for extra random traffic. For a whole city, press **F8** (Town scenario) and watch a full day. Questions to answer: is watching and fixing the jam satisfying? Is it readable? What's missing to make it a game?

*Notes carried forward:*
- Only vehicles that are held up re-route, at most 50 per 5 s; free-flowing vehicles keep their routes until the network changes.
- ~~North-side homes drive away from their destination to turn around~~: fixed in Phase 3 by two-sided lot access (see the Town scenario notes).
- Lights only know two axis phases; protected left-turn phases and per-junction tuning are later polish.
- Held-up vehicles are re-routed 3 per tick (50 per 5 s), so the route searches don't spike one frame.
- Perf after this round (benchmark): 500 → 4.6 ms/tick, 1k → 10 ms, 2k → 21 ms, 5k → 47 ms (more arrivals per second also means more route searches for new trips).

### Phase 3 — The living city ✅
- **3a Zoning.** `ZoneSystem`: paint R/C/I cells (`ZoneTool`: drag a rectangle; H/C/I, X dezone). A zoned cell next to a road develops into a building (`Lot`) after 2–10 s; cells out of road reach wait (dashed red outline) and re-check when roads change. Rezoning / dezoning / bulldozing removes the building. Zones are drawn with `_draw` rather than a `TileMapLayer` (simpler; revisit if large maps get slow). Vehicles got kinds (car 11×6, truck 17×7) with real lengths used everywhere.
- **3b City life.** `ExternalConnections`: highway points on the map edge (new maps start with one entering from the west, HWY sign). `CitizenSystem`: 2 residents per home, jobs at the nearest shop (2 jobs) or factory (3 jobs) with room, personal schedules (leave home 06:30–09:00, leave work 16:00–19:00, 35% shop 30–60 min on the way home). `FreightSystem`: trucks for raw materials and imports from the highway, goods factory → shop, exports, at a rate peaking 10:00–16:00. (The planned separate `TripScheduler` lives inside these two systems.)
- **3c Score.** Patience: trips are timed against their free-flow time (+2 s); past 2× + 15 s the driver turns red, past 4× + 60 s they give up (failed trip). `FlowMetric`: per trip `ideal / actual` capped at 1, failures 0, averaged over the last game day; HUD shows Flow % (green ≥ 80, amber ≥ 60, red), population and jobs. `DayNight` `CanvasModulate` tint (debug toggle).
- Measured: one day in a small zoned city gives cars/trucks started per period: morning 80/22, midday 0/84, evening 88/24, night 8/20 (flow 88%). Flow reacts to the network: cutting the bottleneck scenario's commute road mid-morning takes Flow from 88% to 30%.

**Done when:** one in-game day plays through all four pulse periods with visibly different traffic patterns, and Flow % reacts to network changes.

**Town scenario** (`Scenarios.build_town`, F8 / debug menu, `tests/unit/test_town.gd`): the end-to-end test bed. A 24 × 16-cell town near the north-west corner: ring road, Main Street in from the west highway, the Avenue, two cross streets, two side streets (one fed by a north highway), a cul-de-sac; lights on the five Main Street / Avenue crossings, other junctions uncontrolled; zoned and developed at once (`ZoneSystem.develop_all_now`): 88 homes (176 citizens) in three districts, 38 shops downtown, 37 factories. The test runs a whole day checking safety on every tick.
- Measured: peak 173 vehicles, ~307 truck trips, 0 failed trips, Flow 78%; the morning rush roughly doubles commute times (median 1.5× ideal) and everyone is at work by 13:30, home by ~02:00. Queues form at the Main Street lights (morning) and on the Avenue / east side street southbound (evening).
- Same town with no junction control: everyone at work 12:55, Flow 83%; with lights on every junction: 15:35, Flow 67%. On single-lane roads, uncontrolled junctions with conflict-zone timing out-perform the actuated lights: drivers there behave like a perfect zipper (0.5 s margins, no gap acceptance), which is unrealistically efficient.
- Two-sided lot access (2026-10-20): a building now offers both lanes of its street (`LotSystem.get_access_points`) and trips take the quickest pair (`Router.find_best_route`), as if drivers turned out of the driveway either way. Before it, homes on the "wrong" side drove to the next junction or dead end and back, roughly doubling some trips. Town after the fix: median ideal commute 13.7 s → 8.7 s, actual 27 s → 19.5 s, everyone home by 23:10 instead of ~02:00, Flow 80%; no control 86%, lights everywhere 73%. (Crossing the near lane to reach the far one is not modelled.)
- Junction realism, round 1 (2026-10-20), because uncontrolled junctions behaved like a perfect zipper and beat lights:
  - Drivers slow to 40% of the road's limit by the stop line of a junction without lights (`LOOK_SPEED_FRACTION`), even when it's empty; at a green light they don't.
  - Gap acceptance (`VehicleSystem._gap_accepted`): besides vehicles already in the junction, a driver gives way to approaching (or waiting) traffic with priority over them if it is less than `CRITICAL_GAP` seconds from the shared stretch: 2.5 s to turn left / U-turn, 2 s straight, 1.5 s right (below real-world gaps because vehicles here cover car lengths ~2.5× faster). Applies at lights too, so permissive left turns on green wait for oncoming gaps.
  - Priority (`_must_yield`, cached per connector pair): straight beats right beats left; equal movements, priority to the right. Never mutual; rings of drivers each yielding to the next break up because accepted gaps shrink to zero over `GAP_SHRINK_TIME` (10 s) of waiting, falling back on first come, first served. Congestion re-routing keeps a driver's place and waiting time in the line.
  - Measured: stress grid (250 vehicles, 2 min) none 708 → 540 arrivals, lights 649 → 540 (a tie, was none +30%); Town Flow none 86% → 74%, scenario lights 80% → 73%, lights everywhere 73% → 69%; no deadlocks, longest stop ~15–19 s. Benchmark (machine noisy that day): 500 → 6.8 ms/tick, 1k → 14.6, 2k → 25, 5k → 53; the old rules measured 7.3 / 12.3 / 46 / 86 in the same session.
  - Next steps considered: road hierarchy (bigger road has priority, once multi-lane roads exist), left-turners waiting inside the box at lights, and making lights clearly better than no control at busy crossings.

*Notes carried forward:*
- Buildings need a road on their cell edge, so blocks deeper than 2 cells leave unbuildable middles. Fine for now; Phase 10+ density/lot sizes may change it.
- Citizens, vehicles, light states and the Flow history aren't saved; a loaded game restarts them (people at home).
- Only one highway; no new residents arrive from outside yet, and goods have no economy (trucks are traffic only). Demand quotas and growth are Phase 10+.

### Phase 4 — Interface for more than one road and zone type ✅
Small, and first, because every later phase adds types the player must pick from.
- **Tool palette.** The bottom toolbar keeps one button per tool family (Roads, Zones, Signals, Bulldoze, Route). A family with variants opens a sub-menu just above it: road types (2-lane, 4-lane avenue, one-way 1/2/3 lanes, later bus lanes) and zone types (Residential, Commercial, Industrial, Dezone). The chosen variant is remembered per family. Keyboard shortcuts stay (R cycles road types while the road tool is active).
- **Hint line** moves just above the toolbar (and above the sub-menu when it's open), next to where the player is looking, instead of the top bar.
- `ToolManager` gains "tool + variant" (e.g. `RoadTool.road_type`), so tools stay single nodes. The HUD builds the palette from data (`data/road_types/*.tres` + the zone list), so a new road type appears without UI code.
- Tests: an integration test clicks a family, picks a variant from its sub-menu, and builds with it; the hint sits above the toolbar.

**Done when:** the player can build every road type and paint every zone from the palette, and the hint reads next to the tools.

*Done (2026-10-20):* `ToolPalette` (`ui/tool_palette.gd`) replaces the toolbar: families Roads (variants = `RoadType.list_all()`, so a new `.tres` in `data/road_types/` shows up by itself), Zones (the four zone tools as entries), Signals, Bulldoze, Route. Tools expose variants (`BuildTool.get_variants/set_variant/cycle_variant`); `Events.tool_variant_requested/changed` connect UI and tools; `ToolManager.use_tool` never toggles off; a tool's shortcut pressed again cycles its variants when it has more than one. The palette follows keyboard shortcuts and remembers the last variant per family. Only one road type exists until Phase 5a, so the Roads sub-menu has a single entry for now.

### Phase 5 — Multi-lane and one-way roads ✅
The lane model was built for this (`RoadType.lanes_per_direction`, and junction connectors already match lanes across a junction). What's missing is the behaviour.
- **5a Road types.** A 4-lane avenue (2+2) and one-way roads (1–3 lanes, a `RoadType.one_way` flag). A segment's start→end is its direction, set by the drag; a "reverse" action flips it. Rendering: lane markings, direction arrows on one-way roads. Upgrading a road in place (drag the new type over it) keeps its nodes and lots.
  - *5a done (2026-10-20):* road types in `data/road_types/`: Local Street, Avenue (2+2, 100 px/s), One-way Street (1 lane), One-way 2 and 3 lanes (`RoadType.one_way`, `palette_order`). One-way lanes are centered, index 0 the leftmost, so the existing turn rules (left turns from index 0, right from the curb lane) apply unchanged. `add_road_pieces` converts pieces running along an existing road of another type, or a one-way road dragged the other way, in place (`RoadNetwork.set_segment_type`, `plan.upgrades`); the converted segment keeps its id and nodes, its lanes get new ids and vehicles on them re-snap. `RoadMarkings` paints center lines (dashed on local streets, solid yellow on wider roads), lane lines and one-way arrows; the road tool previews arrows and names the type in its hint; R cycles road types.
  - Measured on the 5 × 5 stress grid before 5b (250 random trips, 90 s): local streets only 371 arrivals; avenues + local 288 (238 in the test's run), avenues only 430, local + one-way 197 — none stuck. Avenues + one-way streets together **gridlock** (94 arrivals, 148 stuck): without lane changing, a car in the wrong lane of an avenue must circle the block to reach its turn, and the circling traffic locks up. The same restriction is why a grid with avenues delivers fewer trips than plain streets. The mixed test was `pending` until 5b.
- **5b Lane choice and lane changing.** Routes stay lane-level, but a vehicle may use any parallel lane of the same road. The route fixes the lane it must be in *at the next junction* (the lanes whose connectors lead where it's going). It changes lanes before that, in gaps (gap acceptance on the target lane, reusing the junction gap logic), preferring the faster lane and keeping right otherwise. Vehicles that miss their lane re-route rather than stop dead. Lane-to-lane turn rules: the left lane turns left, the right lane turns right, the middle goes straight (overridable later).
  - *5b done (2026-10-20):* the lane graph links each road lane to the lane next door going the same way (`Lane.is_beside`), at a fixed `LaneGraph.LANE_CHANGE_COST` (1 s) instead of the lane's length, so routes stay lane lists and A* picks where to change; `Route.get_runs` groups lanes driven side by side for timing and drawing. `VehicleSystem._change_lanes` (each tick, before the prepare pass): a vehicle whose next route lane is beside it moves across when there's room for it and both it and the vehicle that ends up behind can brake comfortably; otherwise it drives on and stops at the end of its lane if needed. Within `MERGE_DISTANCE` of the end (or stopped) it is *merging*: the first vehicle behind on the target lane holds back if it can stop comfortably (zipper). Two vehicles side by side wanting each other's lane swap. A merging vehicle stopped at the front of its lane for `MISSED_LANE_WAIT` (2 s) gives up, goes on from its lane and re-routes (`Router.find_route_onward`), and re-routing near the end of a lane never plans a lane change there. Drawn sliding across (`Vehicle.lateral_offset`).
  - Measured (stress grid, 250 random trips, 90 s, before → after): avenues + local 288 → 514 arrivals, avenues only 430 → 611, avenues + one-way 94 (gridlocked) → 394, local + one-way 197 → 157–277 depending on the random trips (at 250 that grid is at the edge of real gridlock; at 200 every seed flows). Town with Main Street and the Avenue upgraded to avenues: everyone at work 12:47 instead of 15:29, Flow 72% → 77%. Benchmark unchanged.
  - Left for later: lane choice by speed ("keep right unless overtaking") beyond what congestion re-routing gives; cooperative merging only covers the first vehicle behind.
  - Junction spacing (2026-10-21, from playtesting): lanes are cut back from each junction by half its widest road, so two avenue junctions one cell apart left 8 px lanes no vehicle fits, and traffic crawled through one at a time (the avenue carried half as much). `RoadNetwork.plan_road` now refuses roads that would leave (create, convert or shorten) a lane shorter than `MIN_LANE_LENGTH` (a truck and a gap) between junctions: local streets can still meet one cell apart, avenues need two. Merging close junctions into one (a boulevard crossing an avenue) is a possible later feature.
- **5c Junctions with many lanes.** Conflict zones per lane pair (more connectors, so check the cost); parallel non-conflicting turns from different lanes; light phases per approach with an optional protected left. Road hierarchy for priority at junctions without lights: the bigger road has priority and side roads yield, with priority to the right only between equal roads. This replaces the round-1 tie-breaker.
  - *5c done (2026-10-20):* straight movements from parallel lanes already ran side by side (their connectors are 14 px apart, beyond `CONFLICT_DISTANCE`). Road priority: `RoadType.priority` (avenues and multi-lane one-ways 1, local streets and the 1-lane one-way 0); `VehicleSystem._must_yield` puts the bigger road first (even its left turns), then the turn ranks, then priority to the right; drivers on a junction's priority road don't slow down to look (`_entry_speed`, cached per connector). Protected left turns: a third junction control, `TRAFFIC_LIGHT_PROTECTED_LEFT` (Signals tool: none → lights → lights with protected lefts); light phases now belong to connectors (`Lane.signal_phase`): axis 0/1, or with protected lefts 0 axis-0 through, 1 axis-0 left, 2 axis-1 through, 3 axis-1 left; `TrafficLights` detects demand by the connector each front vehicle is heading for and skips phases nobody wants; stop bars show the best light among a lane's turns.
- Tests:
  - lane changes are safe (no overlaps during a change, using the shared checks);
  - vehicles reach the correct turn lane before the junction;
  - one-way roads are never driven backwards (routing + a stress test);
  - a 4-lane arterial carries roughly twice a 2-lane road's flow.
- Measure: a Town variant with Main Street as a 4-lane avenue and a one-way pair downtown; the benchmark at 500/1k/2k (lane changes add work per vehicle).

**Done when:** a 4-lane arterial visibly relieves a jam that a second 2-lane road wouldn't, and a one-way grid downtown flows better than the same grid two-way.

### Phase 6 — Driveways: cars leave and arrive physically (6a ✅)
Today a trip starts and ends instantly on the lane in front of the lot. Real departures and arrivals cause friction, and the player should see it near busy shops.
- **Leaving.** A car starts in the lot's driveway (off the road) and waits for a gap to pull out. To the near lane it merges (a right turn). To the far lane it crosses the near lane (a left turn out, needing a gap in both directions). It enters slowly and accelerates in traffic. Blocked driveways queue cars on the lot, counted as waiting and shown on the lot.
- **Arriving.** The car slows to driveway speed before its lot, so the cars behind it brake. Turning into a far-side driveway waits for an oncoming gap and holds up the queue behind it (the classic left-turn-in jam). The car then leaves the road. Trucks take longer: a wider turn and loading time on site.
- **Parking capacity** per lot: homes a few spaces, shops and factories more by size. When a lot is full, cars go to the next lot of the same kind, or park further away and walk (ties into Phase 7). Kept simple: no on-street parking yet.
- Driveways are short connector-like lanes from the lot to the road, with conflict zones against the road's lanes. They reuse junction grants, gap acceptance and the safety checks.
- Tests:
  - a car pulling out never conflicts with through traffic;
  - a left-in arrival blocks the lane behind it until it can turn;
  - in the Town, busy shop streets show more delay than residential streets;
  - Flow is still scored against free-flow time, with driveway time included in the ideal.

**Done when:** a commercial street lined with busy shops visibly slows through traffic, and giving the shops a side street or back access road measurably improves Flow.

*6a done (2026-10-20): leaving and arriving.* Trips start with `VehicleSystem.depart(origin lot or lane points, destination lot or points)`: the vehicle waits in its lot's driveway (drawn on the lot, nose to the road; `get_waiting_departures`) and the first one per lot pulls out when the lane has a gap (`_clear_to_join`: nobody alongside, traffic behind a right turn's critical gap away and able to brake). Joining the far lane is a left turn out: the lanes on the lot's side need a gap too (`_clear_to_cross`), and after 8 s it settles for the near side. Arriving vehicles slow to driveway speed (15 px/s, trucks 10) so followers brake; arriving on the far lane means stopping to wait for a gap in the oncoming lanes (the gap shrinks to nothing over 10 s), holding up the queue behind, and giving up after 5 s to come round from the near side. After 3 s of waiting (out or in), the next vehicle on the lane to join or cross holds back if it can stop comfortably. Route choice adds `LotSystem.FAR_SIDE_PENALTY` (4 s) to far-side access, and far-side access is the inner lane only. A fairness rule at junctions came out of the testing: the space in a nearly full exit lane goes to whoever has waited for it longest (`EXIT_FAIRNESS_WAIT`), which fixed turning traffic being starved at lights on random seeds (it predates driveways).
- Freight rates were lowered by a third: trucks that found no room to appear used to be dropped silently (about 30%), which is no longer the case.
- Playtest fixes: into a crawling queue the next car lets a waiting driver out after 0.5 s (3 s into a moving stream), but only if the space in front of the driveway is free or the car there is moving on (holding back otherwise just blocks the road); shoppers pick one of the 4 shops nearest their work at random (all going to the nearest one piled up its driveway); at most 3 waiting cars are drawn per lot. With 300 extra random trips the Town is overloaded: up to ~125 cars wait in driveways at the evening peak, draining overnight.
- Measured (Town): Flow 72% → 58%, 1 failed trip; median wait to pull out 1.7 s (p90 9 s), up to ~45 cars waiting at once at the evening peak. Stress grids unchanged.
- *6b (to do): parking capacity per lot* — a full lot sends cars to the next lot of the same kind; needs walking (Phase 7) to be meaningful, so it may move there.

### Phase 7 — Pedestrians and walking ✅ (7a/7b; parking moves to Phase 8)
- **Sidewalks** as pedestrian lanes, generated with each road type (both sides; none on highways). **Crosswalks** at junctions as pedestrian connectors, with conflict zones against vehicle connectors. Mid-block crossings later.
- **Pedestrians** are lightweight agents on pedestrian lanes: no overtaking physics, just speed limited by crowding.
  - At a junction without lights, cars yield to pedestrians on the crosswalk, and a pedestrian waits for a gap in cars before stepping out.
  - At lights, pedestrians get a walk phase with the parallel traffic, so turning cars wait for them.
- **Walking trips**: citizens walk when the destination is close, and later to and from bus stops and parking. Walking disturbs traffic only where it crosses roads. That's exactly what the player should design around: pedestrian-heavy downtowns, crossings away from busy junctions.
- **Performance gate** before this phase: pedestrians at least double the agent count. Decide then whether they share `VehicleSystem` or get a cheaper system of their own, and revisit the C#/GDExtension option (see Key Risks).
- Tests:
  - no pedestrian is ever on a crosswalk while a car is in the same conflict zone;
  - short trips are walked;
  - a busy crossing reduces car throughput at its junction.

**Done when:** a downtown with heavy foot traffic slows turning cars, and moving a crossing or adding lights changes it visibly.

*7a/7b done (2026-10-21):* `sim/walking/`. `WalkNetwork` derives the walkable graph from the roads (rebuilt lazily on change): a sidewalk along each side of every road (5 px beyond its edge, trimmed like the lanes), corners joining neighbouring roads' sidewalks around each node (by the angle of each road leaving it), a walk round dead ends, and at junctions of 3+ roads a crosswalk across each road's mouth. `PedestrianSystem` walks people door to door at 30 px/s (a third of a car), stopping at the curb of a crosswalk until no car is in the junction on a turn over it (`VehicleSystem.is_crosswalk_busy`) and, at lights, until the walk light is on (`TrafficLights.is_walk_green`: green for the other road axis' through phase; waiting pedestrians call it). Cars give way: a turn over a crosswalk (the road it leaves or enters) isn't entered while someone is on it or waiting with the right to go (`_crosswalks_clear`). The shared state is `Crosswalks` (who is on / waiting at each). Citizens walk trips up to 4 cells (`MAX_WALK_DISTANCE`); walks count in Flow. Pedestrians are dots (`PedestrianRenderer`), crosswalks zebra stripes. The pedestrians' own system turned out cheap enough (no pedestrian-pedestrian interaction), so the Phase 7 performance gate needed no port.
- Measured: Town, 28 of ~420 daily trips are walked (homes and jobs are mostly further apart), Flow 60%; grid with random walks and traffic: 195 walks a minute, never a pedestrian on a crosswalk with a car on a turn over it.
- Left for later: crowding (pedestrians don't slow each other), mid-block crossings, and parking (6b) together with car availability in Phase 8 — walking to work and driving home is still possible, since the car's whereabouts aren't tracked yet.

### Phase 8 — Buses and mode choice
- **Buses** (was Phase 4): bus stops on lanes (curbside, where the bus blocks its lane while it waits; bus bays where it pulls out of the lane come later), `BusLineTool`, buses on fixed routes with a timetable or headway, capacity, and boarding time per passenger.
- **Multimodal trips.** A trip is a list of legs, each with a mode: walk, car, bus (later tram, metro, train). A multimodal router searches a combined graph: pedestrian lanes, road lanes (car), and bus lines (edges between stops, with an expected wait of half the headway).
- **Mode choice rules**, applied per citizen and per trip:
  - at most 3 different modes per trip, and a cap on transfers;
  - pick the lowest generalized cost: in-vehicle time + walking time ×2 + waiting time ×2 + a penalty per transfer, with an optional small per-mode preference per citizen so not everyone makes the same choice;
  - walking legs are limited in length (~10–15 game minutes), and a trip is walked entirely only when it's short;
  - car availability: a citizen without a car (or whose car is parked elsewhere) can't drive, and one who drove out drives back;
  - re-evaluated for every trip, so congestion and new bus lines shift people between modes over the days.
- Flow scores each trip by its total door-to-door time against the best free-flow multimodal time. Transit trips are scored like car trips.
- Tests:
  - a citizen near a bus line to their job takes the bus when it's quicker and the car when it isn't;
  - no trip uses more than 3 modes;
  - adding a bus line moves commuters off a jammed road and Flow rises.

**Done when:** a bus line on a congested corridor measurably raises Flow, and the share of each mode is visible (HUD / debug panel).

*8a done (2026-10-21): trips as legs, car availability, parking, walk-or-drive choice.* A citizen's trip is a list of legs (`Citizen.legs`, walk or car) run one after the other by `CitizenSystem`. Each citizen owns a car (`has_car`; ownership below 100% waits for buses, so carless people have something better than a long walk), parked where they left it (`car_lot_id`): they can drive only from there, walking to it first if it's within `MAX_CHOSEN_WALK` (4 cells). `Parking`: homes hold their residents' cars; shops have 3 spaces and factories 4, shared by workers and visitors, taken when the trip is planned. A driver whose destination is full parks at the nearest lot with a free space within walking reach and walks on (else on the street, over capacity). Mode choice (`plan_trip`), by generalized cost in sim seconds: walking time × 1.5, driving = expected time with congestion + 9 s overhead (getting the car out, parking: big next to drives this short, since distances are compressed), × a personal taste for driving (0.85–1.2). Chosen walks are at most 4 cells; with no other way people walk however far; someone going home whose car isn't there drives it home. Flow now scores citizens' trips door to door (`CitizenSystem.trip_ended`; their car legs are flagged `Vehicle.scored_by_owner`), against the sum of the legs' free-flow times; it also keeps the mode share of the last day (`FlowMetric.get_mode_shares`, shown in the HUD).
- Measured: on the same side of a street, the average citizen walks up to ~4 cells (cost 14.3 vs 14.8 by car); across a road with no crosswalk nearby, they drive. Town: 5% of trips walked, Flow 58% (60% before; random draws changed, and 4 drivers gave up in the morning jam instead of 2). (8b retuned these numbers, see below.)

*8b done (2026-10-21): buses, and the bus in mode choice.* `sim/transit/`: `BusStop` (a point at the curb, on a road's outermost lane, for one direction; kept as position + direction and found on the lanes again after every road edit, moving out to the new curb if the road is widened), `BusLine` (a loop through its stops; free-flow and measured times between stops), `TransitSystem`.
- Buses are vehicles (`Vehicle.Kind.BUS`, as long as a truck) with `in_service`: at the end of each route (a stop) VehicleSystem keeps them on the road, `dwelling`, until TransitSystem sends them on (`VehicleSystem.set_route`); they never give up. Stops are request stops: a bus dwells only where someone gets off or waits for its line (1 s + 0.2 s per rider), blocking its lane; elsewhere it drives past. Each line runs one bus per 10 s of its loop (max 12), spawned one by one at its stops; measured stop-to-stop times feed the planner.
- Riders: a bus leg makes the citizen wait at the stop (`wait_for_bus`, drawn as dots by the sign), board the first bus of the line with room (40), ride and get off at their stop. They give up after 3 headways (≥ 30 s), or if the stop or line goes.
- Planning (`plan_ride`): Dijkstra over (stop, line) states: walk to a stop within 4 cells (× 1.5), wait half the headway (× 2), ride, change lines at the same stop or one within 4 cells' walk (5 s penalty, at most 2 changes), walk from the last stop. `CitizenSystem.plan_trip` compares car, bus and walking; a whole trip on foot is an option however far (otherwise a carless rider would choose a nearly full loop over a shorter walk). 80% of citizens own a car.
- Flow's yardstick for every citizen trip is now capped at the drive between the two places on empty roads + 4 s (`CAR_ACCESS_TIME`): walking across town, a slow bus or parking far away count as slow even when nothing held them up. Buses aren't scored themselves (`scored_by_owner`; they were by mistake at first, which inflated Flow by ~20 points).
- `BusLineTool` (U, palette "Transit"): click roads to place stops in order (each side of a street is its own stop; clicking near an existing one reuses it), click the first again or right-click to open the line; right-click a stop to remove it. `TransitRenderer` draws lines as coloured bands, stops as signs with their waiting riders. Stops and lines are saved.
- Realism retune along the way: pedestrians walk at 15 px/s (a sixth of a local-street car, the real ratio in town; it was a third); whole trips on foot cost their time, walks to and from the car or a stop count × 1.5; car overhead 10 s. At lights, people start crossing only in the first 4 s of their green (flashing don't-walk after), so turning traffic gets the crosswalk before the green ends; on a resting green waiting pedestrians get the walk again every 12 s. Without lights, a driver who has waited 4 s at a busy crosswalk goes once it's empty (`CROSSWALK_PATIENCE`). Both were needed once walkers took twice as long to cross: turning cars starved for minutes.
- Town scenario: two bus lines crossing downtown (Main Street, the Avenue), each there and back. The morning-rush check now counts drivers only: someone without a car walking across town takes hours at this clock (a game day is under five sim minutes).
- Measured. Town: car 67%, walk 33%, bus ~0% (for a 1–1.5 km trip in a small town, walking ties with walk + wait + ride + walk, as in real small towns), Flow 56%. Bottleneck (one line along the main road): bus 21% of trips, carless commutes 69 s on foot → 54 s by bus, overall Flow 51% → 50%: the six buses dwell in the jammed single lane, and drivers don't switch (door to door the bus takes about twice as long as the car, which is realistic too).
- So "Done when" isn't met yet: buses help people without a car but don't raise Flow on a congested corridor. What does it in real cities, next (8c): **bus bays** (the bus pulls out of the lane to dwell; pulling back in waits for a gap like a driveway) and **bus lanes** (a road type, or a lane of the avenue, only buses may use, so they skip the jam), then the player sets each line's bus count (it's automatic now) and parking gets scarcer downtown as the city densifies.

*UI pass 1 (2026-10-21), between 8b and 8c: making the city readable.*
- Lots stand on plots: the cell less what the roads around it take (half the road + sidewalk + 2 px). The building sits at the back, with a marked parking strip at the front facing its road, and the parked cars are drawn in the stalls (homes: one stall per resident's car; shops 3, factories 4). Lots no longer overlap avenues. A road that leaves under 16 px removes the lot, and the zone waits for room: an avenue one cell from another road does that, while two local streets one cell apart still leave room. Kept the 64 px grid: halving it would make lots too small for a building and its parking, and would touch every road rule.
- Vehicle colours by kind: cars green, trucks brown and bus-sized, buses in their line's colour (blue, pink, purple, yellow). Red for late drivers is now a debug toggle (F2, "Lateness colours"): every vehicle shades green → yellow → red as it falls behind its ideal time, red at the point its driver turns impatient.
- Fixed along the way: sim events relayed to the views (lots, zones, hours) were only flushed when the worker happened to be idle after the next batch had started, i.e. hardly ever while the game ran; views missed lot and zone changes until something else redrew them.

*8c done (2026-10-21): buses keep to their line, bus bays, bus lanes. Phase 8 done.*
- Buses keep to their line: each stop-to-stop route is fixed when the line is laid out (or the roads change, `BusLine.routes`) and buses drive exactly that; congestion rerouting leaves them alone. Before, they asked the router again at every stop and often went other ways round, mostly to turn back at the ends.
- Bus bays (Transit tool: "Curbside stops" / "Bus bays"; clicking an existing stop makes it the current kind, `TransitSystem.set_stop_bay`): a bus dwelling at a bay leaves its lane's queue (`VehicleSystem.pull_into_bay`), so the traffic behind drives past; sent on, it waits for a gap to pull back in (`_leave_bays`, like a car from a driveway), and after a second the next driver lets it in (`_let_buses_out`).
- Bus lanes: road type "Avenue with Bus Lanes" (`RoadType.bus_lanes`: the curb lane each way, `Lane.bus_only`). Junction turns leave bus lanes out of the general lanes: cars turn right from the lane next to the bus lane; a bus lane goes straight on into the next road's curb lane (its bus lane if it has one) or turns right; buses may also turn right straight into a bus lane; turns into or out of a bus lane are bus-only. Cars route on a graph with no way along bus lanes, only across them to reach the curb (driveways); buses route on the full graph (`Router` `bus` arguments, a second LaneGraph built only when bus lanes exist, sharing congestion costs). Drawn as a reddish tint behind a solid line.
- Measured on the Bottleneck (2 game hours of morning rush; carless commuters otherwise walk ~70 s):

  | Setup | Flow | Car commute | Bus commute |
  |---|---|---|---|
  | No bus | 51% | 22 s | – |
  | Curbside stops | 50% | 25 s | 48 s |
  | Bus bays | 49% | 25 s | 52 s |
  | Bus lanes (one lane each way left for cars) | 55% | 20 s | 43 s |

  Bus lanes meet the "done when": Flow +4, and even drivers get there sooner (buses no longer stop in, or merge into, the jammed lane). Bays don't help on a jammed corridor (what a bus saves at the stop it loses waiting to pull out); they're for buses on free-flowing roads. Drivers still don't switch to the bus (door to door it takes about twice as long, mostly walking and waiting), which is realistic for a suburb. Transit tuning is left for later. "Bottleneck + bus lanes" is in the debug menu.

*Town updated with every feature (2026-10-21). From now on the Town gets each phase's features as part of the phase.* Main Street is an avenue (bus bays along it), the Avenue has bus lanes, the cross streets are a one-way pair (eastbound north, westbound south) with the Crosstown bus loop on curbside stops, and the Avenue × south cross street has protected left turns. `test_town_uses_every_feature` lists what it must show. Putting it all together exposed gaps, fixed realistically:
- Cars stuck in a bus lane (out of a driveway just before a junction, or waiting to turn in) had no way on: cars may now use a bus lane to turn right (as in most cities), so right turns out of bus lanes are open to all.
- Drivers right behind a bus dwelling at a curbside stop now pull out round it into the lane beside, if it goes their way (`_pass_stopped_buses`); before, two-lane one-ways queued behind every stopped bus.
- At lights, a turning driver who has waited 5 s may go on amber, the "sneaker" at the end of the green (`SNEAK_WAIT`); the green isn't cut while people who stepped out on its walk light are still crossing, and lasts 2 s more for the turners they held up (`TURN_WINDOW`); at four-phase lights, through greens are capped at 8 s and protected-left ones at 5 s, so a cycle stays about as long as elsewhere.
- Bug: a driver waiting to turn into its driveway, then re-routed, kept its turn-in timer; when it fired it replaced the route without releasing a junction it had been let into, leaving stale junction bookkeeping (a crash). Re-routing now resets the turn-in state and the arrival crossing.
- Protected lefts first went at the downtown crossing, then at Main Street × the east side street: both jammed the morning (a four-phase cycle at the busiest junctions); at the Avenue × south cross street they cost nothing.
- Measured: Town, 0 failed trips (13 before passing buses, 21 before that), Flow 59% (56% before the update), 95% of drivers at work by 13.6 h. The night check now allows people without a car still walking home (crossing town on foot takes hours at this clock), like the morning one.

*Bus reach (2026-10-21).* Town bus use was 0.7%: 22 of 40 people without a car had no stop within reach, since streets could only be crossed at junctions. Every stop now gets a crosswalk across its road just behind where the bus stands (stops facing each other share one; `TransitSystem._update_crossings`, `Crosswalks.mid_block`, drawn as a zebra): drivers stop for people on it or waiting at it when they comfortably can, people wait while a car is on it or too close to stop, and a driver held up by people waiting goes after `CROSSWALK_PATIENCE`. The Town got a fourth line, the Ring, clockwise past the homes on its north and south edges. Result: every carless commuter has a stop near home, bus share 4.8% of the day's trips, Flow 60%. Car owners still never choose the bus (median 46 s dearer).

### Phase 9 — Curves (was Phase 4)
- Curved road drawing (control-point drag, minimum radius). Everything else should "just work" thanks to the lane model, so this phase still validates the core architectural bet. It comes after the simulation phases because simulation comes before mechanics; it can move earlier at any time, since nothing above depends on it.

**Done when:** a curved bypass changes Flow like a straight one would, with smooth driving through the curves.

*Phase 9 done (2026-10-21).* Curved roads are dense polylines (a quadratic Bézier sampled every 8 px, `RoadNetwork.curve_path`), so everything built on polylines works unchanged: lanes, junctions and connectors, splitting where other roads cross (both ways), sidewalks and crosswalks, saves. Road planning works on paths (`plan_path`, `add_path_pieces`, `add_curved_road`; straight roads are two-point paths). Curves tighter than a 56 px radius are refused (a quarter turn within one grid square is too tight, within two fine), and each lane's speed limit drops round a bend so sideways acceleration stays under 80 px/s² (v = sqrt(a r): a 2-square quarter turn is taken at ~80-88 px/s). Lot plots keep clear of diagonal and curved roads. Road tool: V toggles curved mode (drag from the start to the bend point, release, click the end). The Town's ring road has two curved corners.
- Measured: Bottleneck Flow with no bypass 50%, a straight bypass 50%, the same bypass with curved corners 51% ("done when" met); traffic round a loop of curves never exceeds a lane's limit, no overlaps or conflicts.
- Found along the way: at a permissive light on a busy avenue, a left-turning truck could wait 32 s for a gap. Turners who have waited 5 s may now go up to 2 s after their light turns red, if the junction is clear (a 2 s amber for everyone did it too, but cost every light capacity). Dashed lines now stay dashed round curves.

### Polish track — graphics and gameplay (from 2026-10-22, before new systems)
**Left behind in the migration (2026-11-11), to redo in Bevy:** the art items below (buildings, vehicles, people, roads, ground, day / night, effects) and the UI items (palette, HUD, debug panel) were Godot shaders and scenes and were not ported; the Bevy build draws flat shapes and one status line. The gameplay items are all in the Rust build.

The simulation is ahead of how the game looks and plays: everything is drawn with flat rectangles, lines and dots, and the only way to understand what happens is the debug panel. This track makes the existing systems readable and pleasant to play, without adding new ones. Order within each group is a suggested priority.

**World visuals**
- [x] Buildings with a look per zone and size (roof shapes, a few house / shop / factory styles; shadows), instead of flat coloured rectangles. *Done (2026-10-22):* `BuildingPainter`, style picked from a seed per lot (its cell), so a lot keeps its look. Houses: hipped or gabled roofs in five colours, lit on the north-west slopes, in a garden with a tree when there's room. Shops: flat roofs with a parapet, a striped awning on the street side, rooftop units, two shops on wide lots. Factories: sawtooth or ribbed roofs, yellow-edged loading doors on the street side, sometimes a silo. Everything casts a shadow down-right by its height. Parked cars moved to a layer of their own, so buildings are only redrawn when lots or roads change.
- [x] Vehicles as small top-down sprites (car, truck, bus) with a slight per-car tint; brake lights when slowing, indicators when turning or changing lanes, headlights at night. *Done (2026-10-22):* drawn by a shader (`view/vehicle.gdshader`) on the same single batch, so no textures and no extra draw calls: cars (bonnet, windscreen, roof, rear window), trucks (cab, cargo box with ribs), buses (roof panel, air-conditioning unit, windscreen). Cars come in shades of green (per vehicle id). Brake lights when slowing harder than 15 px/s² or nearly stopped (from the speed change between snapshots), with a red glow behind. Indicators (`VehicleRenderer.indicator_for`, captured in the snapshot) blink within 50 px of a turn or a driveway, while waiting for or making a lane change, and pulling out of a bus bay. Headlight beams fade in with the day / night tint (off when it's toggled off). Parked cars use the same sprite (nose in, lamps off); lots no longer draw a driveway line to the road. Lane changes (and pulling out of bus bays) are now steered, not strafed: the drawn offset follows an S-curve over about five times the distance across as the vehicle drives on, with the nose turned along it (up to ~17°); a stopped vehicle still finishes within 2 s.
- [x] People as tiny figures (or at least a darker outline) so they read against sidewalks; riders visible boarding and getting off. *Done (2026-10-22):* `view/pedestrian.gdshader` draws a person from above (shoulders in a clothes colour per person, dark hair, a dark outline), facing the way they walk. Riders waiting at stops are drawn the same way by `PedestrianRenderer`, in a row facing the road. Boarding: when a stop's boarded count goes up (`BusStop.boarded`, in the snapshot), riders are drawn walking from the front of the queue to the bus's front door, one per `BOARD_TIME`. Getting off is real: riders now walk on from the bus door (`rider_arrived` carries `TransitSystem.get_door_position`; `PedestrianSystem.start_from`), across to the sidewalk.
- [x] Roads: asphalt texture or subtle noise, kerbs and sidewalks drawn (sidewalks exist in the sim but aren't drawn), cleaner junction corners on avenues and curves. *Done (2026-10-22):* `RoadRenderer` layers per segment: a concrete sidewalk strip out to where lots start (`LotSystem.ROAD_CLEARANCE`, people walk on it), a light kerb, the road type's edge colour as a gutter, the asphalt. Inside corners where roads meet are rounded (`RoadRenderer.corner_fillets`: a 10 px kerb radius, the back of the sidewalk concentric with it; none on the outside of bends or between roads running straight on). Asphalt and sidewalks get a soft world-space grain (`view/surface_grain.gdshader`), seamless across pieces.
- [x] Ground: terrain colour variation, trees and small parks on unzoned land, so the map isn't a flat brown grid. *Done (2026-10-22):* the ground is grass (`GridRenderer.map_color`) varied by `view/ground.gdshader` (broad lusher / drier patches, tufts, grain, in world space); grid lines moved to a child layer, untouched by it. `NatureRenderer` (view only, derived from the map) puts trees on unzoned, unbuilt land, clustered into woods by a slow value noise (up to 10 per cell, a lone tree now and then elsewhere). (Small parks beside roads were tried and dropped.) Trees keep to what roads and sidewalks leave free (`LotSystem.compute_plot`, plus an exact check against nearby road pieces, which also catches road ends). Trees are one batch (`view/tree.gdshader`: lumpy canopy lit from the north-west, shadow). Laying out the whole map from the roads takes ~100 ms, done only on road edits; zone and lot changes just filter the cached layout. Same cell, same trees.
- [x] Grid: fainter, and hidden outside the build tools. *Done (2026-10-22):* the grid fades in (0.15 s) only while a tool that snaps to it is active (`BuildTool.shows_grid`: roads and zoning) and is hidden otherwise; the map border always shows. On the new grass the old line strength was nearly invisible, so while shown it is a little stronger than before (white 14% / 24% for major lines).
- [x] Day / night: lit windows, street lights, headlights; a softer night tint. *Done (2026-10-22):* the night tint is lighter (0.48, 0.52, 0.72). `NightLights`, a CanvasLayer that follows the camera (so `DayNight`'s tint doesn't darken it), adds warm glows (`view/light_glow.gdshader`, additive): street lamps every 56 px, staggered on both sides of every road by the kerb, and lit windows along the street side of buildings (homes: the house itself, `BuildingPainter.house_rect`). Windows switch on and off on their own schedules (`NightLights.is_window_lit`): homes from dusk until 21:30-01:00 and early in the morning until 6:15, shops until 20:00-23:00, factories until 19:00-21:00 with a third lit all night. Lights fade in with `DayNight.night_amount` in the evening, but in the morning are out by 6:15 (`NightLights.light_amount`), well before the dawn tint is gone; off with day / night lighting. Headlights were already done with the vehicle sprites.
- [ ] Map edges and highway connections drawn as real roads leaving the map, not a sign.
- [ ] Zoom levels of detail: markings, people and parked cars fade out when zoomed far out; simpler shapes for thousands of vehicles.

*Fixed along the way (2026-10-22):* right-turners at the Avenue × Main Street lights stood in their turn for 4-5 s behind a driver stopped to turn into a corner shop whose driveway was 4 px past the junction (a lot's access point was simply the nearest point on the lane). Driveways now keep `LotSystem.DRIVEWAY_SETBACK` (a truck length and a bit) from both ends of their lane. The other cause of right-turn waits on green, anyone on a crosswalk holding the turn even on its far half, is left as it is for now.

*Fixed along the way (2026-10-22):* adding a road froze the game for about half a second (Town: ~400 ms in one frame). Now about 20 ms, one long frame: the tree layout is redone only for cells near the changed roads (`NatureRenderer` indexes which cells each segment passes near), and its batch is joined from cached per-cell data, per 8×8-cell region, redoing only regions a road edit or a zone / lot change touches; `LotRenderer` draws buildings per 8×8-cell region and redraws only regions whose lots look different; `LotSystem` re-fits plots and clears driveway access only for lots near the changed roads (it keeps each segment's bounds, so it knows where removed roads were); `RoadSegment` caches its polyline and bounds, which `compute_plot` and `find_lane_position` use to skip far-away roads. Left: re-placing every street lamp (~3 ms) and recomputing every junction corner (~5 ms) per edit.

**Transit visuals**
- [ ] Bus stop shelters (and bays) drawn as objects, with a line-colour badge per line serving the stop.
- [ ] Line paths drawn as clean offset ribbons beside the road (one per line where several share a road), not a band over the lanes.
- [ ] Buses with a destination / line number label when zoomed in; a rider-load indicator.

**HUD and UI**
- [ ] Icons on the build palette (roads, zones, transit, signals, bulldoze) with text tooltips, instead of text buttons.
- [ ] A cleaner top bar: clock, period, Flow with a mini trend, speed buttons; the debug stats (vehicles, tick ms) moved to the debug panel.
- [ ] A city panel: population, jobs, mode share (with a 24 h chart), Flow history, worst trips.
- [ ] "Why is Flow low?": the Flow breakdown by cause (jams, failed trips, long walks, waiting for buses) with buttons that jump the camera to the worst spots.
- [ ] Notifications that point somewhere (click to locate): gridlock forming, trips failing, a stop with stranded riders, a zone waiting for a road.
- [ ] Keyboard speed controls (space pause, 1-3 speeds) shown on the buttons.
- [ ] A consistent theme (font, panel style, colours) across HUD, palette and panels.

**Inspecting things (click to see)**
- [ ] Click a building: its type, residents or workers, where they work or live, their last trips, its parking use.
- [ ] Click a vehicle or person: where they're going, by which mode, how late; follow them with the camera.
- [ ] Click a junction: control type, light phases and timings, queues per approach, what's blocking.
- [ ] Click a road: type, speed limit, volume and congestion, lanes.
- [ ] Click a bus line or stop: stops, buses, headway, riders waiting and carried, loop time.

**Building and editing tools**
- [ ] Undo / redo for every build action.
- [ ] Road tool: show length, junctions and what will be removed before building; a clear direction arrow for one-ways; a button to flip a one-way.
- [ ] Bulldoze: drag a rectangle; remove bus stops and lines too (now only by right-click in the bus tool); confirm before removing buildings.
- [ ] Bus lines: edit a line (add, move or remove stops), rename and recolour it, set its number of buses; a line list panel.
- [ ] Traffic lights: show the phases when the junction is selected; per-junction light timing.
- [ ] Zoning: brush and fill shapes; show which cells will develop and which wait for a road or for room.
- [ ] Palette state remembered (last road type, curve mode) and shown on its button.

**Game flow**
- [ ] Main menu: new game, load, scenarios (Bottleneck, Town, ...), settings, quit.
- [ ] Save / load with named slots and a menu (now only F5 / F9 quicksave).
- [ ] Settings: fullscreen, vsync, UI scale, key bindings, day / night on or off.
- [ ] Pause menu (Esc when no tool is active).
- [ ] First-time hints: a short guided start (build a road, zone, watch the commute, read Flow).
- [ ] Scenarios as challenges with a goal (e.g. "get the Bottleneck above 70% Flow") and a result screen.

**Gameplay tuning**
- [ ] Time scale: a game day passes in under five minutes while a car crosses town in a game hour and a walk takes hours; rebalance the clock against travel times so trips feel right in game hours.
- [ ] Flow: make the number understandable and stable (what 80% means, how it moves day to day).
- [ ] Bus usefulness: tune waits, walks and service so buses matter in typical player cities (left for later on purpose: see 8c and "Bus reach").
- [ ] Difficulty curve of the scenarios; sensible defaults (new map start, first zones).
- [ ] Performance budget for large player cities (thousands of vehicles plus pedestrians) with the new visuals.

### Phase D — Dot display: a minimalist, cassette-futurist look (from 2026-10-26)
**Left behind in the migration (2026-11-11), to redo in Bevy:** the whole dot display (layers, composite shader, levels of detail, alerts) was Godot-specific and is not ported; the text below is the design to redo it from. It is in the `godot-final` tag.

The game is redrawn as an amber LED dot matrix, in the spirit of Mini Metro: only what matters (roads, what moves, lights, the transit network, problems) shows, the rest steps back. Chosen from mockups (phosphor glyphs, amber dot matrix, ANSI colour): the amber dot matrix, at 3-4 screen px per dot. A first try as one shader over the existing graphics showed its limits: it only sees colours, so a bus lane, a red car and a brake light can't be told apart, and the assets (textures, roof details, grass grain) turn into noise. So the look is rebuilt as a pipeline, and the assets are redrawn for it.

**Design**
- **Layers by meaning, one composite.** Every renderer draws into one of four display layers, each a `SubViewport` sharing the world's `World2D` and filtering by `CanvasItem.visibility_layer` (`Viewport.canvas_cull_mask`): *map* (ground, roads, buildings, nature: brightness, kept low), *actors* (vehicles, people), *light* (street lamps, windows, headlight beams, brake and indicator glows, lamp cores) and *information* (transit lines and stops, bus lanes, zones, signals, overlays, tool previews). One composite shader (`DotDisplay`) turns them into dots with fixed priorities: information over lights over actors over the map. Lighting is done there too (map and actors are drawn unlit; night is ambient × surface + light), so the light of one car never shows on another.
- **World-space dot grid, stepped zoom.** Dots are fixed to the map, not the screen, so panning and moving traffic don't shimmer and assets can be drawn dot by dot (a car is 11 × 6 dots at the finest level). The camera zooms in steps (0.25× to 4×); at each the grid uses the power-of-two world size (1, 2, 4, 8, 16 px per dot) that keeps dots 3-4 screen px apart. Zoomed out, cells merge (a car becomes 3 × 1 dots at 1×).
- **Meaning in dot shape, not only colour.** Physical things are solid round dots in 16 amber shades; plans and zones hollow rings; bus lanes square dots or dashed rows; alerts and indicators blink; colour only for light (white headlights, red brake lights, orange indicators, signal colours) and for transit lines. Readable without relying on colour alone.
- **Assets redrawn for the medium.** No textures or grain; flat shapes at the dot scale: cars as silhouettes with lamp cells, buildings as outlined blocks with a few window cells, roads as kerb lines and lane dashes, fewer, simpler trees. Renderers keep their logic (snapshots, incremental updates) and change what they draw.
- The old look stays behind a debug toggle while the phase runs, and is removed at its end.

**Steps**
- [x] **D1. Layer pipeline and composite shader.** `DotDisplay`: the four display layers, routing of every renderer, the composite with lighting and priorities, the world-space grid with LOD, stepped zoom in `MapCamera`. The existing art is drawn into the layers unchanged. `NightLights` moves into the world (light layer); lamps and light divide by the night tint in the old look. *Done (2026-10-26):* four `SubViewport`s share the world's World2D and keep their renderers by `visibility_layer` (`DotDisplay.ROUTES`; parents get every bit; children added later are routed as they enter); the main viewport then draws only the UI (`canvas_cull_mask` 1), and the layers stop when the display is off. The composite samples each cell nine times: information (alpha > 0.3) wins and keeps its colour; on an actor only its own lamps count as light (brighter than 0.85), so a beam never shows on the car in front; the map is dimmed (×0.75) and lit by ambient (0.4 at night) plus the light layer; actors keep more ambient at night (0.6). Lit lamps come from `vehicle_light.gdshader` (now also drawing the lamps as sources), on `VehicleLights`, a sibling just before `VehicleRenderer`. Zoom steps 0.25× to 4× (max raised from 3×), cells 16 to 1 world px. Seen so far: transit lines, stops and signals now stand out, but lines cover the buses on them (D2: offset ribbons); bus lanes are still painted in the map layer (D2); the existing art is too detailed for small cells (D3, D4).
- [x] **D2. Information layer.** Bus lanes, lines, stops, zones and signals in their own dot styles; nothing disappears into the amber. *Done (2026-10-26):* instead of encoding palette ids, the style is the layer: two more display layers, *square marks* and *ring marks*, so renderers keep drawing in plain colours and only put each element on the right node. Priorities: solid information (transit lines, signals, overlays, tool previews; round dots, over everything) > light > actors > square marks (bus lanes, stop signs) > ring marks (zones, hollow rings on every other cell) > map; square marks are as big and coloured as they are opaque (bus lanes, a faint tint: small muted squares leaning to amber; stop signs: full white squares); marks are under the actors, so buses in a bus lane and cars on a zoned street stay visible. Information is never lit below 0.75 at night. `DotDisplay.ROUTES` takes deeper paths (`RoadRenderer/BusLanes`, `TransitRenderer/Lines`, `TransitRenderer/Stops`) that override their parent's layer. Bus lanes are a `RoadMarkings` of their own (`bus_lanes_only`) on `RoadRenderer`'s BusLanes layer. `TransitRenderer` draws on three child layers (Ground: bays and zebras, in the map; Lines; Stops); lines are now ribbons beside the lanes on the curb side, side by side where lines share a road, each piece offset on its own with round joints (a mitred offset spiked where paths double back). Zones are drawn only on cells without a building, on their plot (`LotSystem.compute_plot`). Lines, signal bars and stop signs are at least one dot cell wide (`DotDisplay.current_cell`) and information shows in a cell once one of its nine samples is half covered, so the network stays readable zoomed out (at 0.5× the town reads like a transit map).
- [ ] **D-LOD. Levels of detail per zoom.** Zoomed out, crucial information is lost in the dots. Principle (Mini Metro): close up you see objects, far out you see the system (network, flows, problems). The nine zoom steps fall into four bands, and each band changes *what* is drawn, not just its size. This frames D3 and D4: they draw each element per band. Designed 2026-10-26:

  | Band | Zooms | Dot cell | A car is |
  |---|---|---|---|
  | **Detail** | 2×, 3×, 4× | 1-2 px | 5×3 to 11×6 dots |
  | **Street** | 1×, 1.5× | 2-4 px | ~3×2 dots |
  | **District** | 0.5×, 0.75× | 4-8 px | 1 dot |
  | **City** | 0.25×, 0.375× | 8-16 px | less than a dot |

  | | Detail | Street | District | City |
  |---|---|---|---|---|
  | **Vehicles** | Silhouettes per kind, lamps, indicators | Small blocks per kind; brake lights only, no indicators | One dot per vehicle (queues read as solid dot lines); buses as bright dots in their line's colour | Single dots, dimmed (the network and problems dominate); buses still shown |
  | **People** | Dots; riders queue at stops | Only waiting riders, as a short stack of dots by the stop | Hidden (stops show crowding) | Hidden |
  | **Lights** (night) | Everything: beams, street lamps, windows, brake lights, indicators | Lamps as single cells with small pools; one window cell per building; headlights one cell, no beams | No lamps or beams; one warm dot per lit building | Ambient only; lit districts glow faintly |
  | **Buildings** | Outlined blocks with window cells, two-tone roofs | Solid blocks, one amber shade per zone type | One block per lot, merged into urban fabric | Built areas as a sparse dot texture |
  | **Roads** | Kerbs, lane dashes, centre line, crosswalks | Band with kerb lines; no dashes, no crosswalks | One bright line, 1-3 dots wide by road class | Same, side streets dimmer |
  | **Bus lanes** | Muted squares across the lane | One row of squares along the curb | Dashed squares (every other cell) beside the road | Hidden (the lines say it) |
  | **Transit lines** | Ribbons 2-3 dots; stops as signs with riders | Ribbons 1 cell; white square stops | Ribbons kept ≥ 1 cell; buses moving on them | Same: the network is the map |
  | **Signals** | A bar per lane | One dot per approach | One ring per junction with lights (no state) | Hidden |
  | **Zones** | Rings on free plots | same | same | same |

  **Problems, at every zoom, always on top:** a new *alert* display layer, blinking, marks never smaller than 2 cells. Jams: lanes over a congestion threshold in red (pulsing at District and City, so they're found from far out). Gridlock and stuck vehicles: a blinking red ring on the spot. Stranded riders: the stop blinks. Zones waiting for a road or for room: blinking red (replacing the dashed outline under the dot display). An always-on, thresholded version of the F4 heatmap.

  **How:** `DotDisplay` publishes the current band (`current_band`, like `current_cell`) and `Events.dot_band_changed`; renderers decide what to draw per band. The zoom is stepped, so the band switches at once (a short cross-fade later if it feels abrupt). Night lighting is kept in every band. No sim change: jams come from the congestion `CongestionMonitor` already computes.

  **Steps:** (1) band switch and the alert layer; (2) vehicles, people and lights per band (D3); (3) map per band (D4); (4) bus lanes, signals and stops per band.

  *(1) done (2026-10-26):* `DotDisplay.current_band` / `band_for(zoom)` and `Events.dot_band_changed`. `AlertRenderer` (World, after the transit renderer): jams (congestion ≥ 1.5, i.e. 2.5× free-flow time) as red square marks under the queue at Detail and Street, as blinking red lines two cells wide over everything at District and City; vehicles stuck 30 s (`ViewSnapshot.stuck_positions`, new) as one blinking ring per group (grouped by squares of 3 ring radii: hundreds of rings in one canvas item stopped it rendering); stops with 8+ riders waiting as a blinking dot on the sign (instead of "stranded riders": the snapshot has waiting counts, not strand events); zones waiting as a blinking outline (the dashed one stays in the old look). A separate alert display layer was tried and dropped: a seventh texture in the composite was not reliably bound (D3D12; alerts vanished after a while), so alerts are information (the top layer anyway) and blink by themselves (`AlertRenderer` dims its node, 1.25 Hz, lit 60%, dimmed to 60%); the composite draws information as bright as it's drawn. **The composite must stay at six layer textures.**
- [ ] **D3. Vehicles and lights.** Vehicles, buses and people redrawn per LOD for the grid; lamps and beams as light cells.
- [ ] **D4. Map.** Roads (kerbs, lane dashes, crosswalks), buildings (outlined blocks, window cells), nature, simplified and quantised to the grid.
- [ ] **D5. Overlays and UI.** Heatmap, lane overlay, route inspector and tool previews in dot styles; HUD and palette restyled (monospace, zero-padded readouts, departure-board text); remove the old look; update the Town scenario screenshots.

### Phase 10+ — Toward the full game (order to be revisited)
- More road types (6-lane, bus lanes, truck-only roads), lane-turn rules UI.
- Demand quotas, level-up loop, Infrastructure Points, map expansion.
- Density auto-upgrade driven by connectivity + local flow.
- Elevation: bridges, highway ramps, subway layer with "Underground View".
- Cargo rail / harbor external connections, trains, trams.
- Save/load of full game state, undo/redo, art pass, audio.

---

## 5. Key Risks & Decisions

| Risk | Mitigation |
|---|---|
| **Intersection deadlocks / gridlock** that the player can't fix | Don't-block-the-box from Phase 2b; deadlock detector in debug (vehicle stopped > N s with stopped vehicle ahead in a cycle); patience despawn as the last-resort release valve. |
| **Performance** at thousands of vehicles | **Resolved by the port to Rust + Bevy (M4.8, R3).** In GDScript (~7 µs per vehicle per tick) 2 000 vehicles did not fit a frame, even with the sim on a worker thread (Phase 2b decision, 2026-10-20). The Rust sim costs 1.65 ms per tick at 2 000 vehicles against 26.5 ms in GDScript, the Town runs 14x faster, and the threaded runner holds 60 FPS with the Town plus 2 000 stress vehicles (main-thread update about 1.5 ms). Further optimisation (M4.9: dense vectors next to the `IndexMap`s) is optional and not needed yet. |
| **Network edits while vehicles are driving** | All edits go through `RoadNetwork` commands that emit dirty sets; affected vehicles reroute or despawn; edit is applied between ticks only. |
| **Curves arriving late breaks assumptions** | Lanes are polyline `Curve2D` from day 1; Phase 1 tests include a hand-made curved segment even before the curve tool exists. |
| **Agent count grows with pedestrians and buses** (Phases 7–8) | Performance gate at the start of Phase 7: measure, then decide between a separate cheap pedestrian system and optimising the hot loops (settled by the Rust port, see above). Pedestrians are lightweight agents; they only interact with cars at crosswalks. |
| **Fun is unproven** | Explicit playtest checkpoint at end of Phase 2c, before investing in zones/economy. |

### Defaults I've assumed (change any of these early if you disagree)
- **Rust** for everything: `tf_sim` (no engine) and `tf_game` (Bevy). (It was GDScript until the port, 2026-11.)
- **Right-hand traffic.**
- **64 px grid**, 2-lane local road as the only road type until Phase 5.
- **`cargo test`** for tests (unit tests next to the code, integration tests in `crates/*/tests`); `scripts/check.sh` is the gate.
- **Trip-based citizens** (lightweight records) rather than fully simulated people.
