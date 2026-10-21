# Transit Flow — Implementation Plan

Companion to [game_design_godot_architecture_concept.md](../game_design_godot_architecture_concept.md).
That document says *what* the game is. This one says *how we build it, in what order, and how we know each step is done*.

**Engine:** Godot 4.7 (Forward+), GDScript. **Status:** Phase 3 done (3a–3c), plus the Town scenario, two-sided lot access and junction realism round 1. Phase 4 (tool palette) done. Phases 5, 6a (driveways) and 7 (pedestrians, crosswalks, walking trips) done. Phase 8 (8a trips as legs, car availability, parking, walk-or-drive choice; 8b buses and the bus in mode choice; 8c bus bays and bus lanes) and UI pass 1 done. Next: Phase 9 (curves), or more UI / transit work (bus count per line, line editing).

---

## 1. Guiding Principles

1. **Simulation is separate from presentation.** All traffic, road and zone logic lives in plain `RefCounted` classes with no `Node` dependencies. Scenes only *observe* the sim and draw it. This gives us headless unit tests, deterministic replays, and a clean path to move hot code to C#/GDExtension later.
2. **Fixed-tick simulation.** The sim advances in fixed steps (start at 20 Hz) driven by an accumulator in the `Game` autoload's `_process`; sim code never sees frame `delta`. Rendering interpolates between ticks. Speed controls (pause / 1× / 2× / 4×) just change how many ticks run per frame.
3. **"Everything is a lane."** The concept's "a grid road is a 2-point curve" idea, pushed one step further: *both* road lanes *and* the turning paths inside intersections are `Lane` objects holding a `Curve2D`. A vehicle's route is just a list of lanes. Curves, ramps, roundabouts and bus lanes all reuse the same movement code.
4. **Debug views from day one.** Lane IDs, route lines, per-lane occupancy, and trip times are visible via toggleable overlays. A traffic game is impossible to tune blind.
5. **Data is serializable from day one.** The road network and zones serialize to a `Dictionary` → JSON. Saving/loading test layouts is also how we build repeatable test scenarios.

---

## 2. Architecture

### 2.1 Layers

```
┌──────────────────────────────────────────────────────────────┐
│ UI (HUD, toolbar, demand bar, flow %, clock, speed controls) │
├──────────────────────────────────────────────────────────────┤
│ Tools  (RoadTool, ZoneTool, BulldozeTool, SignalTool …)      │  ← mouse input, grid snapping, previews
├──────────────────────────────────────────────────────────────┤
│ View   (RoadRenderer, VehicleRenderer[MultiMesh], ZoneLayer, │  ← reads sim state, never writes it
│         Overlays)                                            │
├──────────────────────────────────────────────────────────────┤
│ Sim    RoadNetwork → LaneGraph → Router                      │  ← pure data + logic, unit-testable
│        VehicleSystem, IntersectionControllers                │
│        ZoneSystem, DemandSystem, TripScheduler, FlowMetric   │
├──────────────────────────────────────────────────────────────┤
│ Core   Game (autoload: runs sim ticks), Events bus (autoload)│
└──────────────────────────────────────────────────────────────┘
```

Tools mutate the sim only through `RoadNetwork` / `ZoneSystem` commands (`add_road`, `remove_segment`, `paint_zone` …). This is also the natural hook for undo/redo later.

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

```
res://
  core/            game.gd, events.gd (autoloads), grid.gd
  sim/
    roads/         road_network.gd, road_node.gd, road_segment.gd, lane.gd, road_type.gd, lane_builder.gd
    routing/       lane_graph.gd (AStar2D subclass), router.gd (route cache)
    traffic/       vehicle_system.gd, intersection_controllers/*.gd
    city/          zone_system.gd, demand_system.gd, trip_scheduler.gd, flow_metric.gd
    sim_clock.gd   (in-game time of day, advanced by ticks)
    simulation.gd  (owns all systems, step(), save/load)
  view/            road_renderer.gd, vehicle_renderer.gd, zone_layer.gd, overlays/*.gd
  tools/           tool_manager.gd, road_tool.gd, zone_tool.gd, bulldoze_tool.gd
  ui/              hud.tscn, toolbar.tscn, …
  data/road_types/ local_2lane.tres, avenue_4lane.tres, …
  scenes/          main.tscn, sandbox/*.tscn
  tests/           unit tests (GUT) + saved scenario JSONs
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

  Bus lanes meet the "done when": Flow +4, and even drivers get there sooner (buses no longer stop in, or merge into, the jammed lane). Bays don't help on a jammed corridor (what a bus saves at the stop it loses waiting to pull out); they're for buses on free-flowing roads. Drivers still don't switch to the bus (door to door it takes about twice as long, mostly walking and waiting), which is realistic for a suburb; denser cities, scarce parking or faster service (a later bus-count setting per line) would change that. "Bottleneck + bus lanes" is in the debug menu.

### Phase 9 — Curves (was Phase 4)
- Curved road drawing (control-point drag, minimum radius). Everything else should "just work" thanks to the lane model, so this phase still validates the core architectural bet. It comes after the simulation phases because simulation comes before mechanics; it can move earlier at any time, since nothing above depends on it.

**Done when:** a curved bypass changes Flow like a straight one would, with smooth driving through the curves.

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
| **GDScript performance** at thousands of vehicles | Done (2026-10-20): option (b), the sim steps on a worker thread a couple of ticks ahead of the screen, which draws interpolated `ViewSnapshot`s: frames stay at 16.7 ms even at 2000 vehicles (before: one frame in three over 20 ms at 1000, 230 ms frames at 2000 / 4×), and when the sim can't keep up it runs slower (HUD shows the real speed) instead of stalling frames. Vsync is now "enabled" (it was "adaptive", which tears when frames run late). Ticks run ~40% slower on the worker than headless on this laptop CPU (likely power sharing with the render thread). Measured in Phase 2b: ~7 µs/vehicle/tick, so 2k cars don't hold 60 FPS. Options, cheapest first: (a) lower the tick rate to 10 Hz (halves cost; rendering already interpolates); (b) run the sim tick on a worker thread with a snapshot for rendering (removes frame hitches, not total cost); (c) port `VehicleSystem` to **C#** (needs the Godot .NET build + .NET SDK; typically 5–20× faster for this kind of loop); (d) port it to a C++ **GDExtension** (fastest, heaviest toolchain). The `VehicleSystem` API boundary stays the same in all cases. |
| **Network edits while vehicles are driving** | All edits go through `RoadNetwork` commands that emit dirty sets; affected vehicles reroute or despawn; edit is applied between ticks only. |
| **Curves arriving late breaks assumptions** | Lanes are polyline `Curve2D` from day 1; Phase 1 tests include a hand-made curved segment even before the curve tool exists. |
| **Agent count grows with pedestrians and buses** (Phases 7–8) | Performance gate at the start of Phase 7: measure, then decide between a separate cheap pedestrian system and porting the hot loops (C#/GDExtension, see above). Pedestrians are lightweight agents; they only interact with cars at crosswalks. |
| **Fun is unproven** | Explicit playtest checkpoint at end of Phase 2c, before investing in zones/economy. |

### Defaults I've assumed (change any of these early if you disagree)
- **GDScript** for everything, with a possible C# port of the sim core gated on the Phase 2b benchmark.
- **Right-hand traffic.**
- **64 px grid**, 2-lane local road as the only road type until Phase 5.
- **GUT** as the unit-test framework.
- **Trip-based citizens** (lightweight records) rather than fully simulated people.
