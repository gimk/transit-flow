# Transit Flow — Implementation Plan

Companion to [game_design_godot_architecture_concept.md](../game_design_godot_architecture_concept.md).
That document says *what* the game is. This one says *how we build it, in what order, and how we know each step is done*.

**Engine:** Godot 4.7 (Forward+), GDScript. **Status:** Phase 1a done. Next: Phase 1b.

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

### Phase 1b — Lane graph + routing
- Connector lanes at nodes (straight / left / right; no U-turns).
- `LaneGraph` (AStar2D subclass) with travel-time costs; `Router` returning lane-id lists.
- Debug: click two points on roads → route is drawn.

**Done when:** routes are correct on a hand-built test map (including "longer but faster" choices once a second road type exists), with unit tests on a few fixed layouts.

### Phase 1c — A single car
- Green (Residential) and Yellow (Industrial) placeholder lots with access points.
- One vehicle: route home → work, follow lanes via `sample_baked_with_rotation`, wait, drive home. Interpolated rendering.

**Done when:** the car loops home↔work forever, turns smoothly through connectors, and reroutes correctly if a road on its path is bulldozed (or despawns cleanly if no route exists).

### Phase 2a — Crossings & splitting
- Drawing a road across an existing one (same layer) splits both and creates a 4-way node. T-junctions when ending on a segment.

**Done when:** arbitrary grid layouts produce a valid graph (tested by fuzzing random road placements and asserting graph invariants).

### Phase 2b — Many vehicles & queuing
- Struct-of-arrays `VehicleSystem`, car-following across lane boundaries, `MultiMeshInstance2D` renderer.
- `Uncontrolled` intersections with conflict table + don't-block-the-box.
- Stress spawner: N cars with random lot-to-lot trips.

**Done when:** 500 cars run on a grid without overlapping, without permanent deadlock, at 60 FPS. Record a perf baseline at 1k / 2k / 5k cars → **this decides whether the sim core stays in GDScript** (see §5).

### Phase 2c — Signals & the first jam
- `StopSign` and `TrafficLight` controllers; `SignalTool` to toggle them per node.
- Congestion feedback into routing costs.
- Occupancy heatmap overlay.

**Done when:** a single bottleneck intersection visibly jams, adding a parallel road visibly relieves it, and rerouting is observable. ⭐ **First "is this fun?" checkpoint** — play it before going further.

### Phase 3 — The living city
- `ZoneTool` + `TileMapLayer` zone painting for R/C/I; lots, access points.
- External connection on the map edge (highway entry).
- Citizens (home/work), `TripScheduler` with time-of-day curves, trucks External → Industrial → Commercial.
- Patience, failed trips, `FlowMetric`, HUD showing clock + Flow %.
- Day/night tint (`CanvasModulate`).

**Done when:** one in-game day plays through all four pulse periods with visibly different traffic patterns, and Flow % reacts to network changes.

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
| **GDScript performance** at thousands of vehicles | Sim isolated in `RefCounted` classes + struct-of-arrays + route cache + fixed 20 Hz tick. Measure at Phase 2b. If 2k cars can't hold 60 FPS, port `VehicleSystem` (and maybe `Router`) to C# or a GDExtension — the API boundary stays the same. |
| **Network edits while vehicles are driving** | All edits go through `RoadNetwork` commands that emit dirty sets; affected vehicles reroute or despawn; edit is applied between ticks only. |
| **Curves arriving late breaks assumptions** | Lanes are polyline `Curve2D` from day 1; Phase 1 tests include a hand-made curved segment even before the curve tool exists. |
| **Fun is unproven** | Explicit playtest checkpoint at end of Phase 2c, before investing in zones/economy. |

### Defaults I've assumed (change any of these early if you disagree)
- **GDScript** for everything, with a possible C# port of the sim core gated on the Phase 2b benchmark.
- **Right-hand traffic.**
- **64 px grid**, 2-lane local road as the only road type until Phase 5.
- **GUT** as the unit-test framework.
- **Trip-based citizens** (lightweight records) rather than fully simulated people.
