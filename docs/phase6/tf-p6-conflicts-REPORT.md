# Phase 6 W4b report: classic conflicts on the street, and track < painted lane < no facility

Branch `p6-conflicts`, worktree `../tf-p6-conflicts`, from `pivot-phase-6` 834849d.
Package W4b of the pivot plan, section 8 "Phase 6 — Cyclists".
Only `tf_sim` changed.

## 1. Result

Items 1 to 7 work and have tests. Item 8 is fixed for the rider on the zebra; the
walker/car wait at a zebra stays open (it is a W4a open point).

The "done when" test passes. On the test street, the protected track gives 0 conflicts,
the painted lane 4, and no facility 5. The margin between the lane and no facility is
small. Section 8 gives the cause and the open point.

## 2. What the run reads now

Before this package, nothing in the run read `Separator::crossability`. A painted lane
and a kerb track behaved the same.

Now one function decides: `cross::may_cross`. It reads the separator between two strips
and the access of the vehicle:

| Separator | A vehicle may cross |
| --- | --- |
| `Free` | always |
| `Paint` | only when the driver bends the rule (`cross::bends`) |
| `Blocked` (kerb, bollards, planter) | never |

The run asks this question in four places:

1. A vehicle that passes a queue in the bicycle lane beside it (`bike_lane_pass`).
2. A van that stops in the bicycle lane (`delivery_stop_place`).
3. A driver that cuts the corner across the bicycle lane (`cut_corner`).
4. A rider that leaves its lane around an obstacle (`leave_lane`).

`cross::bends` gives one draw per agent and per rule, from the run seed and the agent
id. The chance grows with the lack of compliance, the risk and the frustration. Thus one
driver can stop on a painted lane and still not cut a corner.

## 3. New files

| File | Content |
| --- | --- |
| `crates/tf_sim/src/street/run/cross.rs` | `Neighbour`, `neighbour`, `same_direction`, `may_cross`, `bend_chance`, `bends`, the four rule streams, `delivery_stop_place`. 2 unit tests. |
| `crates/tf_sim/src/street/run/door.rs` | `Door`, `Doors` (one stream per parking strip), `Doors::new`, `step`, `open`, `ahead`. 2 unit tests. |
| `crates/tf_sim/tests/street_conflicts.rs` | 5 micro-scenarios: squeeze, van in a painted lane, door, early right turn, walkers on a bicycle lane. |

Changed files: `run/mod.rs` (the two modules, the `doors` field, `open_doors`),
`run/drive.rs` (door brake, delivery stop, shoulder checks, `held_by_walkers`),
`run/bike.rs` (squeeze, pass clearance, `lateral_limit`, `leave_lane`, `cut_corner`,
queue pass), `run/agent.rs` (`DeliveryStop`), `run/spawn.rs` (the stop draw),
`run/perceive.rs` (`DriverTask::PullIn`, `PullOut`), `run/conflicts.rs` (close pass in
the lane of the rider), `tests/street_bike_design.rs`, `tests/street_run.rs`.

## 4. New constants

