# WP1 report: speed from the look of the street

Status: STUCK on 2 existing tests. Work is NOT committed (see below).

## What changed
- New `crates/tf_sim/src/street/speed.rs` (+ `pub mod speed;`): `street_speeds(street)` gives a speed per road lane from width, side friction (strip next to each edge, separators), obstacles within 1 m of the edge (density per 10 m, tall ones weigh 1.5), bounds 15 km/h to 59 km/h. All start values, documented. 5 unit tests.
- `drive.rs`: `World.street_speeds`; `desired_speed` on a road lane = `speed_factor * (c*min(limit,street) + (1-c)*street)`. Movements unchanged. `compliance` is on the traits.
- `run/mod.rs`, `spawn.rs`: field built once in `StreetRun::new`, passed to `World` (5 lines).
- `tests/street_speed.rs`: 4 tests (bollards, narrower lane, careful never over limit, careless on wide street over sign). All pass. They use a 50 km/h sign for (a): with 30 km/h compliant drivers are held to the limit and little shows.
- `tests/street_run.rs`: replay hashes updated on purpose (behaviour changes):
  - 1 Mixed x1: a2c317ad16ca4f12 -> c59d2a872df0c53f
  - 2 Mixed x1: 9cfc202d59740427 -> 0148f27719f690e1
  - 1 Careless x4: ecd8d0bcc5fe3fb8 -> b4ad133ff9b078d1
  - 2 Careless x4: 94af4386c96c681d -> 5f0c0cb15d4cd5f7

## Tests
fmt and clippy (-D warnings, tf_sim) clean. street_run, street_cross, street_space, street_speed pass.
Two EXISTING tests fail. I did not change them:
1. `street_meet::a_hidden_pedestrian_can_be_hit_by_an_inattentive_driver` (seed 2): a contact whose only cause is `Speeding { 53.7 km/h, limit 30 }`; the test wants HiddenWalker / look causes. Careless drivers (speed_factor up to 1.5) now take the street speed (about 39 km/h on that lane, parked strip + painted line) times 1.5. Before, the same test found 5 contacts in 20 seeds, all with the expected causes.
2. `street_walk::pedestrians_spawn_from_the_level_demand_and_do_not_shift_the_vehicles` (seed 1, Careless, x4): one walker (1000016) was hit in that run (1 walker contact) and so never arrived; the test treats it as stuck. Run_level shows 105 of 106 walkers arrived, 1 contact.
Careful populations: zero contacts, all pass.

## Decision needed
- Is a speed-factor above 1 on top of the street speed too fast (53.7 km/h in a 30 zone)? An option: cap the wanted speed of any driver (for example at the street speed * 1.15 or at the sign * 1.5). That is a behaviour choice, so I did not do it.
- The two tests need a rule change (accept a Speeding-only contact in test 1; skip hit walkers in test 2), or the cap above. Your call.

## Figures
- run_level carrefour-ecole seed 1, before -> after (Mixed): hash a2c317ad... -> c59d2a87...; walker mean delay 2.36 -> 2.45 s. Full "before" output is in the scratchpad (before-mixed.txt, before-careless.txt). Careless x4 after: 84 spawned, 45 arrived, 4 contacts, 1 walker contact.
- Lane street speeds on carrefour-ecole: 3.0 m lanes next to parking + paint: 10.8 m/s (39 km/h); 3.0 m lane next to sidewalk + paint: 11.6 m/s (42 km/h); sign 30 km/h. I did not measure mean wanted speed per segment before/after in full.
- Mean speed on segment 2 (x 100..140), 50 km/h sign, Mixed, 4 seeds: bollards lower it (test asserts at least 0.3 m/s drop); narrower lane lowers it. With the 30 sign: 8.84 -> 8.64 m/s with bollards (small, as drivers are held by the sign).

