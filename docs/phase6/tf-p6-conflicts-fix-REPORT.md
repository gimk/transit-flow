# W4b fixes: report

Status: **stopped on one failing test.** Contacts now follow the order. One older
assertion of the "done when" test fails. I did not tune anything (brief rule).

## What changed (`crates/tf_sim` only)

1. **Fix A** (`bike.rs`, `lateral_limit`): other bodies are moved forward by one reaction
   time, as in `hold_if_blocked`.
2. **Fix B** (`spawn.rs`, `try_spawn`): a car waits at the gate while a slower body of the
   same segment and the same entry node, in another lane, is within a stopping gap.
   New constant `ENTRY_GAP_HEADWAY_S` = 1.2 s (start value, documented).
   First version forgot the same-way check and blocked the other gate; it broke 3 zebra
   tests. Fixed with `from_node`.
3. **Squeeze, option (b)** (`bike.rs`, `corridor_clear`): a driver that `squeezes()` uses
   `CORRIDOR_MARGIN_M * SQUEEZE_ROOM_FACTOR` for oncoming bodies. No new draw, no new constant.
   `conflicts.rs`: two motor vehicles that meet head on with under 1 m of room are logged
   as `ClosePass` (`a` = the one furthest off its side position).
4. **"Done when" test:** `lane.contacts > 0` and its comment removed; added
   `lane.contacts <= none.contacts`.

## Tests

- `spawn.rs`: `a_car_does_not_enter_behind_a_slow_body_of_another_lane_of_its_gate`.
- `street_bike_design.rs`: `a_van_that_pulls_over_does_not_sweep_into_a_rider_it_overtakes`,
  `a_car_does_not_come_in_behind_a_rider_that_swerves_at_the_gate` (moving vehicles only; one
  contact at speed 0, seed 14, is a rider into a jam: other cause).
- `street_conflicts.rs`: `an_impatient_driver_squeezes_past_a_rider_against_oncoming_traffic`
  (careful 0, careless 3 head-on close passes in 12 seeds).
- Replay hashes: **did not move** (the 4 hashes use the empty design). Nothing to update.
- All `tf_sim` tests pass except the "done when" test.

## Tables (24 seeds, Mixed)

| Facility | Serious | Close passes | Contacts |
| --- | --- | --- | --- |
| Track | 0 | 0 | 0 |
| Painted lane | 9 (was 4) | 0 | 0 (was 7) |
| No facility | 2 | 3 | 0 |

W4a: priority 41 serious / 63 any / 18 contacts; signals 15 / 30 / 8; box and lead 7 / 32 / 1.

## Why the test fails

`lane.conflicts()` = 9, `none.conflicts()` = 5. Assertion at `street_bike_design.rs:306`.
The 7 contacts did not vanish: most became hard brakes (serious). Cause chains of the 9:

- **Seeds 5/2495, 6/2592, 8/2292, 13/1814, 20/4609** (5 events): a rider swerves into the car
  lane at x = 2.5 to 5.5 m (`leave_lane` for a standing van), a car already in the lane behind
  it brakes at 7.5 to 9 m/s². `corridor_clear` checks nothing behind the rider. Fix B only
  holds cars that enter later. This is the review's optional second guard (no swerve in the
  first metres, or check followers). Not in the brief: not done.
- **Seed 6, t553/696/821** (3 events): riders brake hard for one standing van in the lane at
  x = 26 (4 to 5.5 m/s²). Real cost of a lane that lets vans stop.
- Seed 19/4476: same kind. Two events are rider against rider (not counted).

## Open points / decision for the user

1. Add the follower check to the rider's swerve (`leave_lane`/`corridor_clear`)? It should remove
   the 5 swerve events. Then the lane would be near 4 (the van brakes remain), still not below 5.
2. Or accept lane >= none on serious conflicts and change the assertion (the lane removes
   contacts and close passes, not hard brakes for vans). That is your call.

## Text for the plan (replaces section 9 of the W4b report)

> The run reads `Separator::crossability` for every vehicle that leaves its lane. A painted
> line is a rule to bend; a kerb, bollards or a planter are a wall. A van stops in a painted
> lane and the riders go round it or brake; a car passes a queue in it; a driver cuts the
> corner across it. An impatient driver squeezes past a rider and also accepts a tight gap
> to oncoming traffic (a head-on close pass). On the test street the protected track gives 0
> serious conflicts and 0 contacts. The painted lane gives 0 contacts (7 before the fixes) but
> 9 serious conflicts, mostly hard brakes for standing vans and for riders that swerve in
> front of a car. No facility gives 2 serious conflicts and 3 close passes. The ordering
> "lane better than none" is not yet met on serious conflicts.

---

# Update 2: look-back by traits (user decision via t13)

Status: **stopped again, on contacts.** Serious conflicts now follow the order. One contact
breaks `lane.contacts <= none.contacts`. I did not change the assertion.

## What changed