| Constant | Value | Source or start value |
| --- | --- | --- |
| `cross::VAN_STOP_CHANCE` | 0.25 | Start value: one van in four makes a delivery stop. |
| `cross::CAR_STOP_CHANCE` | 0.15 | Start value, times the lack of compliance: a careful driver never stops on a lane. |
| `cross::STOP_DURATION_S` | 20.0 s | Start value: unloading a parcel. |
| `cross::STOP_APPROACH_M` | 25.0 m | Start value: the van pulls over only near its place. |
| `cross::STOP_STREAM`, `QUEUE_STREAM`, `CUT_STREAM`, `SQUEEZE_STREAM` | 4 seeds | One stream per rule, so one draw per agent and per rule. |
| `door::PARKED_CAR_LENGTH_M` | 5.5 m | Start value: a car and the room to get out. |
| `door::DOOR_CHANCE_PER_CAR_S` | 1/900 | Start value: a car parks or leaves every few minutes, and a door opens once per stop. |
| `door::DOOR_OPEN_S` | 4.0 s | Start value: getting in or out with bags. |
| `door::DOOR_OPEN_M` | 1.0 m | A car door is about 0.9 to 1.1 m wide. |
| `door::DOOR_LOOK_M` | 12.0 m | Start value: the person in the car looks before it opens the door. See section 7. |
| `bike::SQUEEZE_ROOM_FACTOR` | 0.4 | Start value: the share of the rule a driver keeps when it squeezes past. |
| `bike::PASS_MARGIN_M` | 0.3 m | Start value: the room the rule asks stays while the body eases sideways. |
| `bike::PULL_IN_CLEAR_M` | 20.0 m | Start value: a vehicle does not pull in front of a body that comes up behind it. |
| `bike::CUT_REACH_M` | 25.0 m | Start value: how far before its line a driver may cut a corner. |
| `bike::LEAVE_REACH_M` | 25.0 m | Start value: how far ahead a rider looks for an obstacle in its lane. |
| `bike::LEAVE_SPAN_M` | 20.0 m | Start value: the stretch that must be free before a rider leaves its lane. |
| `drive::PULL_IN_LOOK_M` | 0.4 m | Start value: a vehicle that moves this much to its right looks over its shoulder. |

The rule of the side room is R414-4: 1 m in town. `bike::PASS_CLEARANCE_TOWN_M` already
held it.

## 5. Items 1 to 8

**Item 1, the separator matters.** `cross::may_cross` is the only place that reads
`Separator::crossability`. The four callers are in section 2. A kerb, bollards or a
planter stop every vehicle the same way. A painted line is a rule to bend, not a wall.

**Item 2, squeeze past.** `bike::squeeze_chance` uses the lack of compliance, the risk,
the frustration and the hurry (the speed factor above the limit; a rider has no hurry).
`squeezes` gives one draw per agent. A driver that squeezes keeps
`SQUEEZE_ROOM_FACTOR` of the rule; the others keep the rule and `PASS_MARGIN_M`. A
careful driver never squeezes, so it waits or passes with the whole metre.
Measure on the micro-street, 6 seeds, 300 cars and 200 riders per hour: careful
population 0 close passes and 0 contacts; careless population 7 close passes.

**Item 3, vans and cars in a painted lane.** The stop is a chance per van (and per
careless car), not level data: `VAN_STOP_CHANCE` and `CAR_STOP_CHANCE`. The place is the
middle of the longest painted bicycle lane on the right of the route
(`delivery_stop_place`). The van pulls over in the last `STOP_APPROACH_M` metres and
stands `STOP_DURATION_S` seconds. A rider behind it leaves the lane when the way is free
(`leave_lane`), else it brakes. A car may drive in the painted lane to pass a queue
(`bike_lane_pass`). A kerb, bollards or a planter block both, because `may_cross` says
no.

**Item 4, the door of a parked car.** `Doors` gives one stream per parking strip, from
the run seed. The chance is per parked car and per second, with the number of cars taken
from the length of the strip. The door reaches `DOOR_OPEN_M` into the travel strip beside
the parking. A rider in the door zone brakes for it or leaves the lane. A buffer strip
between the parking and the lane puts the door out of reach, and so does a track that is
not beside the parking.

**Item 5, early right turn.** `cut_corner` moves a driver that turns right into the
painted lane on its right, up to `CUT_REACH_M` before its line, when the separator lets
it cross and the driver bends the rule. A kerb track forces the turn at the corner.

**Item 6, walker against rider.** The micro-test `walkers_crossing_a_bicycle_lane_meet_riders`
puts walkers across a bicycle lane away from a zebra. The riders meet them: 2 conflicts
with a walker on one side. The W4a slip rule at zebras stays.

**Item 7, the "done when" test.** See section 6.

