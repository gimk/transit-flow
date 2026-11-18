# Review: drive.rs and meet.rs after the Phase 6 merges

Branch `pivot-phase-6`, commit 28e8b0e. Read-only review. I did not build or run anything. Each finding was checked against the code. Where a finding rests on arithmetic from the code and not on a run, the text says so.

Files: `crates/tf_sim/src/street/run/drive.rs` (1379 lines), `crates/tf_sim/src/street/run/meet.rs` (500 lines). I also read `mod.rs` (decide, integrate, snapshot stages), `agent.rs`, `yielding.rs`, `conflicts.rs`, and the three Phase 6 reports.

Severity key: **bug** = wrong result in a reachable state. **risk** = wrong or fragile in some states, or a cost problem. **cleanup** = no wrong result, but it costs readability.
"Changes hashes" = behaviour change. It moves the replay hashes. It is the user's call.

## Summary

- 2 bugs (F1, F2)
- 6 risks (F3 to F8)
- 4 cleanups (F9 to F12)
- Determinism: no finding. No `HashMap`, no `swap_remove`, no `sin`/`cos`/`exp`/`powf`/`atan2` in `drive.rs`, `meet.rs`, `bike.rs`, `perceive.rs` (checked with grep). All loops go over `IndexMap` or `Vec` in id order. Only `sqrt` is used.

---

## Findings, most severe first

### F1. A vehicle that stops with its front on the line counts as committed and drives in (bug, changes hashes)

- Where: `drive.rs:980-991` (sets `committed`), `drive.rs:1061-1063` (the stop branch needs `!committed` and `d < line_d`), `mod.rs:707`, `mod.rs:818-826` (`in_junction` in the snapshot).
- Explained in section "The committed rule" below.
- Failure scenario: seed 47. The east car brakes late (stale view) and stops with its front at `line_d`. In the next tick `agent.d >= approach.line_d` is true. `committed` becomes true and stays true. The stop branch is now dead for this car. Its target is the normal desired speed, so it pulls away from standstill into the junction. Only `EnteredWithoutGap` (W3) records it. Behaviour is unchanged.

### F2. `AT_LINE` (4 m) is too short for a car that waits at a zebra before the line (bug, changes hashes; from arithmetic, not from a run)

- Where: `drive.rs:1088` (`stopped_at_sign`), `drive.rs:1141-1146` (`held_by_walkers`). Constants: `DEFAULT_CROSSING_WIDTH = 3.0` (`level.rs:174`), `ZEBRA_LINE_MARGIN = 0.3` (`yielding.rs:53`), `ZEBRA_AT_LINE = 1.0` (`yielding.rs:62`).
- What is wrong: `stop_point` (`yielding.rs:467-478`) stops a car at `area.enter - 0.3` when the zebra exit is less than 1 m before the line. So the front stops at `line_d - exit_gap - 3.0 - 0.3` from the line, that is 3.3 to 4.3 m. The test is `< AT_LINE` (4.0). It fails when the exit gap is more than 0.7 m (width 3.0), or always when the zebra is wider than 3.7 m.
- Failure scenario A (stop sign): a car with `full_stop` waits at the zebra. `stopped_at_sign` is never set, because `to_line` is 4.1 m. After the walkers leave, `junction_blocks` still returns `Block::StopSign` (`drive.rs:515-521`), the car stays at the zebra edge, and it waits until the give-up time (`GIVE_UP_S`).
- Failure scenario B (held cars): a head car held by walkers at 4.1 m never gets `held_by_walkers = true`. The cars that give way to it (`drive.rs:594`) still count it as "about to go" and wait for it. This is the ring problem that W1 patched with `RING_REACH` (6 m) in `stands_at_line`, but only there and only with a cyclist about. The two other places still use `AT_LINE`.
- Fix: one function `at_line(agent_or_seen, world)` that uses the stop point (zebra aware) and one reach constant. Use it in the three places and in `stands_at_line`. Remove the cyclist gate for it.

### F3. Leader position is extrapolated by `speed x reaction` only for pairs with a cyclist (risk, changes hashes if ungated)

