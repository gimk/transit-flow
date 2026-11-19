# Brief: W4b fixes (contacts, squeeze against oncoming traffic, contacts in the "done when" test)

Branch `p6-conflicts`, worktree `C:\Users\AntoinePOULIGNY\Documents\code-project\tf-p6-conflicts`. Read `CLAUDE.md` (architecture rules: determinism, no `sin`/`cos`/`exp`/`powf`, `IndexMap`, tests), `docs/phase6/tf-p6-conflicts-brief.md`, and the review `docs/phase6/tf-p6-vans-REVIEW.md` (the cause chains and the proposed fixes). The W4b report is in the main repo: `C:\Users\AntoinePOULIGNY\Documents\code-project\transit-flow\docs\phase6\tf-p6-conflicts-REPORT.md`.

Scope: `crates/tf_sim` only. Do not touch `tf_game`.

## Work, in this order (user decisions of 2026-11-19)

1. **Fix A** (`bike.rs`, `lateral_limit`): compare against the other bodies moved forward by one reaction time, as `hold_if_blocked` and `beside_anyone` do (see review). Test: `a_van_that_pulls_over_does_not_sweep_into_a_rider_it_overtakes` in `tests/street_bike_design.rs` (review, "Tests", item 1).
2. **Fix B** (`spawn.rs`, `try_spawn`): a vehicle does not enter at a gate while a slower body from the same gate, in another lane of the same segment, is within a stopping gap ahead of the entry (review, "Fix B"). Unit test that `try_spawn` is blocked in that case, and the review's flow test (no contact in 24 seeds).
3. **Squeeze, option (b):** a driver who squeezes past a rider (an impatient driver, the W4b squeeze rule) may also accept a tight gap to the oncoming traffic: today the corridor check stops it, so "no facility" looks safer than it is. The careful driver keeps the full rule. The tight gap to oncoming traffic must come from the driver's traits (the same traits that let it squeeze), not from a new random draw outside the agent's `rng`. If a tight pass against an oncoming vehicle is close, it must show up as a conflict (close pass or PET), as in reality. Not option (c): delay is annoyance (Phase 8), not a safety conflict. Add a micro-scenario in `tests/street_conflicts.rs`: shared lane, oncoming traffic, impatient population, at least one squeeze that happens with oncoming traffic; careful population: none.
4. **"Done when" test** (`a_track_gives_fewer_serious_conflicts_than_a_lane_and_a_lane_fewer_than_none`): remove the `lane.contacts > 0` assertion and its comment about standing vans. Also check contacts: track <= painted lane <= no facility (the track must stay 0). If after fixes A, B and the squeeze the contacts do not follow that order, do not tune constants to force it: stop, find the cause chain (seed, tick) and report it.
5. **Measure** the "done when" table again (24 seeds: serious, close passes, contacts per facility) and the W4a numbers (the same test file prints them).

## Rules

- Each fix changes behaviour on purpose: the 4 replay hashes in `tests/street_run.rs` will move. Update them once at the end, with a comment that names the fixes.
- Build: in Bash, `source ~/rust-gnu-tools/env.sh` first. `CARGO_TARGET_DIR=C:/Users/AntoinePOULIGNY/Documents/code-project/tf-target-1`, `CARGO_BUILD_JOBS=6`. `cargo check -p tf_sim` while coding; run only the affected tests; before each commit `cargo fmt`, `cargo clippy -p tf_sim --all-targets -- -D warnings`, `cargo test -p tf_sim` (use `--release` for long measures if useful).
- Commits: on `p6-conflicts`, with `git -c user.name=gimk -c user.email=a.pouligny@gmail.com commit`. One commit per item is fine. **No `Co-Authored-By` line, no "Generated with" line.** Do not push. Do not delete `docs/phase6/tf-p6-vans-*.md` or this brief: commit them with your first commit.
- No new constant without a `///` that says the reason and the source (real value or "start value"). Do not retune existing constants.
- No subagents.
- At the end, write `docs/phase6/tf-p6-conflicts-fix-REPORT.md` (ASD-STE100, short): what changed, the tests, the new tables, open points, and a "Text for the plan" paragraph that replaces section 9 of the W4b report. Commit it. Then report with `hc send t13 "<one line + file path>"`. Also report when stuck.