1. `bike.rs`: `looks_back(agent)` = a fixed hash draw (`cross::LOOK_BACK_STREAM`, same method
   as `squeezes`, no draw from the agent's `rng`) below the rider's `attention`. Careful riders
   have attention 1: they always look.
2. `bike.rs`: `follower_too_close(agent, target, snapshot)`: a motor vehicle behind the rider,
   in the place it moves to, faster than the rider, that could not stop (others moved forward by
   one reaction time; `FOLLOWER_BRAKE`, `FOLLOWER_MARGIN_S`, `FOLLOWER_ROOM_M` reused). Used in
   `leave_lane` (the rider then brakes for the obstacle) and when a rider starts a pass.
3. `spawn.rs` (Fix B again): the look-back alone did not change any of the 5 events. Cause:
   in seeds 5, 6, 8 the rider is already in the car lane when the car **spawns** 9 to 12 m
   behind it, 2.5 to 3 m/s faster. The first guard was 0.02 m short (seed 6, tick 2590). The
   gap now adds the braking distance (`ENTRY_BRAKE` = 3 m/s², start value) and a 2.0 s headway
   (`ENTRY_GAP_HEADWAY_S`, was 1.2; reaction 0.8 s + 1.2 s margin). Both constants are mine and
   documented.
4. Test `careful_riders_never_move_out_in_front_of_a_close_car` (no car or van hard brake
   for a rider, no contact, careful population, 24 seeds).

## Tables (24 seeds, Mixed)

| Facility | Serious | Close passes | Contacts |
| --- | --- | --- | --- |
| Track | 0 | 0 | 0 |
| Painted lane | 2 (was 9) | 0 | 1 |
| No facility | 2 | 3 | 0 |

Conflicts (serious + close passes): track 0, lane 2, none 5. W4a: priority 41 serious / 66
any / 23 contacts; signals 10 / 21 / 5; box and lead 3 / 22 / 1. One replay hash moved on purpose (seed 2, Careless x4) and was updated with a comment.

## The 5 move-out events

Gone: seeds 5, 6, 8, 13, 20 move-out events no longer appear. Cause was the spawn gap, not the
rider (the look-back alone left them unchanged). Remaining lane events: a car hard brake at the
gate (seed 18, t2388, rider in car lane) and a van pull-in behind a rider (careful seeds 1, 23
before the spawn change; see below).

## The one contact (seed 1, tick 2722)

Van 39 stands in the painted lane at x = 29 for its delivery stop. Rider 41 is
"Distracted" and rides into it at 17.9 km/h; the van's cause is "LookedElsewhere". The rider
braked hard at tick 2718 (serious) and did not leave the lane in time. So this is a rider hitting
a **standing** van in a painted lane: the original W4b premise, now real. The other contacts
of the run are car/van against car/van (not counted).

## Open point (decision for the user)

The lane has 1 contact, none has 0. Options: (a) a rider that is Distracted still notices a standing van sooner (perception work, not a constant); (b) the test reads contacts per rider instead of as a count. I did not change the assertion. Your call.

## Text for the plan (replaces the one above)

> ... On the test street the protected track gives 0 serious conflicts and 0 contacts; the
> painted lane gives 2 serious conflicts and 1 contact (a distracted rider against a van
> standing in the lane); no facility gives 2 serious conflicts, 3 close passes and 0 contacts.
> A rider looks back before it moves out, by its attention, and a car does not enter the street
> behind a slower body of its own gate that it could not stop for.

---

# Update 3: review of ab3cfd9 (the entry guard held for riders in their own lane)

Status: **done, the "done when" test passes with all its assertions.** Nothing was weakened.

## What changed

1. `spawn.rs` (Fix B): the guard holds a newcomer only for a body whose **edge is in the
   newcomer's first lane** or that is moving into it (`passing`). Sideways position: the lane
   offset difference plus the body's `min(lateral, lateral_target)`. A rider in its own painted
   lane no longer holds the entry. The braking distance and headway of update 2 are kept.
2. `bike.rs`, `lateral_limit`: found with the careful test (seed 1, tick 975, van 12 and rider
   10, careful population, contact 3.5 km/h). A van pulls over into the painted lane 3 to 4 m
   **behind** a rider that is only 0.9 m/s slower. The pull-in rule "ahead and slower by more
   than 1 m/s" did not hold it, so the van swept into the rider's tail. The rule now also holds
   when the gap is under the reaction distance plus `MIN_GAP` (existing constant). No new constant.
3. Tests: `riders_in_their_own_lane_do_not_cut_the_car_flow` (1209 cars and vans entered with
   and without riders, 24 seeds, same number). `Facility` now counts the cars that entered.

## Tables (24 seeds, Mixed)

| Facility | Serious | Close passes | Contacts | Cars entered |
| --- | --- | --- | --- | --- |
| Protected track | 0 | 0 | 0 | 1209 |
| Painted lane | 1 | 0 | 0 | 1209 |
| No facility | 2 | 3 | 0 | 1209 |

Conflicts: track 0, lane 1, none 5. Contacts 0 / 0 / 0. All assertions of the test pass.
W4a (24 seeds): priority 38 serious / 60 any / 20 contacts; signals 12 / 20 / 7;
box and lead 4 / 23 / 1. One replay hash moved on purpose (seed 2, Careless x4) and was updated with a comment.

## Text for the plan (replaces the one in update 2)

> On the test street the protected track gives 0 serious conflicts and 0 contacts; the
> painted lane gives 1 serious conflict and 0 contacts; no facility gives 2 serious conflicts,
> 3 close passes and 0 contacts (the same 1209 cars and vans entered in each case). A rider
> looks back before it moves out, by its attention. A car does not enter the street behind a
> body of its own gate that is in its lane and that it could not stop for. A van does not pull
> over into a lane just in front of a rider.
