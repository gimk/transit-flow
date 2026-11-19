# W6b code review

- Branch: `p6-views2`
- Commit: `05914e8809dfdcf66a7caed54fae394952be67c7` (`git show HEAD`)
- Scope: `tf_game` only (read-only review, no build or test run)
- Findings: 0

## Summary

No defects were found in this commit.

## Checks

1. **Rider position and interpolation (`crates/tf_game/src/views/agents.rs`, `crates/tf_game/src/street_runner.rs`)**
   - `AgentPose::pos` comes from `Agent::pos`. In `crates/tf_sim/src/street/run/mod.rs`, `spawn::pose` already adds `agent.lateral` to `agent.pos`.
   - `draw_agents` interpolates `before.pos.lerp(pose.pos, alpha)` and `lerp_angle(before.dir.to_angle(), pose.dir.to_angle(), alpha)`. It does not add `lateral` a second time.
   - `draw_agents` draws the bicycle body, the dark rider dot, and the light front mark in the same `Batch` mesh.

2. **Bicycle light colour logic (`crates/tf_game/src/play/signals.rs`, `crates/tf_game/src/street_runner.rs`)**
   - `approach_lights` fills `ApproachLight::color` from `SignalPlan::light` and `ApproachLight::bicycle` from `SignalPlan::bicycle_light`.
   - In `crates/tf_sim/src/street/signals.rs`, `bicycle_light` uses `lead_s = 0.0`, while `light` uses `self.bicycle_lead_s`.
   - During a run, `update_signal_marks` reads `view_to().lights` and passes `Lit { car, bicycle }` to `build_marks`. `bike_color` maps `lit.bicycle` with `color_of`.
   - Outside a run, `light` is `None` and `bike_color(None)` returns `BIKE_LIGHT_COLOR`.

3. **Report mapping (`crates/tf_game/src/play/ui.rs`)**
   - `cyclists_text` maps all fields of `CyclistSummary` (`crates/tf_sim/src/street/run/mod.rs`) to the matching labels in the `Riders:` block.
   - `report_tail_text` shows the `Riders:` block only when `s.cyclists.spawned > 0`.
   - Known and accepted: HUD text `(1 riders)` in `run_text` has a small plural error.

4. **Inspector card (`crates/tf_game/src/play/inspect.rs`)**
   - `car_card` shows `Rider #<id>` for `Mode::Bicycle`, prints `bike.edge_m`, `pose.lateral`, and the rule draws (`red_u`, `sidewalk_u`, `filter_u`).
   - `car_state` returns `"riding"` for a moving, non-waiting, non-distracted, non-crashed rider.

5. **Architecture rules (`CLAUDE.md`) and dead code**
   - All views and UI modules read only `StreetSnapshot`, `StreetRunner`, `LoadedLevel`, and `ReportData`. They do not read `StreetRun`.
   - Moving riders use `views::batch::Batch` in `AgentViews` at `z::AGENTS`. Signal marks use `ShapeBuilder` at `z::SIGNALS`.
   - All new structs, fields, constants, and functions are used.

6. **Tests (`crates/tf_game/tests/app/riders.rs` and unit tests)**
   - The 6 tests in `riders.rs` and the new unit tests in `agents.rs`, `signals.rs`, `inspect.rs`, and `ui.rs` check what their names and comments claim.
   - Note: `the_bicycle_light_leads_the_cars_light_in_a_run` checks `StreetSnapshot::lights` directly, while the `build_marks` and `bike_color` view logic is covered by the unit test in `play/signals.rs`.
