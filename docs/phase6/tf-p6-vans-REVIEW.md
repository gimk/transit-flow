# Review: the 7 rider-vehicle contacts of the painted lane (W4b)

Read-only review. No source or test file changed. The throwaway tests are deleted.
Method: a temporary `#[ignore]` test stepped the "done when" street (painted lane on
segments 1 and 2, 24 seeds, Mixed) and dumped both bodies for 70 ticks before each contact.
A second one called `bike::lateral_limit` with the same snapshot the van used.

## Result in short

- **No contact is a rider against a van that stands still.** At the contact tick the vehicle
  is still moving (6 to 9 m/s), 13 to 17 m before its stop place (`at_d` = 34.5 m).
  The brief premise is wrong. The vehicle does not stand, so the rider is not wrong not to brake.
- 3 of the 7 vehicles are vans, 4 are cars (2 of them careless cars with a delivery stop).
- There are two different causes (A: 4 contacts, B: 3 contacts).

| seed | tick | vehicle     | cause |
|------|------|-------------|-------|
| 5    | 2343 | van (stop)  | A     |
| 6    | 432  | van (stop)  | A     |
| 7    | 352  | car (stop)  | A     |
| 11   | 717  | van (stop)  | A     |
| 1    | 3643 | car (stop)  | B     |
| 6    | 2573 | car         | B     |
| 9    | 1628 | car         | B     |

(Seed 7 tick 437 is rider against rider: not counted.)

## Cause A: the van sweeps into the rider it overtakes (4 contacts)

Seed 5, tick 2343 (rider 40, van 41). Chain:

1. The rider rides in the painted lane (y = 78.5, lateral 0, 3.8 m/s). Nothing blocks it, so it does not brake and does not leave the lane. `leave_lane` is correct here.
2. Van 41 spawns behind it at 9 m/s. It has a delivery stop at d = 34.5. From `STOP_APPROACH_M` (25 m) before, `drive::decide` sets `lateral_target = stop.lateral` (2.5 m, the bike lane).
3. The van overtakes the rider while it moves over. `bike::lateral_limit` must hold the van back while a body is beside it. It does so from tick 2316 to 2328 (target 0.75, 0.55, 0.45).
4. At tick 2332 the limit is lost. The van lateral then rises 0.6, 0.8, 1.0, 1.2 and the target is 2.5. The van tail (rear at d = 15.9) overlaps the rider front (d = 16.3) at lateral 1.2: van edge y = 78.2, rider edge y = 78.2. Contact, 7 km/h.
5. The rider cannot see a van behind it and does not react. The driver cause "LookedElsewhere" in the log is a side effect, not the root.

Why the limit is lost (reproduced): `drive::decide` gets `decided_from(agent)` (mod.rs:706), a
snapshot one reaction time old (`reaction_s` = 0.82 s here). `lateral_limit` compares the
**old** position of the rider (3.1 m behind where it is now) with the **current** position of
the van. The `beside` test (`|along| <= (L1+L2)/2 + SIDE_MARGIN` = 4.0 m) fails as soon as the
van is 1 m ahead of the rider centre, while the van tail still covers the rider. The `near` test is
false too (rider behind, slower). With the old snapshot, my reproduction gave limit = 2.5 from
tick 2332. With the other bodies moved forward by `speed * reaction_s`, it gave limit = 0.6, 0.8,
1.0, 1.2 (= the current lateral: the van holds its side position). So this is a **stale-position
error in `lateral_limit`**. It is not a perception miss, a lane-id or leader problem, a body-length
error or a contact-measure error. `hold_if_blocked` (bike.rs:774) and `beside_anyone` (bike.rs:752)
already move the others forward this way. `lateral_limit` (added in W4b) does not.

Seeds 6/432, 7/352 and 11/717 have the same dump: vehicle at lateral 0.25 to 0.65 twenty ticks
before, lateral 1.2 to 1.3 at contact, rider at lateral 0 and constant speed, vehicle still 6 to 7 m/s.