## Update after t13's decision (cap added)
- `speed::STREET_TOLERANCE = 1.2`; `desired_speed` on a road lane is `min(speed_factor * mix, 1.2 * street)`.
- Mixed hashes did not move (cap never binds): 1 Mixed x1 c59d2a872df0c53f, 2 Mixed x1 0148f27719f690e1.
- Careless x4 hashes (old original -> now): seed 1 ecd8d0bcc5fe3fb8 -> ac1abcfab98750b6; seed 2 94af4386c96c681d -> 6cd5807b2a438da1.
- street_walk test changed (as agreed), still strict for everyone else:
  1. `Arrivals::check`: a walker named as victim `b` in a logged `Contact` event counts as accounted for.
  2. The x1 "vehicles not shifted" check: seed 2 Careless x1 now ends with a pedestrian DEATH at 98 s (EndReason::Death), so the run with walkers has 10 spawns, the cars-only run 32. When the run ended by Death, the cars-only list is cut to the same length before comparing; otherwise full equality.
- street_meet `a_hidden_pedestrian_can_be_hit_by_an_inattentive_driver` NOT changed and still FAILS (seed 2): a car-walker contact whose cause chain is only `[AcceptedShortGap { gap_s: 3.06 }]`. No HiddenWalker, DidNotLook, TooFastForSight or DidNotYieldToWalker. That is a cause-detection gap (a walker contact is blamed on a junction-style cause only), not hidden.
- Green: street_run (16), street_cross, street_space, street_speed, street_walk, unit tests; clippy clean. NOT committed because street_meet is red.

## Cause-detection gap: trace and fix
Contact: street_meet hidden zebra (30 km/h, van before the zebra), seed 2, Careless x1, tick 1803, car 9 hits walker 1000027 at 11.2 km/h, no injury. Chain was only `[AcceptedShortGap 3.06 s]` (a walker cause).
Trace of car 9:
- t1610 to 1745: stopped at 54.6 m, 8 m from the zebra, held by walker 1000024 (a person hit earlier by car 8, lying on the zebra until taken away at about t1724).
- t1750: starts again (accel 2.0). t1750 to 1800: creeps 0.7 -> 10 km/h. Not distracted (distracted_until 0). `looks` true, `yields` false (its own draw).
- Walker 1000027 comes from the north kerb, hidden behind the van until the car is near x 56.5. Then it "intends" the zebra (t1780 to 1795) and is on it from t1796.
- The driver decides from a snapshot about 1 s old. In it the walker is 3 m from the lane line, and the driver predicts it will not be in the path when it passes (the walker moves 1.2 m/s). The walker is in the path 1 s later. At the contact the front has just entered the zebra (d 58.6, zebra enter 58.5). Stop distance was no longer enough (3 m/s needs about 0.4 m at max brake, but the walker was 1 m away at the time of the decision lag).
- A careful driver (looks, yields) would have yielded to 1000027 while it waited at the kerb (t1783 to 1795, 1.5 to 2.5 m before the zebra, speed under 9 km/h): it could have stopped. So the true driver cause is `DidNotYieldToWalker`.
Why it was not logged: `CrossingApproach.pending_yield` kept only ONE walker, the first the careful check would yield to (`get_or_insert`). That was the lying walker 1000024, still first in the list until t1796 (and stale after it left). `meeting_causes` keeps only causes about the walker that was hit, so 1000024's cause was dropped and 1000027 never got one.
Fix (logging only, no behaviour change): `WalkerView.yield_all` collects every walker a careful driver yields to; `Decision.crossing_pending` and `CrossingApproach.pending_yield` hold the list (no duplicates); `crossing_causes` makes one cause per walker. Files: yielding.rs, drive.rs, run/mod.rs.
Result: the contact now has `[DidNotYieldToWalker{1000027}, AcceptedShortGap]`. The test was not changed. Replay hashes did not change.
Tests: lib 53, street_cross 11, street_meet 12, street_run 16, street_space 17, street_speed 4, street_walk 9: all pass. clippy -D warnings clean.