- Where: `drive.rs:889-897`.
- What is wrong: the extrapolation assumes the leader keeps its old speed. It is optimistic in exactly the dangerous case: a leader that braked in the last reaction time. The leader's stale speed is also used in `safe_speed`, so the follower is fooled twice in the same direction.
- Failure scenario: a car brakes hard for a walker. A rider follows at 5 m/s with `BIKE_HEADWAY_FACTOR` x headway. The snapshot shows the car at 5 m/s. The rider takes the car as 4 m further on (5 m/s x 0.8 s) than it is. The gap is overestimated by up to 4 m.
- Gate question: this is a general fix gated to keep old hashes. Car-to-car following double-counts the delay (the stale position makes the gap too small by `v x tau`, and `safe_speed` has a `tau` term again). That is why a car-only street is safe, and also why it follows loosely. The extrapolation is only needed because a rider keeps less than `tau` of headway.
- Fix proposal: do not extrapolate the other. Measure the gap in the snapshot frame: take the agent's own `Seen` from the same snapshot (`d_then`), and use `front_then - d_then`. No gate is needed, and it is correct for every pair. Behaviour change for car-only runs (they follow closer).
- Also: the lag exists only for the on-route branch. The sampled branch (`drive.rs:905-945`) has none.

### F4. `arrives` and `leaves` of the other vehicle use car acceleration and the lane limit for every kind (risk, changes hashes)

