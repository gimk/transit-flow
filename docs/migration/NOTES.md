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
| `TransitSystem._on_vehicle_arrived` → `rider_arrived` (`transit_system.gd:581`) | pushed inside `on_vehicle_arrived`, delivered when it returns: inside the `VehicleArrived` delivery (depth first), but after the bus's `_board` | Nearly. In Godot each rider's listeners run before the next rider gets off and before `_board(bus)`. Here they run after. Citizens' handler (M7.1) can only matter if it makes someone wait at that stop for that bus's line, which it can't: a rider changing lines waits for another line, and `_board` skips other lines. The bus trace (rider events on the same tick, same door) matches. |
| `TransitSystem._strand` → `rider_stranded` (`transit_system.gd:635`): `step` (road changes, give-ups), `remove_stop`, `remove_line`, `on_vehicle_failed`, `plan_ride` (`_ensure_resolved`) | delivered after the call that stranded them (`transit.step`, the command, the `VehicleFailed` delivery) | Nearly: a command that strands several riders delivers all the events after it ends. Their listeners only start a citizen's day over, so no effect is expected. |
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

### PedestrianSystem (M5.2)

The system doesn't hold the roads, lots, lights, vehicles or crosswalks: the methods take a `WalkCtx` (`Simulation::walking()` splits the borrows; `crosswalks` is `&mut`, since pedestrians update it while reading `VehicleSystem::is_crosswalk_busy`). Pedestrians who arrive move from `pedestrians` to `finished` (read with `pedestrian_any`) until the next tick starts; `remove` takes the `Crosswalks` to release the pedestrian's claims. `owner_id` is an `i32` like `Vehicle.owner_id`. `Parking::find_space`'s walking-distance closure is `PedestrianSystem::get_walking_distance(&ctx, from, to)`, wired by M7.1.

### TransitSystem (M6.2)

