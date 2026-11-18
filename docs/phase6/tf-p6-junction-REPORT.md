# W4a report: junction conflicts

Branch `p6-junction`, two commits on top of 28e8b0e. Not pushed. `cargo test -p tf_sim`, clippy `-D warnings`, `cargo fmt`, `cargo check -p tf_game`: all pass. The four ignored tests (seeds 47, 26, 32, 17) are un-ignored and pass. Replay hashes changed on purpose (old values in the comment).

## What changed
1. **F1, committed rule.** `Approach::go` is now `Option<bool>` (None: not decided). `drive::commits`: a vehicle past its line is committed when it decided to go, or when it cannot stop within `OVERRUN_M` (1.5 m) at its own hardest braking. Otherwise it keeps its stop branch and decides again. The snapshot marks `in_junction` only for committed vehicles. `EnteredWithoutGap` only for `go == Some(false)`. Seed 47 needed no stale-release step.
2. **F2, line reach.** `Seen::stop_gap` (distance from the front to the stop place, zebra aware via `yielding::stop_point_on`) and one `drive::at_line(stop_gap)` with one constant `AT_STOP_REACH` = 2.0 m. Used in `stopped_at_sign`, `held_by_walkers`, `stands_at_line`. `AT_LINE` and `RING_REACH` are gone. The cyclist gate is gone (cars-only runs changed).
3. **Gap rules against riders.**
   - F4: `Seen::{accel, pace}`; `arrival_limit` + `travel_time_with` for the other vehicle (rider pace and 1.3 m/s²).
   - Right turn (and any turn across) gives way to a vehicle from the same approach that turns less (`must_yield`): the right turn yields to a rider going straight on its right.
   - Left turn never leads a ring against oncoming straight traffic (R415-4; user decision).
   - Ring leader = lowest id (not longest wait: each agent reads a different snapshot age). New cause `LedRingAgainst { other }` when a ring leader goes in front of a vehicle with priority (careful check only).
   - A priority holder waits for a ring leader that stands at its line.
   - A priority holder gives way to a vehicle that has just pulled out (moving, under `PULL_OUT_REACH_M` = 3 m of its line).
   - A driver that was held by walkers itself a moment ago (`defers`) does not ignore other held vehicles (both are released together).
   - `no_return` only before the line.
4. **Riders on zebras (user decision A).** Careful riders hold like cars; a rider standing on a zebra waits for a walker in its path. `yielding::slips`: by compliance, risk, frustration a rider slips past a walker that is at least `SLIP_CLEARANCE_M` (0.7 m) from its body, at `SLIP_SPEED` (1.5 m/s). The draw is fixed per rider from the run seed and id (no stream shifts).
5. **Close passes (user decision C).** `ConflictKind::ClosePass`, `CyclistSummary::close_passes`: a motor vehicle level with a slower rider, same direction, side room under 1.0 m at over 5 m/s is light; under 0.5 m at over 10 m/s is serious (R414-4).
6. **Tests.** `tests/street_bike_design.rs` (new), unit tests in `drive.rs` (commit rule, right turn yields, at_line). Ignored tests un-ignored; seed skips removed from the loops.

## New constants (all start values)
`AT_STOP_REACH` 2.0 m, `OVERRUN_M` 1.5 m, `PULL_OUT_REACH_M` 3.0 m, `SLIP_CLEARANCE_M` 0.7 m, `SLIP_SPEED` 1.5 m/s, slip chance `(1-compliance)*(0.6+0.4*frustration)+0.3*risk`, `CLOSE_PASS_LIGHT_M` 1.0, `CLOSE_PASS_MIN_SPEED` 5, `CLOSE_PASS_SERIOUS_M` 0.5, `CLOSE_PASS_SERIOUS_SPEED` 10.

## Measures
Right-turn test street (west arm of level 1, parking strip turned into a painted lane, 300 cars + 100 vans turning right and 200 riders straight per hour, 24 seeds, mixed population; all car/rider conflicts and contacts):

| design | serious conflicts | contacts |
|---|---|---|
| painted lane, priority to the right | 52 | 25 |
| + signals (two phase) | 25 | 13 |
| + bicycle box 4 m | 17 | 8 |
| + box + leading green 4 s | 12 | 4 |

Before the right-turn yield rule (8 seeds, 200/120): lane 22 serious, 10 contacts; after 11, 9; with the pull-out rule 4, 1. A track (kerb) gives the same numbers as a lane at the junction.

Facility ordering (not a test, input for W4b; 12 seeds, 200 riders/h, serious conflicts / close passes / contacts with cars): 30 km/h none 0/4/0, lane 1/2/1, track 1/3/1; 50 km/h none 0/5/0, lane 1/2/2, track 1/3/1. Cause: nothing in the run reads `Separator::crossability`, so lane and track behave the same, and without a facility the rider shares the lane and cars follow it. No track/lane/none test was added (user decision).

Level 1 data: not changed.

## Test premise changes
- `street_meet::a_car_held_by_people_on_the_zebra...`: the reach in the test is a local 6.0 m (was `AT_LINE`).
- `street_meet::a_hidden_pedestrian_can_be_hit_by_an_inattentive_driver` (second commit, flag for review): `InBlindSpot` joins the accepted causes. Seed 12 careless: a walker crossing in the driver's blind spot, 12.6 km/h, no injury; the cause is the perception cause of W3 and names no other cause. Revert that commit if you do not want it.

## Open points
- Walker/car deadlock seen on the way (seed 2, stop sign on segment 3, before my last changes): a car standing 0.8 m inside a zebra and a walker stopped 1.9 m ahead of it waited 2000 ticks. It did not occur in the final runs, but the cause (walker stops in front of a car that stands on a zebra) is not fixed. A standing vehicle on a zebra now holds for a walker in its path.
- Busy x4 runs jam as in the base (delay 70-160 s, 30-40 unfinished): not changed.
- W4b: squeeze past riders, vans/cars in a painted lane, early right turn across a painted lane; `COMMIT_SPEED` cleanup; merge conflicts with `p6-tidy` expected in `drive.rs`, `mod.rs`.

## Text for the plan
Phase 6 W4a done: committed rule (a vehicle that stops over its line decides again), line reach from the stop place, gap rules against riders (real rider pace, right turn gives way to a rider, left turn does not lead a ring, pull-out rule), ring leader by id with a cause, riders slip between walkers by traits, close-pass conflicts. Measured: the right-turn conflict (52 serious, 25 contacts per 24 seeds) falls to 25/13 with signals, 17/8 with a bicycle box, 12/4 with box and leading green. Track against lane against none is not modelled yet (W4b).