## Cause B: a rider swerves into the car lane, a car spawns right behind it (3 contacts)

Seed 1, tick 3643 (rider 62, car 63). Chain:

1. A van (57) stands in the bike lane at x = 31 (a real standing van). Rider 62 is at x = 3.4, 28 m before it.
   `leave_lane` fires (`LEAVE_REACH_M` = 25 m; along - length = 22 m). This is correct.
2. The rider moves left into the car lane (lateral 0 to -1.0 at 1 m/s; `passing` = true) from tick -19.
3. `corridor_clear` looks at `along > -agent.length` only (bike.rs:362): 2.6 m behind the rider. Nothing behind is checked.
4. Car 63 does not exist at tick -19. It spawns at the gate at tick -17 and is 5 m behind the rider at 9.7 m/s.
   `spawn::try_spawn` (spawn.rs:143-150) checks only agents whose route starts in the **same first lane**.
   The rider is in the bike lane, the car in the car lane, so the spawn is "clear".
5. The rider cannot see the car (it spawned after the rider snapshot). The car cannot brake in 4 m for a body in its lane at 9.7 m/s. Contact, 24 km/h.

Seeds 6/2573 and 9/1628: the car is absent from the dump 20 ticks before the contact (it spawned less than 1 s before). The rider has `passing` = true and lateral -0.6 to -1.2. Same chain.
(Seed 9: the rider is slow, 1.26 m/s, and "Distracted". The impact is 27.6 km/h.)
This cause does not need a van to stand still: any swerve in the first 8 m of the street is enough.
I did not check which obstacle triggered the swerve in seeds 6/2573 and 9. The spawn gap is the part I verified.

## Do all 7 share the cause?

No. A: 4 contacts. B: 3 contacts. Seed 1 has a little of both (car 63 drifts to lateral 0.25 for its own stop),
but the rider swerve plus the spawn is what makes the contact.

## Smallest fixes (not implemented)

**Fix A** (`crates/tf_sim/src/street/run/bike.rs`, `lateral_limit`): compute `rel` from the other body moved forward
by one reaction time, as `hold_if_blocked` does:
`let centre = other.pos + other.dir * (other.speed * agent.traits.reaction_s) as f32; let rel = centre - agent.pos;`
Use `rel` for both `along` and `side`. Checked offline: the limit then holds.
I did not run the 24 seeds with it (no source edit allowed), so the contact count after the fix is not measured.

**Fix B** (`crates/tf_sim/src/street/run/spawn.rs`, `try_spawn`): block the entry also when a body from the same gate, in
another lane, is slower than the newcomer and its rear is nearer than a stopping gap
(`rear < length + MIN_GAP + 1.0 + (v_spawn - other.speed).max(0) * headway`). A rider that swerves in the first metres then holds
the car back at the gate. Optional second guard in `bike.rs`, `leave_lane`: no swerve in the first metres of a route.

## Tests that must prove it

1. `crates/tf_sim/tests/street_bike_design.rs`: new test
   `a_van_that_pulls_over_does_not_sweep_into_a_rider_it_overtakes`. Level: `facility_level(0, 120, 400)`, painted lane on
   segments 1 and 2, seeds 1..25 (seeds 5, 6, 7, 11 included). Assert: no `Contact` between a motor vehicle and a rider where the vehicle
   moves faster than 1 m/s and has not reached its stop place. A tick-level variant for seed 5: the van lateral never goes past
   `rider_y - van_y - (w1 + w2)/2 - SIDE_MARGIN` while the rider is within 4 m along.
2. For B: one rider in the bike lane at d = 3 m, and a car flow of 1200/h: no contact in 24 seeds; and a unit test that `try_spawn`
   returns `Blocked` while a slower body is in the other lane within the gap.
3. The "done when" test asserts `lane.contacts > 0` (line 300), and its comment blames vans that stand. After fixes A and B this
   can reach 0. Then the assertion and the comment must change (the track must stay `== 0`, and the lane must stay worse than the track on
   close passes). That is the owner's decision. Replay hashes will move: the commit must say it is deliberate.
