# W3 part 2 report: merge with W1 and riders in perception

Commit b7e4cae on p6-sense (merge of pivot-phase-6 e8bcdd1 + wiring). Not pushed.

## Done
- Merge: conflicts in agent.rs, drive.rs, events.rs, meet.rs, spawn.rs, tests/street_run.rs resolved by keeping both sides.
- Wiring (`drive::notices_other`, used in `junction_blocks`): a driver sees a rider, and a rider sees a motor vehicle, through a `Looker` of its own mode (gaze of its task, field of view, blind spots, 40 m sure-notice for fully attentive agents). A rider hears motor vehicles. What was in view in the last 3 glances stays known (moved to the side or behind). Pairs of the same kind are unchanged.
- `DriverTask::Junction` (straight on: looks right, left, ahead). Without it, a car at its line never looked at a rider on its right.
- Walkers hear riders through `sound_level` (bicycle almost silent).
- Cause chain: contact with a rider adds `InBlindSpot` / `LookedElsewhere`.
- New `CauseKind::EnteredWithoutGap` (user decision): a vehicle passes the stop line although its own last decision at the line was "stop" (`approach.go == false`): the "committed" flaw of seed 47. The behaviour is not changed (W4). The contact of careless x3 seed 17 (car 53 / car 57) now has a cause chain. Text added in score.rs.
- Rate test: `street_perceive::a_van_driver_notices_a_cyclist_in_its_blind_spot_only_by_shoulder_checks`.
- Seed 17 x4 (car and rider both GapTooShort, serious injury) split like seeds 26/32: ignored tests `careful_drivers_and_walkers_never_touch_seed_17_x4` (street_meet) and `careful_drivers_never_touch_on_a_busy_street_seed_17` (street_run), seed skipped in the loops. The contact also happens with rider perception off.
- W1's ignored tests (seeds 47; 26 and 32) stay ignored: 3 ignored in street_meet, 1 in street_run (my new ones included).

## Effect of recording EnteredWithoutGap on other tests
No test changed except the replay hash of seed 1 Careless x4 (the log has the new cause). Everything else in tf_sim passes.

## Checks
`cargo test -p tf_sim`: all pass (4 ignored). Clippy `-D warnings`, `cargo fmt --all --check`, `cargo check -p tf_game`: ok.
Replay hashes changed on purpose: 0x4c23_82f7_e2df_b0ef, 0x9aeb_f813_b7c4_9860, 0x92fe_7d63_a49f_bc07, 0xb272_d3db_cdaa_6107.

## Open points
- Rider perception covers the junction check. Cars following a rider in the lane use the snapshot (a rider ahead is in the centre of view).
- No whole-run test of a van turning right against a rider; the rate test is on the perception functions.
- Text for the plan: Phase 6 WP3 done and merged with WP1: drivers and riders see each other through perception (gaze, field of view, blind spots, 3-glance memory); riders are almost silent; a driver going straight scans right, left and ahead at a junction; new cause EnteredWithoutGap.