- Where: `drive.rs:570-575` and `drive.rs:615-619` (also `box_blocks`, `drive.rs:758-764`). `travel_time` uses `ACCELERATION` (2.0). The top speed is the lane limit.
- What is wrong: for a rider the earliest arrival is computed with 2.0 m/s² (a rider has 1.3) and the lane limit (a rider's own pace is about 5 m/s, `speed_factor`). The earliest arrival is too early. A driver or a rider waits for a rider who is still far.
- Failure scenario: a rider stands 6 m before its conflict zone, speed 0.5 m/s, on a 30 km/h street. The computed `arrives` is about 2.9 s. The real value is about 3.7 s or more. A car with `margin` of 2 s waits for a rider who needs another second.
- Fix: `Seen` already has `bike`. Give `Seen` the top speed (or acceleration) of the agent, and call `travel_time_with`. Conservative error, so low severity.

### F5. The PET band ignores the side position of a rider (risk, changes hashes)

- Where: `meet.rs:394-411` (vehicle cells), `meet.rs:417-430` (walker cells, `BAND_HALF_WIDTH = 0.9`).
- What is wrong: the band is the route line plus or minus 0.9 m plus the walker radius. It does not use `agent.lateral`. W1 gave riders and passing cars a side position (`bike::lateral_plan`).
- Failure scenario: a rider rides 1.3 m right of the lane centre. A walker crosses 1.3 m from the route line (`beside` = 1.3 > 0.9 + 0.25 = 1.15). The walker is not in the band. A near miss (PET under 1 s) is not logged. The reverse also holds: a car that moved over to pass a rider is still counted in the band it left.
- Fix: shift the walker's `beside` by the vehicle's `lateral` (compute `beside - lateral` per vehicle in the cell), or build the band from `pos` samples. Also use the body width of the vehicle, not a car half width, for a bicycle.

### F6. Braking constants of riders (risk, changes hashes)

- Where: `drive.rs:1068`, `drive.rs:1189-1192`, `drive.rs:32`, `drive.rs:99`.
- (a) `no_return` uses `HARD_BRAKE` (4.5). A rider has a maximum of 4.0 to 5.5 and a conflict threshold of 3.0 (`brake_thresholds`). A rider is "not able to stop" at a longer distance than the car formula says. Use `brake_thresholds(agent.mode).0`.
- (b) `COMFORT_BRAKE` (3.0) is used for riders (`safe_speed`, braking floor) and `BIKE_HARD_BRAKE` is also 3.0. Any braking a little above comfort is logged as a light conflict (`brake > threshold`). Failure scenario: `need` = 3.02 in a normal stop gives a `HardBrake` event for a rider, and not for a car. Give riders their own comfort value, below `BIKE_HARD_BRAKE`.
- (c) `REFERENCE_BRAKE` (8.5, a car value) scales the pedal rise in `needed_brake` for riders. For a rider (max 4 to 5.5) the rise is too long. Small effect.

### F7. A body beside the agent gives a negative gap and a full stop (risk, from the code, not from a run)

- Where: `drive.rs:894-904`, then `drive.rs:1121-1127`.
- What is wrong: `front <= agent.d` skips only a body that is wholly behind. A rider whose rear is behind the car's front, whose front is ahead, and whose side clearance is 0 to 0.3 m, gives `gap = front - length - d < 0`. `safe_speed(gap - min_gap)` returns 0 and `needed_brake` uses `distance.max(0.1)`. The car brakes at its maximum for a body that is level beside it.
- Failure scenario: a rider filters up next to a stopped car (`bike::lateral_plan`) and ends level with the car's nose at 0.2 m clearance. The car (stopped) holds. A moving car would brake at the limit and log a `HardBrake`.
- Fix: for a body that overlaps in length, treat it as "beside" (skip) when `side > 0`, and as a contact risk (stop) only when `side <= 0`.

### F8. The "only while a cyclist is on the street" scans cost `O(N^2)` to `O(N^3)` per tick and flip behaviour (risk)

- Where: `drive.rs:610` (inside the loop over `other`), `drive.rs:1276` and `drive.rs:1280` (`stands_at_line`), which `leads_ring` calls inside a filter over all of `snapshot.seen` (`drive.rs:798`). `queued_behind` (`drive.rs:1293`) is a scan inside the loop.
- Cost: with no cyclist, `any(|s| s.bike)` scans all N entries every time. In `leads_ring`: N calls of `stands_at_line`, each with two scans of N, is `O(N^2)`. `leads_ring` is called once per standing `other` of each agent that is in its decision window, each tick. At 400 agents that is about 160 000 operations per call. No measurement was done.
- Behaviour: the gate is read from the snapshot of the deciding agent. Snapshots have different ages (reaction time per agent, `mod.rs:684-686`). When a rider spawns or leaves, `AT_LINE` (4 m) changes to `RING_REACH` (6 m) for car-only pairs far from the rider, and two agents of one ring can read different snapshots. `leads_ring` says "Everyone reads the same past tick" (`drive.rs:787`). This is wrong: each agent reads the snapshot that is `reaction_s` old. Two agents of a ring with different reaction times can both lead or both wait. The risk exists without cyclists, and the gate makes it show up with them.
- Fix: add `has_bike: bool` and a per-lane head table to `Snapshot` (built once in `snapshot_stage`). Better: remove the gate (see the gate table below). Fix the doc of `leads_ring`, or make the ring check read one fixed tick (for example the oldest snapshot any member uses).

---

### F9. Gate table: bike-specific, or a general fix kept behind a gate? (cleanup / decision)

| Rule | Place | Verdict | Cleaner without the gate? |
|---|---|---|---|
| Red light by habit `red_u` and `cross_traffic_empty` | `drive.rs:486-502` | Really rider specific. | Keep. But `empty` is computed before the `careful ||` short cut: compute it only when `!careful`. Wasted call per tick. |
| Amber uses `obeys_signal` for riders | `drive.rs:507-512` | Inconsistent with red (red uses `red_u`). | Decide on purpose. |
| Perception between kinds, `bike != other.bike` | `drive.rs:551`, `drive.rs:739` | Kind specific by design (cars among cars keep `approach.unseen`). | Keep, but give it a name (`must_perceive`). Two systems now answer "does A see B" (see F10). |
| Queue followers hold nobody (`queued_behind`, "speed < STOPPED") | `drive.rs:610-614` | General. A car-only ring with one queue follower per arm is the same case. | Yes. Changes hashes. |
| `stands_at_line`: `RING_REACH`, head of queue only | `drive.rs:1275-1288` | General (and see F2). | Yes. Changes hashes. |
| Leader lag | `drive.rs:889` | General (see F3). | Yes, with the snapshot-frame fix. |
| `accel_of`, `min_gap_of`, `headway`, `brake_thresholds`, `desired_speed_with` | many | Really rider specific data. | Keep. But `agent.bike.is_some()` (many places), `mode == Bicycle` (`drive.rs:249`, `meet.rs:354`) and `other.bike` are three ways to ask "is it a bicycle". Pick one. |
| `CYCLIST_IMPACT_FACTOR` | `meet.rs:353` | Really rider specific. | Keep. The logged `impact_kmh` is the unscaled value. Say so in the doc of `CauseKind`/`Contact`. |

### F10. Two or more ways to answer "does A see B, does A yield to B" (cleanup)

- Vehicle with vehicle of the same kind: the random draw `approach.unseen` (`drive.rs:546`, `drive.rs:734`).
- Driver with rider, rider with driver: `notices_other` (`drive.rs:652`), a `perceive::Looker`.
- Driver with walker: `yielding::walker_view`.
- Cause text after a contact: `Looker` built again in `meet.rs:197-205` (walker) and `conflicts.rs:123-131` (rider). Four copies of the same `Looker { mode, eye, dir, table, seed, fov, alternate }` literal (with `drive.rs:654`).
- In the `careful` pass `notices_other` is skipped, so for an unnoticed rider `cause_of` gives `GapTooShort` (`drive.rs:848`), and the perception cause (`InBlindSpot`, `LookedElsewhere`) comes only from `conflicts.rs`. Both causes can be in one chain. This is not wrong, but it is hard to read.
- `notices_other` takes its memory (`snapshot_before`) from `world.peds` (`drive.rs:682`). With `peds = None` there is no memory. That is an odd link between pedestrians and history.
- Fix: one `Looker::for_agent(agent, eye)` constructor in `perceive.rs`; one `decided_from(&self, agent) -> &Snapshot` in `StreetRun` (the same three lines are in `meet.rs:120-123`, `meet.rs:180-183`, `mod.rs:684-686`).
- Duplicates inside `drive.rs`: the margin block (`drive.rs:523-531` and `drive.rs:707-715`), the gate with `notices_other` (`drive.rs:551-555` and `drive.rs:739-743`), the zone and time computation (`drive.rs:559-630` and `drive.rs:745-780`). A shared `crossing_times(agent, other, ...)` would remove about 40 lines.

### F11. Dead code, waste, magic values (cleanup)

- `meet.rs:432`, `meet.rs:439`, `meet.rs:441`, `meet.rs:446`, `meet.rs:448`, `meet.rs:469`: the fourth tuple element `Vec2::ZERO` of `found` is never read. Remove it.
- `meet.rs:433`: `self.bands.keys().chain(now.keys())` has every key twice when it is in both. The second pass does nothing (checked: it is idempotent), but it clones the vectors again. Dedupe the keys.
- `drive.rs:1255` `QUEUE_GAP_M` (8), `drive.rs:87-88` `QUEUE_REACH` (12), and `drive.rs:1285` `RING_REACH + 2.0` (8) are three values for "a vehicle close ahead in a queue". `QUEUE_CRAWL` (1.5) and `STOPPED_SPEED` (0.3) are two values for "ahead is not moving". One pair of constants.
- `meet.rs:9`: a doc line was not wrapped. The module doc does not say that riders are in the contact and PET code.
- `SPEEDING_FACTOR` (`conflicts.rs:23`, 1.15) equals `BIKE_LIMIT_TOLERANCE` (`drive.rs:104`, 1.15) by chance. A rider at its allowed maximum is not "speeding" only because the test is `>`. Share one constant, or state the link.
- Missing `///` on public items: `World` (`drive.rs:173`), `critical_gap` (`drive.rs:343`), `MoveInfo::node`, `MoveInfo::from_segment`, `Block` variants `Red`, `Amber`, `StopSign`, `ExitFull`, `Decision` fields `accel`, `go`, `stopped_at_sign`, `committed`.
- Constants without a source note: `BIKE_GAP_FACTOR` and `BIKE_HEADWAY_FACTOR` ("our own"), `QUEUE_CRAWL`, `QUEUE_REACH`, `RING_REACH` (start values only), `COMFORT_BRAKE` for riders (see F6), `MEET_REACH`. The `BIKE_*` limits say "start values" with a reason; the queue constants do not.
- `ZEBRA_LOOK_M` (`drive.rs:1270`) is defined after the function that uses it and sits between two items. Move it up.

### F12. Long functions (cleanup)

- `decide` is `drive.rs:957-1199`, 243 lines, with 9 blocks in sequence (lateral plan, line pass, crossing, distraction, road speed, approach, box, obstacle, pedestrians, accel). Split by block; each block returns a small struct (`target`, `need`).
- `junction_blocks` is `drive.rs:464-646`, 183 lines. The loop body can move to `block_from_other(...) -> Option<(Block, arrives, leaves)>`; this also removes most of the duplication with `box_blocks`.
- `nearest_obstacle` is `drive.rs:862-954`, 93 lines, two branches (on-route, sampled). Make each branch a function.
- `drive.rs` has 1379 lines. The ring and queue helpers (`leads_ring`, `stands_at_line`, `queued_behind`, `in_front_of`) and the task code (`driver_task`) can go to `drive/ring.rs` and `drive/task.rs` (module names are a proposal).

---

## The committed rule (item 5): how it works now

State: `Approach { go, committed, .. }` (`agent.rs:286-304`). `go` starts `false` (`mod.rs:599`). `committed` starts `false`.

1. In `decide`, when `d < line_d` and `to_line < v^2/(2 x COMFORT_BRAKE) + v x tau + DECIDE_MARGIN`, the driver decides `go` each tick (`drive.rs:1061-1089`). `no_return = approach.go && to_line < v^2/(2 x HARD_BRAKE)` keeps an earlier "go" when braking is too hard. If `go` is false, the target is the stop point (`yielding::stop_point`, 0.5 m before the line, or at a zebra edge).
2. The stop uses the snapshot, which is `reaction_s` old, and the brake builds up over `BRAKE_RISE_S`. A late stop can overshoot the 0.5 m margin. The front then reaches `line_d`.
3. In the next tick `d >= line_d` and `!approach.committed`. `decision.committed = true` (`drive.rs:980-984`). If `go` was false, `EnteredWithoutGap` is recorded and the careful check adds its causes. `mod.rs:707` sets `approach.committed`. Nothing resets it.
4. From then on the block at `drive.rs:1061` (`!approach.committed && d < line_d`) never runs again. The car has no stop target. It follows the desired speed and accelerates from standstill. Its only check is `box_blocks` (`drive.rs:1094-1106`), which looks only at cars already in the junction, not at the priority traffic at the other lines.
5. The snapshot marks the car `in_junction` from the same test (`mod.rs:818-826`: `d >= line_d && d - length < end_d`). So other cars see it as in the box at speed 0 (`arrives = 0`, `leaves = INFINITY`, `drive.rs:566-568`).

Two flaws follow. The first: "front on the line" is used as "has decided to go". The second: others cannot tell a car that has rolled 5 cm over the line (still waiting) from a car in the junction. Seed 47 and the `EnteredWithoutGap` cases come from the first flaw. The try in the bike report ("a car that rolled less than 1 m over its line is still stopping") failed because of the second flaw: others took the car as in the junction and waited for it at `leaves = INFINITY`.

### Fix shape for W4 (no code)

1. **Separate "past the line" from "committed".** Set `committed` when the front is past the line and one of these is true: `approach.go` is true; the speed is above `STOPPED_SPEED`; the overrun is above `OVERRUN_M` (about 1.5 m: the car cannot back up). A car that is stopped, `go == false`, and past the line by less than `OVERRUN_M` stays uncommitted.
2. **Keep the stop branch for an uncommitted car that is past the line.** Change the test at `drive.rs:1063` from `d < line_d` to `!committed` (the `to_line` becomes 0 at minimum; use `.max(0.0)`). The stop target is "hold here" (target 0). It decides `go` again each tick with `junction_check`. When `go` becomes true, it sets `committed`.
3. **Make the snapshot tell the truth.** In `snapshot_stage`, set `in_junction` to `approach.committed && d - length < end_d` (not only `d >= line_d`). For an uncommitted overrunning car, `in_junction` is false and `s_on_movement` is small. `stands_at_line` must then test `other.s_on_movement` with a positive limit too (the sign is no longer certain): use `-reach < s && s < reach`, or the zebra-aware `at_line` of F2. This is what the earlier try lacked.
4. **Keep `EnteredWithoutGap`.** It then fires only for a real case: a car that was distracted over the whole window, or one that took `no_return` with `go == false`. Note that a distraction over the whole window also leaves `go` at its start value `false` (`drive.rs:1002-1005` returns before the decision), so the cause is also logged for a car that never decided. Start `go` as `None`, or set a `decided` flag, so that the cause is not logged without a decision.
5. **Second cause of seed 47 (stale release).** The east car skips the north car while it is `held_by_walkers` (`drive.rs:594`), then sees the release one reaction time late. Optional: add `last_held: Option<u64>` to `Seen`. Exempt a held car only if it has been held for more than the largest reaction time (1.6 s) in the snapshot. Alternative: do not exempt, and let the existing `defers` rule do it from the other side.
6. **Tests.** Un-ignore `careful_drivers_and_walkers_never_touch_seed_47` and the seeds 26 and 32 cases. Add a unit test: a car that stops 0.1 m over its line does not enter the junction while a car with priority crosses. Add a test that a car that stops 0.1 m over its line does not block the others. The change moves the replay hashes in `crates/tf_sim/tests/street_run.rs`. The commit must say it is on purpose. Do F2 in the same package (same function, same constants).

Behaviour-changing fixes in this review: F1, F2, F3, F4, F5, F6, F7, and the "ungate" items of F9. Fixes that keep hashes: the cleanups in F10 (constructor, `decided_from`), F11, F12, the lazy `empty` in F9, and the `has_bike` cache of F8 (if the gate stays).
