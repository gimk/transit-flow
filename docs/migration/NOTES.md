# Migration notes

The log the migration plan (`docs/BEVY_MIGRATION_PLAN.md`) asks for: decisions taken while porting, deviations from the GDScript, GDScript bugs found and left as they are, and what was left behind. Newest entries at the end of each section.

## Decisions

### Event dispatch (M0.2)

Godot signals run their listeners synchronously, inside the `emit()` call, in connection order. In Rust a system pushes a `SimEvent` into the `Outbox` it was given, and `Simulation::call` delivers the outbox as soon as that call returns. Each listener runs through `Simulation::call` too, so its own events are delivered before the next listener runs. That's depth first, as in Godot (for example: `vehicle_arrived` → transit → `rider_arrived` → citizens, all before citizens hear `vehicle_arrived`). The views get every event last, through `Simulation::take_view_events`.

Listener order is `Simulation._init`'s construction order. Read from the code (the table in the plan's §2.4 had `LotsChanged` slightly wrong; this is the corrected order):

| Signal | Listeners, in order |
|---|---|
| `RoadNetwork.changed` | router, lots, zones, signals, vehicles, pedestrians (its `WalkNetwork`, created in `PedestrianSystem._init`), transit, stress |
| `LotSystem.changed` | vehicles (`_request_reroute`), citizens (`_sync_with_lots`), freight |
| `VehicleSystem.vehicle_arrived` / `vehicle_failed` | transit, citizens, stress, flow |
| `PedestrianSystem.pedestrian_arrived` | citizens |
| `TransitSystem.rider_arrived` / `rider_stranded` | citizens |
| `CitizenSystem.trip_ended` | flow |
| `ZoneSystem.changed`, `ExternalConnections.changed`, `SimClock.hour_changed` / `day_started` | views only |

Where each emit site's events are delivered, and whether that differs from Godot:

| Emit site (GDScript) | Rust | Same as Godot? |
|---|---|---|
| `SimClock.advance` | delivered after `clock.advance` | Yes (views only) |
| `RoadNetwork` commands (`road_network.gd:118, 134, 145, 163`) | the command takes an `Outbox` and pushes `RoadsChanged` where Godot emits; the game and scenarios run commands through `Simulation::call(|sim, out| sim.roads.add_road(.., out))`, which delivers it | Yes |
| `LotSystem.add_lot` / `remove_lot` / `restore` (`lot_system.gd:62, 131, 190`) | delivered after the calling command, or after `zones.step` when zones develop lots | Nearly. `zones.step` can add several lots in one tick, each emitting `changed`, and in Godot citizens sync after each one. Here all of them are delivered after `zones.step`: the first sync sees every new lot and the later ones find nothing new. New lots are processed in the same order, so the same citizens are created with the same RNG draws. Vehicles only set a flag, and freight rebuilds its lists. |
| `LotSystem._on_roads_changed` (`lot_system.gd:252`) | inside the `RoadsChanged` delivery (depth first) | Yes |
| `ZoneSystem` (`zone_system.gd:54, 65, 99, 137`) | after the call | Yes (views only) |
| `VehicleSystem._reroute_all` → `vehicle_failed` (`vehicle_system.gd:1516`), at the start of `step` | `step` is split: `step_begin` (`_time += dt`, re-route), deliver, then `step_rest` | Yes |
| `VehicleSystem.step` arrivals and give-ups (`vehicle_system.gd:396`, `_fail` at 400) | pushed in Godot's order (all arrivals, then the give-ups) and delivered after `step_rest` | Nearly. In Godot an arrival's listeners run before the give-up loop runs `_fail` on the vehicles that gave up. Here the give-ups have already been detached when the arrival listeners run. Listeners never touch other vehicles' give-up state, so no effect is expected. |
| `VehicleSystem.reroute_delayed_vehicle` → `_fail` (called by `CongestionMonitor.step`) | delivered after `congestion.step` | Nearly. Congestion re-routes up to 3 vehicles per tick, and a failure's listeners run after all of them instead of between them. |
| `PedestrianSystem.step` (`pedestrian_system.gd:111`), at the end of its step | delivered after `pedestrians.step` | Yes |
| `TransitSystem._on_vehicle_arrived` → `rider_arrived` (`transit_system.gd:581`) | inside the `VehicleArrived` delivery (depth first) | Yes |
| `TransitSystem._give_up_waiting` → `rider_stranded` (`transit_system.gd:635`), last in `step` | delivered after `transit.step` | Yes |
| `CitizenSystem._finish_trip` → `trip_ended` (`citizen_system.gd:309`) | delivered after whatever citizen call emitted it | Yes (flow only records it) |

Vehicles and pedestrians that leave the sim are removed from their map before the signal in Godot, but listeners still read them through the signal argument. In Rust the event carries the id, and the system keeps those objects in a "finished" list until the end of the tick (`clear_finished`, called last in `Simulation::step`). Listeners look them up there. M4.2 and M5.2 implement this. (Road and lot edits only set `_reroute_pending`, so failures happen inside `step`.)