**Item 8, open points.** A rider that stands for walkers on a zebra now sets
`held_by_walkers`: the condition no longer needs the agent to be at its line, only that
it stands for a blocker (`view.blocker.is_some() && view.target <= target && v <
STOPPED_SPEED`). The walker/car wait at a zebra is not touched; it stays a W4a open
point.

## 6. Measures

The "done when" test: the west arm of the school level, both arms of the facility, 24
seeds, one slice of 240 s, 400 cars and 120 vans per hour each way, 400 riders per hour
eastbound. The metric of the brief is the serious conflicts, and the close passes (light
or serious) count as conflicts. A contact is not a conflict; the test reports it apart.

| Facility | Serious conflicts | Close passes | Contacts |
| --- | --- | --- | --- |
| Protected track | 0 | 0 | 0 |
| Painted lane | 4 | 0 | 7 |
| No facility | 2 | 3 | 0 |

The W4a test on the same file still passes: priority 51 serious, 73 conflicts with a
rider, 23 contacts; signals 21, 42, 15; bicycle box and leading green 13, 65, 2.

## 7. Test premise changes

1. **The facility runs on both arms of the school street.** A lane that ends at the
   junction makes the riders merge into the car lane after it, and that merge conflict
   has nothing to do with the separator.
2. **Oncoming traffic.** Without it, a car always has room to pass a rider with the
   whole metre, and a shared lane looks safe.
3. **The close pass needs the car and the rider in the same lane.** A car that keeps to
   its own lane beside a bicycle lane does not overtake the rider: the room between them
   is the width the design gave the two strips, not the room the driver left. Without
   this rule, every car beside a rider in a 2 m lane was a close pass, and the painted
   lane looked worse than it is.
4. **The door test accepts a rider that brakes or leaves the lane**, and its rider flow
   is 1200 per hour, so that a rider meets an open door.
5. **`DOOR_LOOK_M`.** A person in a parked car looks before it opens the door. Without
   this rule, a door opened 1.8 m in front of a car at 20 km/h and the car braked at
   8.4 m/s² (`street_brake`, seed 2). That brake is not a driver error, and the test
   rightly refuses it.

## 8. Open points

1. **The margin between the painted lane and no facility is small: 4 conflicts against
   5.** The cause is in the model, not in the test. In a shared lane the cars come up
   behind the riders and queue: the cost is delay, and the conflict metric does not show
   delay. The painted lane removes that cost, but it lets a van stop in it, and the
   riders meet the vans (7 contacts). Thus the lane has fewer conflicts and more
   contacts. The user must decide if the metric must include delay, or if a squeezing
   driver must also accept a tight gap to the oncoming traffic (it does not today: the
   corridor check stops it).
2. **The walker/car wait at a zebra** (W4a open point) is not touched.
3. **A car has no swerve.** A rider leaves its lane around an obstacle; a car only
   brakes. A door or a van that appears close therefore gives a hard brake, not a swerve.
   Swerving as a general reaction is package W5.

## 9. Text for the plan

Section 8 of `URBANISM_PIVOT_PLAN.md`, "Phase 6 — Cyclists", can say:

> The run reads `Separator::crossability` for every vehicle that leaves its lane. A
> painted line is a rule to bend; a kerb, bollards or a planter are a wall. A van stops
> in a painted lane and the riders go round it; a car passes a queue in it; a driver cuts
> the corner across it. A parked car opens its door into the lane beside it, and a rider
> in the door zone brakes or leaves the lane. On the test street the protected track
> gives 0 conflicts, the painted lane 4 and no facility 5; the painted lane also gives 7
> contacts with the vans that stop in it.

## 10. Checks

- `cargo test -p tf_sim`: pass.
- `cargo clippy -p tf_sim --all-targets -- -D warnings`: pass.
- `cargo fmt`: done.
- `cargo check -p tf_game`: pass.
- The four replay hashes in `tests/street_run.rs` moved on purpose. The comment names
  the old values.
