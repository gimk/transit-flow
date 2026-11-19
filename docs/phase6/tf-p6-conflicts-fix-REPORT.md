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
