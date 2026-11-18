# Phase 6 W6b brief: draw the cyclists and the bicycle lights

Branch `p6-views2`, worktree `../tf-p6-views2` (from `pivot-phase-6` f846516: W1 cyclists, W2 bicycle design, W3 perception, W4a junction conflicts, W6a tools are merged). `CARGO_TARGET_DIR=../tf-target-2`, `CARGO_BUILD_JOBS=3` for every cargo command (shared laptop; another agent builds too). In bash, `source ~/rust-gnu-tools/env.sh` first.

## Read first
- `CLAUDE.md` (views only read snapshots; `draw::ShapeBuilder`; `views::batch` for moving things; `views::z`; tests in `crates/tf_game/tests/app/`; screenshot mode).
- `../tf-p6-bike-REPORT.md` (riders: `Agent::lateral`, `bike`, `Mode::Bicycle`), `../tf-p6-views-REPORT.md` (W6a: what is drawn already, the static bicycle light), `../tf-p6-junction-REPORT.md` (close passes, `CyclistSummary`).
- `crates/tf_game/src/street_runner.rs` (`StreetSnapshot`, `AgentPose`), `crates/tf_game/src/views/agents.rs`, `views/walkers.rs`, `play/signals.rs`, `play/` report and incident UI.

## Scope (tf_game only. In tf_sim, only read access: if you need a new public getter there, ask t13 first)
1. Draw riders as their own shape (a narrow body with a rider, about 1.8 m x 0.6 m), at their real side position (`lateral`), in the batched agent mesh or a batch of their own. Interpolate like cars. Colour by archetype like cars, but clearly different from cars and walkers.
2. The bicycle light in a run: use `SignalPlan::bicycle_light()` so the small bicycle light of W6a changes colour during the run (leading green visible). Add what is needed to `StreetSnapshot` / `ApproachLight` in `tf_game`.
3. Report and incidents: riders appear in the incident list and the report like the other modes (contacts, conflicts, close passes from `CyclistSummary`). Keep it simple and in the existing style.
4. Agent inspect (if play mode has one): show that an agent is a rider and its habits.

## Done when
- App tests (one module per file, listed in `main.rs`): riders are in the snapshot and drawn; the bicycle light changes; riders appear in the report.
- Screenshots: level 1 in a run with riders (`--play carrefour-ecole --run --frames N`) and one with signals, a box and a leading green. PNGs in a scratch dir outside the repo; look at them with the Read tool; keep the paths in your report; do not commit them.
- `cargo fmt`, `cargo clippy -p tf_game --all-targets -- -D warnings`, `cargo test -p tf_game` (full, once at the end) pass.
- One commit: `git -c user.name=gimk -c user.email=a.pouligny@gmail.com commit`, no Co-Authored-By, no push, no merge.

## Report
Write `../tf-p6-views2-REPORT.md` (what is drawn, files, tests, screenshot paths, open points), then `hc send t13 "W6b done: ...; report in <path>"`. Stuck or a design question: `hc send t13` and wait. Do not start other agents. Write in ASD-STE100 Simplified Technical English.