The system doesn't hold the roads, router, vehicles, pedestrians or crosswalks: the methods take a `TransitCtx` (`Simulation::transit_ctx()` splits the borrows; `crosswalks` is `&mut` since `_update_crossings` sets the mid-block crosswalks, and the pedestrians' `WalkNetwork` is read for walking distances). Commands that can strand riders (`remove_stop`, `remove_line`, `plan_ride`, `step`) take the `Outbox`: `plan_ride` strands the riders of stops that lost their road, because `_ensure_resolved` runs first, so CitizenSystem (M7.1) must pass `out` through `plan_trip`.
- **Riders live in one map** (`riders`, by citizen id, in order); `BusStop.waiting` and `Bus.riders` list citizen ids where Godot lists the same `Rider` objects. A citizen can only wait or ride once (Godot would keep two `Rider` objects for a second `wait_for_bus` of the same citizen; it never happens).
- `plan_ride` returns `RidePlan { legs: Vec<RideLeg>, cost }` (`RideLeg::Walk` / `RideLeg::Bus`): M7.1 maps them to `Citizen.legs`. Its Dijkstra keeps Godot's `open` array scanning order and the stop order of `_stops_in_reach` / `_stops_near_stop` (an `IndexMap`, the stop itself overwritten in place), so ties resolve the same way.
- Line timings (`free_flow_times`, `segment_times`) are `f32`, like `PackedFloat32Array`; sums and the moving average are computed in `f64` and stored back as `f32`.
- Buses are `VehicleSystem` vehicles with `in_service`; `TransitSystem` reads them through `ctx.vehicles.vehicles[&id]`. "Is the vehicle's lane still the network's" (`_roads.lanes.get(vehicle.lane.id) != vehicle.lane`) is `!roads.lanes.contains_key(id)`.
- `LINE_COLORS` is `TransitSystem::line_color(line id)`; colours are `[f32; 4]` (`bus_line::Color`).
- `find_stop` takes its radius (Godot's default is `MERGE_DISTANCE`), `to_dict` / `restore` use `serde_json::Value`, like the other systems. `restore` needs the `TransitCtx` (it resolves stops on the roads and lays out the lines).

**Result: stops, lines, crosswalks and buses match Godot.** For every golden layout, the stops restored from the save have Godot's lane, distance, bay, position and direction; the lines have its path, free-flow times, routes (lane for lane) and colour; the mid-block crosswalks are the same, keys in order (`tests/transit_golden.rs`; `walk_golden.rs` still seeds its crosswalks from the golden files, so the walk network is checked independently). `migration/probes/bus_trace.gd` traces what the layouts can't: a street with bays and curbside stops under 40 cars with riders turning up (1258 events), and a grid with two lines under 80 cars through a street added, a road retyped, a stop removed and a block cut (563 events: failures, stranded riders, give-ups of waiting). Every vehicle arrival and failure, every bus state change (stop index, dwelling, in a bay), every rider getting off (door position to 0.1 px) or stranded lands on Godot's tick; buses, stops, line timings and vehicles end the same; and 200 `plan_ride` queries (76 with a plan, 11 changing lines) after a minute of running give the same cost (1e-6) and legs (`tests/bus_trace.rs`).

### CitizenSystem (M7.1)

`Citizen` is `city::citizen` (`State`, `Mode` with Godot's integer values; `Leg` is an enum: `Walk { from, to }`, `Car { from, to }` (lot ids), `Bus { line, from, to }`; ids are `Option`s where Godot has `-1`). `CitizenSystem` holds `citizens`, `parking`, its RNG (seed 11) and job bookkeeping; like the other systems it doesn't hold the rest: calls take a `CitizenCtx` (`Simulation::citizens_ctx()`: roads, router, lots, lights, vehicles, pedestrians, transit, crosswalks, clock; it hands out the `WalkCtx` / `TrafficCtx` / `TransitCtx` the other systems want, by reborrowing).
- Methods take a citizen **id**, not the object (the citizen lives in `citizens`, which the system itself borrows): `plan_trip(ctx, citizen, from, to, going_home, out)` (it needs `out` because `plan_ride` can strand riders), `pick_shop(lots, work)`; `plan_drive` is public too (a GUT test prints its cost). `_workplaces` / `_shops` hold a `Place` copy (id, type, centre) instead of `Lot` objects: lots never change type or place.
- `trip_ended` is `SimEvent::TripEnded { citizen, completed }`, delivered when the citizen call returns; the Flow listener reads the citizen's `trip_*` fields then. They don't change between the emit and the delivery (a citizen starts at most one trip per call), so it sees what Godot's synchronous handler sees. A citizen removed in the same call can't have a trip ending: `sync_with_lots` finishes no trip of a removed citizen.
- `VehicleSystem::depart` returns the id of a vehicle that may still be in its driveway; `vehicle_or_departure_mut` finds it to set `scored_by_owner` (Godot sets it on the object `depart` returns).
- **`sort_custom` is Godot's** (`geom::sort`, `SortArray`'s introsort with its insertion-sort tail): Godot's isn't stable, and `_pick_shop` and `Parking.find_space` sort lists where lots are equally far (a street of shops). The probe sorts 60 points with ties everywhere and the order is the same (`citizen_trace.rs`). `Parking::find_space` now uses it (M3.1 used a stable sort). The heap-sort fallback for a too-deep recursion is a stable sort (never reached at these sizes). The other `sort_custom` calls (walk network, road network) are still `sort_by`.
- Mode choice maths is Godot's: costs are `f64`, `randf()` draws are widened `f32`s (`leave_home_hour = 6.5 + (randf() + randf()) * 0.5 * 2.5`), `car_preference` is `randf_range(0.85, 1.2)` in `f32`. Draw order per new citizen: leave-home (2), leave-work (2), preference, car; per day-end decision: shopping chance, then `pick_shop`'s `randi`; arrival at a shop: `randf_range`.

**Result: the citizens are Godot's, tick for tick.** `migration/probes/citizen_trace.gd` → `golden/citizen_trace.json` is a day (6000 ticks, 06:00 to 06:00 the next day) in a small city: zoned homes, shops and factories on a grid, a highway with trucks, a bus line, shop parking of three spaces, a street cut and laid again at 16:00 and 18:30, a shop removed, everyone at home sent out at once at 22:40 (spread over 10 s). 80 citizens, 2083 events: every vehicle and pedestrian arrival (402, 145), every state change (801), trip start (364: mode, destination, Flow's reference time) and trip end (358: completed, mode, ideal time and duration to the millisecond), the riders getting off and stranded; and at the end every citizen's schedule, taste, car and job, the parking spaces, the vehicles, and 320 planned trips (mode, legs, cost to 1e-6). All the same (`tests/citizen_trace.rs`). `test_daily_pulse` prints the same cars and trucks per period (morning 61/23, midday 0/74, evening 59/24, night 6/13), and the mode-choice costs `test_buses` prints (22.8, 35.6, 34.0) are the same.

### FlowMetric (M7.2)

