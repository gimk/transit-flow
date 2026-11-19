# W6b report: draw the riders and the bicycle lights

Branch `p6-views2`, worktree `../tf-p6-views2`. Only `tf_game` changed. `tf_sim` is read only.

## What is drawn

### 1. The riders

A rider is an agent with `Mode::Bicycle`. It is drawn in the same batched mesh as the cars
(`views/agents.rs`), so it costs no extra entity.

- **Shape.** A body of 1.8 m x 0.6 m, the size the run gives the agent (`AgentPose::length`
  and `width`). The body is at the side position of the run: `AgentPose::lateral` is already
  in `pos`, so the rider sits in its own lane and not in the middle of the road.
- **Motion.** A rider interpolates like a car: the position and the angle come from the two
  snapshots around the render clock.
- **Colour.** One violet family, one shade for each archetype: careful 0.58/0.18/0.92,
  velib tourist 0.78/0.42/1.00, bike commuter 0.50/0.05/0.85, delivery rider 0.68/0.25/0.95,
  cargo bike 0.80/0.60/0.98, careless cyclist 0.92/0.08/0.72. No rider colour is a car
  colour or a walker colour (a test checks this).
- **The rider on the bicycle.** A dark dot (`RIDER_COLOR`, radius 0.22 m, 10 sides) 0.15 m
  forward of the middle of the body.
- **Heading.** A short light front mark, 0.3 m long and 70 % of the body width, at the nose.
- **State.** A waiting rider is darker (x0.6), like a waiting car. A crashed rider is
  magenta, like a crashed car.
- **Count.** `draw_agents` returns `Drawn { agents, riders }` and `AgentViews::riders` keeps
  the number of riders of the last frame.

### 2. The bicycle light in a run

- `StreetSnapshot` gives the light of the bicycle lane of each approach:
  `ApproachLight::bicycle`, filled from `SignalPlan::bicycle_light()` in `street_runner.rs`.
- `play/signals.rs` has a new `Lit { car, bicycle }`. `build_marks` takes
  `&[(ApproachMark, Option<Lit>)]` and `bike_color` gives the colour of the small light.
- In a run the small light shows the light of the bicycle lane. Thus the leading green shows:
  the small light is green while the car light of the same approach is still red.
- While designing there is no run to read. The small light then keeps the fixed
  `BIKE_LIGHT_COLOR`, which says that a lead is set.

### 3. The report and the HUD

- The run HUD line is now `On the street: N vehicles (R riders), M on foot`.
- The report has a new `Riders:` block, in the same shape as the walkers' block. It reads
  `CyclistSummary`: came, arrived, gave up, unfinished, mean delay, near misses (with cars,
  with walkers, with riders), close passes, hard brakes, and contacts (light injuries,
  serious injuries, deaths).
- The block shows only when `cyclists.spawned > 0`, so a level without bicycle flows has no
  empty line.

### 4. The agent card

- A rider's card starts with `Rider #id` (a car keeps `Vehicle #id`).
- It adds `Rides X m from the right edge, Y m off the lane middle` and
  `Rule draws: red / sidewalk / filter` from `BikeInfo`.
- Its state is `riding` (a car keeps `driving`).

## Files

- `crates/tf_game/src/street_runner.rs`: `AgentPose::lateral` (a copy of `Agent::lateral`);
  `ApproachLight::bicycle`; `CarInfo::bike` with `BikeInfo { edge_m, red_u, sidewalk_u,
  filter_u }`.
- `crates/tf_game/src/views/agents.rs`: the rider shape, `rider_color`, `RIDER_COLOR`,
  `Drawn`, `AgentViews::riders`, 2 new unit tests.
- `crates/tf_game/src/views/mod.rs`: the module note names the riders.
- `crates/tf_game/src/play/signals.rs`: `Lit`, `build_marks`, `bike_color`, 1 new unit test.
- `crates/tf_game/src/play/inspect.rs`: the rider card and the `riding` state, 1 new unit test.
- `crates/tf_game/src/play/ui.rs`: the HUD line, `cyclists_text`, the `Riders:` block, 1 new
  unit test and 1 changed test.
