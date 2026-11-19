# Brief: review of the rider–van contacts (W4b, read-only)

Branch `p6-conflicts`, worktree `C:\Users\AntoinePOULIGNY\Documents\code-project\tf-p6-conflicts`. Read `CLAUDE.md`, then `docs/phase6/tf-p6-conflicts-brief.md`. The W4b report is in the main repo: `C:\Users\AntoinePOULIGNY\Documents\code-project\transit-flow\docs\phase6\tf-p6-conflicts-REPORT.md` (sections 5, 6, 8).

## Problem

The "done when" test `a_track_gives_fewer_serious_conflicts_than_a_lane_and_a_lane_fewer_than_none` (`crates/tf_sim/tests/street_bike_design.rs`, helpers `facility_level`, `measure_facility`) gives 7 contacts with a painted lane, over 24 seeds. The 7 contacts are riders against vans that stand still in the painted lane for `STOP_DURATION_S` (20 s). A rider must see a van that stands still and brake or go round it (`leave_lane`). Thus this looks like a defect.

## Task (read-only: do not edit source or tests, do not commit)

1. Find the seeds and ticks of the 7 contacts. You may write a throwaway test or example in your scratch dir, or a temporary `#[ignore]` test that you delete afterwards; leave the tree clean (`git status` empty except this brief).
2. For at least one contact, give the full cause chain: what the rider perceives each tick before the contact (`perceive.rs`, `bike.rs`, `cross.rs`, `drive.rs`), why it does not brake or leave the lane, and where the van is (is it a snapshot / leader / lane-id problem, a perception miss, a body-length or side-position error, the van moving into the lane after the rider committed, a contact measure error...).
3. Check if all 7 contacts share the cause.
4. Propose the smallest fix (file, function, what to change) and the test that must prove it. Do not implement it.

## Rules

- Build: in Bash, `source ~/rust-gnu-tools/env.sh` first. Use `CARGO_TARGET_DIR=C:/Users/AntoinePOULIGNY/Documents/code-project/tf-target-1` and `CARGO_BUILD_JOBS=6`. Build `tf_sim` only (never `tf_game`). Use `--release` for the 24-seed measure if debug is slow.
- No subagents.
- Write the result to `docs/phase6/tf-p6-vans-REVIEW.md` in the worktree (ASD-STE100 Simplified Technical English, short), then report with `hc send t13 "<one line + file path>"` when done or stuck.