`FlowMetric` holds no systems: `Simulation::dispatch` passes it the vehicle (`on_vehicle_arrived(&Vehicle)` / `on_vehicle_failed`, found with `vehicle_any`, still readable while their events are delivered) or the citizen (`on_trip_ended(&Citizen, completed)`). Getters take `&mut self` (they forget old records first): `flow_percent(now)` (-1 when empty), `trip_count(now)`, `mode_shares(now)` (an `IndexMap<Mode, f64>` in the order the modes first appear, summing `1 / n` per record as the GDScript does). `FlowMetric::score(&Vehicle, arrived)` is the static `score`.
- Godot keeps the records in `Vector2`s, so the end time and score are **`f32`** there, and `_total` adds the `f64` score but subtracts the rounded `f32` when a record is forgotten. Ported as it is (`records: VecDeque<(f32, f32)>`); the drift is of the order 1e-8 per forgotten trip.
- The probe samples Flow every 400 ticks (15 samples over the day: value, trips in the window, shares per mode). The Rust values equal Godot's to the last digit printed (1e-6 of a percent), including the share of each mode (walk 26.5%, car 69.8%, bus 3.6% at the end). `test_daily_pulse` prints flow 71% and the Rust test pins it.

### Scenarios, saves, Town (M7.3)

- `debug::scenarios` returns `BottleneckInfo { junction, bypass }` and `TownInfo { center, lights, highways, bus_lines }` where Godot returns dictionaries; `add_bottleneck_bus_line` and `add_bottleneck_bus_lanes` return `Option<LineId>`. A bus stop that can't be placed is `StopId(-1)` in the list given to `add_line` (Godot appends -1 too), which `add_line` drops.
- `Simulation::to_dict` / `from_dict` live in `save.rs` (an `impl Simulation`); `from_dict` returns `None` for another `version`, builds the sim with `with_roads(RoadNetwork::from_dict(..))`, restores lots, zones, externals and transit in Godot's order, and delivers each restore's events through `sim.call`, so `LotsChanged` makes the citizens move into the homes (as `_sync_with_lots` does in Godot's `lots.restore`). **Compared with Godot's saves** (the six golden layouts): loaded and saved again they come out equal (numbers to 1e-3, as every golden check does: `serde_json` writes `f32`s as the nearest `f64`, so exact text equality isn't expected). Each scenario built in Rust also saves exactly Godot's save of it (`tests/scenarios.rs`).
- `CitizenSystem::set_seed` is `citizens._rng.seed = seed` (the Bottleneck tests and the baseline reseed the draws before building).
- The safety checks of `traffic_checks.gd` (queue gaps, conflict zones) and the crosswalk rule of `test_walking.gd` moved from the tests' helpers into `debug::checks`, so `bin/run_baseline.rs` can use them; `tests/common/traffic.rs` re-exports them.
- `TripEnded` is delivered after the citizen call, so a listener reading `citizen.state` sees the state after `_arrive`. The Town's commute statistic (Godot: `state == TO_WORK` at the emit) therefore tests `state == AtWork` on a completed trip, which is the same set: a completed trip to work always ends at work (`_arrive`), and nothing moves the citizen on within the tick. The count (175) and the median agree with Godot.
- **Result: the Town is Godot's, tick for tick.** `migration/probes/town_trace.gd` → `golden/town_trace.json` (5760 ticks): 2577 events, Flow every 240 ticks (to 1e-6 %), vehicle / pedestrian / departure / rider / stuck counts every 120 ticks, the stats of `baseline.md`, and the end state of 176 citizens and 165 vehicles all equal Godot's (`tests/town.rs`). The crosswalk rule holds on every tick too (a Rust-only check: Godot's Town test doesn't run it). Every number in `migration/baseline.md` is reproduced exactly by `bin/run_baseline.rs`, which also prints Godot's value and the band and exits 1 outside it.
- The probe's vehicle events carry `Vehicle.Kind` and the destination lot (not in `citizen_trace`'s format); `tests/town.rs` re-implements the recorder instead of sharing `citizen_trace.rs`'s, since the two formats differ.

## Tests waiting for a later work package

None: every ignored stub is ported. (M7.3 ported the eight: zones, freight and commute saves; Flow after cutting the road; the curved bypass; the bus save test, the bus line and the bus lanes on the Bottleneck.) Godot's printed numbers are asserted in `flow.rs`, `curves.rs`, `buses.rs` and `bus_lanes.rs`.