- `crates/tf_game/tests/app/riders.rs` (new, 6 tests) and `tests/app/main.rs` (the module).

## Tests

`crates/tf_game/tests/app/riders.rs`:

1. The snapshot has riders, with their side position, size and archetype.
2. A rider is drawn with more vertices than a car (a car is 8).
3. `AgentViews::riders` is above 0 in a run, and the mesh has more than 8 vertices for each
   agent.
4. The bicycle light leads: the bicycle is green while the car is red, and it changes.
5. Without a lead the bicycle light is the car light.
6. Riders appear in the report: the summary, the `Riders:` line and the `Bicycle` mode.

## Results

- `cargo fmt --check`: clean.
- `cargo clippy -p tf_game --all-targets -- -D warnings`: clean.
- `cargo test -p tf_game`: 51 unit tests and 60 app tests pass.
- `cargo build -p tf_game --features dev`: OK.

## Screenshots

All PNGs are in `/Users/antoinepouligny/Documents/Projects/Dev/tf-p6-shots/`. They are not in
the repository.

Level 1, empty design (`TRANSIT_FLOW_DATA_DIR=/tmp/tf-p6-views2-empty`), zoom 1:

- `w6b-riders-1200.png`: frames 1200 (t 20 s). The HUD shows `3 vehicles (1 riders)`. The
  rider is a violet body with a dark dot in the bicycle lane of the south arm.
- `w6b-riders-1500.png`: frames 1500 (t 25 s). The rider is in the bicycle lane of the south
  arm, on its way away from the junction.
- `w6b-riders-2000.png`: frames 2000 (t 33 s). The rider is at the stop line of the south
  arm, in the green bicycle lane.
- `w6b-riders-detail.png`: a 4x crop of the rider in `w6b-riders-2000.png`. It is a crop of
  that PNG, not a separate render. The violet body, the dark rider dot and the light front
  mark show.
- `w6b-riders-detail-1500.png`: a 4x crop of the rider in `w6b-riders-1500.png`.

Design with signals, a lead and boxes (`TRANSIT_FLOW_DATA_DIR=/tmp/tf-p6-views2-data`,
`designs/carrefour-ecole.json`: `SetJunctionControl` TwoPhase green 20 s,
`SetBicycleLead` 4 s, `AddBicycleBox` on segments 1 and 2), zoom 1:

- `w6b-signals-lead-1562.png`: frames 1562 (t 26 s, 1 s into phase 1). The north and the
  south approaches show a red car disc with a green bicycle disc below it. The east and the
  west approaches show red and red. The green bicycle boxes show on the west and the east
  approaches.
- `w6b-signals-lead-3062.png`: frames 3062 (t 51 s, 1 s into phase 0). The east and the west
  approaches show a red car disc with a green bicycle disc below it. The north and the south
  approaches show red and red. This is the same leading green on the other pair of
  approaches.

The two signal PNGs together show that the small light follows the run and not a fixed
colour.

## Open points

- **The closest colour pair.** The riders' violet family and the walkers' indigo
  (`commuter on foot`, 0.35/0.22/0.65) are the nearest pair. The shape tells them apart: a
  rider is a 1.8 m x 0.6 m body with a dot, a walker is a small dot. At the default framing
  the rider body is about 11 x 4 px and the rider dot about 2 px. A higher zoom shows the
  rider well. The user decides if the rider needs a bigger mark.
- **The front mark of a rider** is 0.3 m long, about 2 px at the default framing. The dark
  dot shows the rider better than the mark shows the heading.
- **The bicycle light while designing** is a fixed green. It says that a lead is set; it does
  not show the light of the moment, because there is no run to read.
- **The HUD count.** `N vehicles (R riders)` counts the riders inside the vehicle count,
  because the run counts them as agents. If the user wants them apart, it is a one-line
  change.
- **Incidents.** `tf_game` adds no incident code. The run's incident list already has the
  close passes and the contacts of the riders (`ConflictKind::ClosePass`), and the report
  reads them.
- **`tf_sim` is not changed.** `AgentPose::lateral` and `ApproachLight::bicycle` are copies of
  values that `tf_sim` already gives.
