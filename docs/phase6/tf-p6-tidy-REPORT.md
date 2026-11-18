# Report: Phase 6 cleanup (branch p6-tidy)

No behaviour change. Replay hashes are unchanged (see the test result in the commit step).

## Done
- F10: `Looker::for_agent(agent, eye, dir)` in `perceive.rs`. It replaces the literal in `drive.rs` (`notices_other`), `conflicts.rs` and `meet.rs`. The walker literals in `kerb.rs` and `yielding.rs` are other code (walkers, `alternate`) and stay.
- F10: `StreetRun::decided_from(&self, agent) -> &Snapshot` in `mod.rs`. It replaces the three copies (`meet.rs` x2, `mod.rs`).
- F11: removed the unused fourth tuple element of `found` in `meet.rs`.
- F11: the band keys in `meet.rs` are now each visited once (bands of the last tick first, then the new keys: same order as before).
- F11: wrapped the `meet.rs` module doc. It now says that riders are in the contact and PET code.
- F11: `SPEEDING_FACTOR` and `BIKE_LIMIT_TOLERANCE` are now one value (`BIKE_LIMIT_TOLERANCE = conflicts::SPEEDING_FACTOR`), with a doc on both.
- F11: added `///` docs on `World`, `critical_gap`, `MoveInfo::node`, `MoveInfo::from_segment`, `Block::{Red, Amber, StopSign, ExitFull}`, `Decision::{accel, stopped_at_sign}`. (`go` and `committed` already had docs.)
- F11: source notes on `BIKE_HEADWAY_FACTOR`, `COMFORT_BRAKE` (with the rider remark of F6), `MEET_REACH`.
- F11: `ZEBRA_LOOK_M` moved up to the other constants.
- F9: `cross_traffic_empty` is now computed only when the rider is not `careful` (it is pure).

## Dropped (in the zone of W4a)
- F11: one pair of constants for `QUEUE_*`, `QUEUE_CRAWL`, `RING_REACH + 2.0`. Source notes for `QUEUE_CRAWL`, `QUEUE_REACH`, `RING_REACH`. These constants are in the do-not-touch list.
- F12 (all): in the zone, as the brief says.
- F9 other items (ungate, amber rule, one way to ask "is a bicycle"): behaviour change or in the zone.
- F10 duplicates inside `drive.rs` (margin block, notice gate, `crossing_times`): in `junction_blocks` and `box_blocks`, in the zone.
- F10: `notices_other` memory read from `world.peds`: needs a design decision.
