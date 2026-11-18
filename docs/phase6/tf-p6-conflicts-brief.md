# Phase 6 W4b brief: classic conflicts on the street, and track < painted lane < no facility

Branch `p6-conflicts`, worktree `../tf-p6-conflicts` (from `pivot-phase-6` f846516: W1, W2, W3, W4a, W6a and a cleanup are merged; check.sh passed). `CARGO_TARGET_DIR=../tf-target-1`, `CARGO_BUILD_JOBS=3` for every cargo command (shared laptop; another agent builds too). In bash, `source ~/rust-gnu-tools/env.sh` first. Edit only `tf_sim`.

## Read first
- `CLAUDE.md`; `docs/URBANISM_PIVOT_PLAN.md` section 8 "Phase 6 — Cyclists" (classic conflicts, "Done when").
- Reports: `../tf-p6-junction-REPORT.md` (W4a, with the facility numbers and their cause), `../tf-p6-bike-REPORT.md` (W1), `../tf-p6-infra-REPORT.md` (W2: `Separator::crossability`, `crossable_by`, `crossable_between`), `../tf-p6-sense-REPORT2.md` (W3), `../tf-p6-review.md` (review; F3, F5, F6, F7 are NOT in this package).

## Scope
1. **The separator matters.** Today nothing in the run reads `Separator::crossability`, so a painted lane and a kerb track behave the same. Make the run use it for every vehicle that leaves its lane (passing, stopping, turning).
2. **Squeeze past (no facility).** In a shared lane, impatient drivers (by traits: risk, frustration, hurry) pass a rider with too little side room instead of waiting, which makes close passes (W4a logs them) and sometimes contacts. Careful drivers wait or pass with at least 1 m (R414-4).
3. **Vans and cars in a painted lane.** Delivery vans (and some careless cars) stop on a painted bicycle lane; riders must leave the lane into the car lane (conflicts with cars from behind). A car may also drive in a painted lane to pass a queue. Kerb, bollards and planter block both. Spawn delivery stops as level/design data or as a chance per van (say which, start value).
4. **Door of a parked car.** A parked car opens its door into the bicycle lane or the car lane edge (chance per parked car and per tick, start value; source if you find one). Riders who ride in the door zone (W1 rest position is "door zone aware") must brake or swerve; a buffer between parking and the lane removes the risk. If the level has no parked cars, use a test street.
5. **Early right turn across a painted lane.** Some drivers turn right across a painted lane before the corner (cutting it); a kerb track forces the turn at the corner.
6. **Pedestrian against cyclist at a crossing.** Walkers who cross a bicycle lane or track (not only at zebras) and riders who do not yield; conflicts and contacts both ways. W4a's slip rule at zebras stays.
7. **Phase 6 "Done when" test** (in `tests/street_bike_design.rs`, statistical over seeds): on a test street, a protected track gives measurably fewer serious conflicts than a painted lane, and a painted lane fewer than no facility. Close passes count as conflicts (light or serious, as W4a grades them).
8. Open points to look at: a rider who stands for walkers on a zebra is not flagged `held_by_walkers` (W1); the walker/car wait at a zebra (W4a open point).

Every new behaviour must have a micro-scenario test. Name each new constant, with a source or "start value". Do not relax any existing test: if a new rare case appears, `hc send t13` with seed, scenario and cause chain, and wait. New behaviour rules beyond this brief and tuning are the user's call: ask through `hc send t13`. Swerving as a general reaction is package W5, not this one: here a rider only leaves the lane around an obstacle (van, door) when it is free, else it brakes.

## Done when
- Items 1 to 7 work, with tests; item 8 fixed or reported. `cargo test -p tf_sim`, `cargo clippy -p tf_sim --all-targets -- -D warnings`, `cargo fmt` pass; `cargo check -p tf_game` once at the end.
- Replay hashes in `crates/tf_sim/tests/street_run.rs` updated on purpose.
- Commits with clear messages: `git -c user.name=gimk -c user.email=a.pouligny@gmail.com commit`, no Co-Authored-By, no push, no merge.

## Report
Write `../tf-p6-conflicts-REPORT.md` (what changed, new constants, measures with numbers: serious conflicts / close passes / contacts per facility, per conflict type; test premise changes; open points; text for the plan), then `hc send t13 "W4b done: ...; report in <path>"`. Do not start other agents. Write in ASD-STE100 Simplified Technical English.
