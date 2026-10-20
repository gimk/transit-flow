# Transit Flow — Implementation Plan

Companion to [game_design_godot_architecture_concept.md](../game_design_godot_architecture_concept.md).
That document says *what* the game is. This one says *how we build it, in what order, and how we know each step is done*.

**Engine:** Godot 4.7 (Forward+), GDScript. **Status:** Phase 3 done (3a–3c). Next: playtest the living city, then Phase 4.

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
- Curved segments can't be split yet ("Can't join a curved road in the middle yet"). Phase 4 needs a proper Bézier split (de Casteljau).
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
- `Scenarios.build_bottleneck` (F7 in game, also used by tests): 28 commuters through one stop-sign T-junction; `build_bypass` adds a 3-road detour.
- Measured (tests): queue of 17 on the approach, approach congestion 4.0; with the bypass the morning finishes in 43.4 s instead of 65.7 s (−34%), 12 of 28 commuters re-route onto it.

**Done when:** a single bottleneck intersection visibly jams, adding a parallel road visibly relieves it, and rerouting is observable. ⭐ **First "is this fun?" checkpoint** — play it before going further.

**How to playtest:** run the game, press **F7** (Bottleneck scenario), **F4** (heatmap), **3** (4× speed). Watch the queue form at the lights, then try: build your own bypass (R), toggle the lights (L), zone more (H/C/I), F6 for extra random traffic. Questions to answer: is watching and fixing the jam satisfying? Is it readable? What's missing to make it a game?

*Notes carried forward:*
- Only vehicles that are held up re-route, at most 50 per 5 s; free-flowing vehicles keep their routes until the network changes.
- North-side homes on a road drive away from their destination to turn around (lane-side access, see Phase 1c notes). In the bottleneck scenario this sends half the commuters to the west dead end first; it reads oddly and is worth revisiting with Phase 3's zoning.
- Lights only know two axis phases; protected left-turn phases and per-junction tuning are later polish.
- Held-up vehicles are re-routed 3 per tick (50 per 5 s), so the route searches don't spike one frame.
- Perf after this round (benchmark): 500 → 4.6 ms/tick, 1k → 10 ms, 2k → 21 ms, 5k → 47 ms (more arrivals per second also means more route searches for new trips).

### Phase 3 — The living city ✅
- **3a Zoning.** `ZoneSystem`: paint R/C/I cells (`ZoneTool`: drag a rectangle; H/C/I, X dezone). A zoned cell next to a road develops into a building (`Lot`) after 2–10 s; cells out of road reach wait (dashed red outline) and re-check when roads change. Rezoning / dezoning / bulldozing removes the building. Zones are drawn with `_draw` rather than a `TileMapLayer` (simpler; revisit if large maps get slow). Vehicles got kinds (car 11×6, truck 17×7) with real lengths used everywhere.
- **3b City life.** `ExternalConnections`: highway points on the map edge (new maps start with one entering from the west, HWY sign). `CitizenSystem`: 2 residents per home, jobs at the nearest shop (2 jobs) or factory (3 jobs) with room, personal schedules (leave home 06:30–09:00, leave work 16:00–19:00, 35% shop 30–60 min on the way home). `FreightSystem`: trucks for raw materials and imports from the highway, goods factory → shop, exports, at a rate peaking 10:00–16:00. (The planned separate `TripScheduler` lives inside these two systems.)
- **3c Score.** Patience: trips are timed against their free-flow time (+2 s); past 2× + 15 s the driver turns red, past 4× + 60 s they give up (failed trip). `FlowMetric`: per trip `ideal / actual` capped at 1, failures 0, averaged over the last game day; HUD shows Flow % (green ≥ 80, amber ≥ 60, red), population and jobs. `DayNight` `CanvasModulate` tint (debug toggle).
- Measured: one day in a small zoned city gives cars/trucks started per period: morning 80/22, midday 0/84, evening 88/24, night 8/20 (flow 88%). Flow reacts to the network: cutting the bottleneck scenario's commute road mid-morning takes Flow from 88% to 30%.

**Done when:** one in-game day plays through all four pulse periods with visibly different traffic patterns, and Flow % reacts to network changes.

*Notes carried forward:*
- Buildings need a road on their cell edge, so blocks deeper than 2 cells leave unbuildable middles. Fine for now; Phase 5 density/lot sizes may change it.
- Citizens, vehicles, light states and the Flow history aren't saved; a loaded game restarts them (people at home).
- Only one highway; no new residents arrive from outside yet, and goods have no economy (trucks are traffic only). Demand quotas and growth are Phase 5.

### Phase 4 — Curves & buses
- Curved road drawing (control-point drag, minimum radius); everything else should "just work" thanks to the lane model — this phase validates the core architectural bet.
- Bus stops on lanes, `BusLineTool`, bus vehicles on fixed routes; citizens choose bus vs. car by comparing estimated trip times (mode choice).

**Done when:** a curved bypass and a bus line both measurably change Flow %.

### Phase 5+ — Toward the full game (order to be revisited after Phase 3 playtests)
- Road hierarchy (4- and 6-lane, bus lanes, truck-only roads), lane-turn rules UI.
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
| **GDScript performance** at thousands of vehicles | Measured in Phase 2b: ~7 µs/vehicle/tick, so 2k cars don't hold 60 FPS. Options, cheapest first: (a) lower the tick rate to 10 Hz (halves cost; rendering already interpolates); (b) run the sim tick on a worker thread with a snapshot for rendering (removes frame hitches, not total cost); (c) port `VehicleSystem` to **C#** (needs the Godot .NET build + .NET SDK; typically 5–20× faster for this kind of loop); (d) port it to a C++ **GDExtension** (fastest, heaviest toolchain). The `VehicleSystem` API boundary stays the same in all cases. |
| **Network edits while vehicles are driving** | All edits go through `RoadNetwork` commands that emit dirty sets; affected vehicles reroute or despawn; edit is applied between ticks only. |
| **Curves arriving late breaks assumptions** | Lanes are polyline `Curve2D` from day 1; Phase 1 tests include a hand-made curved segment even before the curve tool exists. |
| **Fun is unproven** | Explicit playtest checkpoint at end of Phase 2c, before investing in zones/economy. |

### Defaults I've assumed (change any of these early if you disagree)
- **GDScript** for everything, with a possible C# port of the sim core gated on the Phase 2b benchmark.
- **Right-hand traffic.**
- **64 px grid**, 2-lane local road as the only road type until Phase 5.
- **GUT** as the unit-test framework.
- **Trip-based citizens** (lightweight records) rather than fully simulated people.