### RNG: bit-exact with Godot (M0.2)

`rng::SimRng` reproduces Godot 4.7's `RandomNumberGenerator` exactly: PCG32 with Godot's seeding (`pcg32_srandom_r` with `PCG_DEFAULT_INC_64`), `randi()`, and Godot's two-draw `randf()`. It's checked against values Godot printed for 5 seeds. A system seeded like its GDScript original therefore draws the same numbers, which the plan didn't expect. `rand_pcg` and `rand_core` were dropped from the workspace dependencies.

### Number types (M0.2)

GDScript `float` is 64-bit, while `Vector2` and `Curve2D` distances are 32-bit. The Rust port keeps the same split: `f64` wherever the GDScript has a `float` variable (speeds, distances along lanes held in vehicles, times), `glam::Vec2` (`f32`) wherever it has a `Vector2`, and `f32` for everything a `Curve2D` returns or takes (`Polyline`). Convert where the GDScript mixes them, as Godot does (`Vector2 * float` computes in `f32`). Together with the exact RNG, this keeps runs close to Godot's for as long as possible, which helps when debugging a divergence.

### Geometry (M0.2)

`geom::segment` ports `Geometry2D.get_closest_point_to_segment`, `segment_intersects_segment` (Godot's algorithm, including `CMP_EPSILON` and the parallel-segment rule) and `get_closest_points_between_segments`. They're checked against 48 cases printed by Godot (`geom/godot_geometry2d.json`).

**Resolved in M1.2** (`geom::Curve2D` ports the baking; all golden curves match). The original question: Godot bakes every `Curve2D` into points `bake_interval` (5 px) apart before sampling, even straight polylines, so corners get cut and the baked length is a little shorter than the polyline's. Connectors are cubic Béziers. `sample_baked_with_rotation` also interpolates headings between baked points rather than taking the segment's heading. M0.3's golden files must include, for each lane, its control points, handles, `get_baked_points()`, `get_baked_length()`, and `sample_baked_with_rotation` at a few offsets. M1.2 then ports Godot's baking (`Curve2D::_bake` in Godot's `scene/resources/curve.cpp`, MIT licensed) as `geom::bake` until they match. `Lane`'s straight-lane fast path (exact math) doesn't depend on it.

## Deviations

None yet.

## GDScript bugs found (ported as they are)

- `test_road_types.gd` `test_vehicles_on_a_road_being_upgraded_carry_on` passes in Godot without testing what it says. The car's trip ends at a plain lane position, and upgrading the road in place gives its lanes new ids, so the destination lane is gone: the car fails at once (both builds, tick 0). The test passes anyway because it only checks `not vehicle.gave_up`, and a failure for lack of a route isn't a give-up. Ported as written. Real trips go to lots, whose access points are re-found, so they aren't affected; debug and stress trips to a lane position are.

## Left behind

By the user's decision (plan §1): generated art and visual effects, the dot display, and the UI beyond basic commands. The files are listed under M9 in the plan.

### Removed lanes are retired, not dropped (M1.3)

Godot vehicles hold `Lane` objects and keep driving a lane the network has dropped: a replaced connector is finished, then the vehicle continues from the lane it fed, and re-snapping reads the old lane's curve. Rust vehicles hold ids, so `RoadNetwork` moves removed lanes to `retired_lanes` (with their `outgoing_lane_ids`, which Godot also keeps) instead of deleting them. `lane_any(id)` finds a lane live or retired. `lanes` holds live lanes only, so `lanes.contains_key` means what Godot's `lanes.has` means. M4.2 must call `prune_retired_lanes` with "is any vehicle on it" each tick, or retired lanes pile up.

### Road types are `&'static` (M1.1)

`RoadType::list_all()` builds the registry once from the embedded `.ron` files. Segments hold `&'static RoadType` and compare them by identity (`std::ptr::eq`), as Godot compares `Resource` objects. Tests that need a type outside the data files leak one (`tests/common/roads.rs`, `custom`).

### `randi_range` (M1.3)

Ported for the road fuzz test: Godot's `RandomPCG::random(from, to)`, PCG32's unbiased bounded draw. It matches Godot's output for 5 seeds.

### A* is Godot's, ties included (M2.1)

`LaneGraph` extends Godot's `AStar2D`, and on grids many routes are equally quick, so which one Godot picks matters for parity. The Rust `LaneGraph` ports Godot 4.7's `AStar2D::_solve`: `f32` scores (`real_t`), its binary heap (`SortArray` `push_heap` / `adjust_heap` / `pop_heap`), ties broken toward the larger g, the open-list update by `find`, and neighbours tried in connection order. A probe showed Godot 4.7 keeps neighbours in insertion order (an `AHashMap`; removal swaps with the last, which the lane graph never does). Every golden route matches lane for lane, including 80 on a plain street grid (`golden/grid.json`); flipping the tie rule makes a grid route fail.

### Router API (M2.1)

The router doesn't hold the network: every query takes `&RoadNetwork`. Congestion costs live on `Router::congestion` (one map for both graphs; GDScript copied `graph.congestion` into the bus graph at each query, which amounts to the same thing). `CongestionMonitor` (M4.6) replaces it every 5 s. Access points are `LanePoint { lane_id, distance, penalty }` (the GDScript dictionaries; `penalty` 0 when absent).

### Lots and parking (M3.1)

`Grid` (`core/grid.gd`) moved into `tf_sim::grid`: the sim uses it. `LotSystem` and `Parking` take the road network (and lots) as parameters. `Parking::find_space` takes the walking distance as a closure (`PedestrianSystem.get_walking_distance`, M5.2). Access points are `LanePoint`s. `Lot.Type` is `LotType`, with Godot's integer values (saved).

### Vehicles, lights and crosswalks (M4.1)

`Vehicle` holds lane ids (`lane: Option<LaneId>`, `next_lanes`, `granted_connector` …) where Godot holds `Lane` objects; look them up with `RoadNetwork::lane_any`, which also finds retired lanes. GDScript `-1` and `INF` sentinels became `Option`s and `f64::INFINITY`. `VehicleSystem::vehicles` exists already (empty until M4.2) so `TrafficLights` can read the front vehicle of each queue. `Crosswalks` (planned for M5.1) was ported here because the lights read waiting pedestrians; its mid-block entries are a `MidBlockCrossing` struct.

### VehicleSystem (M4.2–M4.6)

The whole of `vehicle_system.gd` was ported at once (its four planned slices call into each other every tick), split into modules by GDScript section: `mod.rs` (state, public API, the tick, movement, bookkeeping, re-routing), `junctions.rs`, `lane_changes.rs`, `driveways.rs`, `buses.rs`, `crosswalks.rs`. Then each slice's GUT tests were ported. Layout decisions:
- **Lane queues live in `VehicleSystem`** (`occupied`: the non-empty queues, by lane id, in the order lanes became occupied, which is Godot's `_occupied_lanes` order), not on `Lane`. The road network stays read-only during a tick, and `TrafficLights` reads queues through `VehicleSystem::queue`.
- Systems the GDScript class held are passed as a `TrafficCtx` (roads, router, lots, lights, crosswalks), built by `Simulation::traffic()`.
- Godot's `outgoing_lanes` (objects, emptied when a lane is removed) is `live_outgoing` (the ids, or none for a retired lane).
- Vehicles waiting in driveways live in `departures` (owned), as in Godot, outside `vehicles`.
- Finished vehicles (and pedestrians) now stay readable until the **start of the next tick** (`Simulation::step` clears them first): Godot keeps their objects for whoever holds them, and tests read them after the tick.
- `StressTraffic` and `CongestionMonitor` (M4.6) were ported along with it: the stress tests need both.

**Result: the Rust traffic is Godot's, tick for tick.** All five stress grids of `migration/baseline.md` give exactly Godot's numbers (arrivals 507 and 622 on the local grids, 491, 283 and 419 on the mixed ones; same vehicle counts, gaps, 0 stuck), and the benchmark gives the same arrivals per second at every size. R2 pinned the five grids in the tests and added `traffic_trace` (road edits under traffic, a day of freight), which also matches tick for tick.

### WalkNetwork and the shared A* (M5.1)

`WalkNetwork` extends `AStar2D` too, so the Godot search was moved from `LaneGraph` into `routing::astar::AStar` (dense point indices, neighbours in connection order, caller-supplied step cost and estimate as `f32`); `LaneGraph` and `WalkNetwork` both use it. Walk point ids are the GDScript's (1, 2, ... in build order), so point `id` is index `id - 1`. `WalkNetwork` doesn't hold the roads or the crosswalks: `find_path` / `distance` / `ensure_built` take them, and `PedestrianSystem` forwards `RoadsChanged` to `on_roads_changed`. `find_path` returns `Option<WalkPath>` (Godot: `{}`). Both `sort_custom` calls in `_build` (crossings by distance, arms by angle) are `sort_by`, which is stable where Godot's isn't; they only differ on exact ties (two roads leaving a node at the same angle, two crosswalks at the same distance), which the golden layouts don't have.

## Tests waiting for a later work package

Ported as `#[ignore]` stubs with the work package that brings what they need. That WP ports them fully and removes the `ignore`:

| GUT test | Rust stub | Needs |
|---|---|---|
| `test_lots.gd` `test_an_avenue_too_close_removes_the_lots_between` | `lots.rs` | M7.1 (zones and citizens) |
| `test_zones.gd` `test_save_round_trip_keeps_zones_and_their_buildings` | `zones.rs` | M7.3 (Simulation saves) |
| `test_freight.gd` `test_saves_external_connections` | `freight.rs` | M7.3 (Simulation saves) |
| `test_curves.gd` `test_a_curved_bypass_works_like_a_straight_one` | `curves.rs` | M7.3 |
