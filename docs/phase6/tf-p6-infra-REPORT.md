# P6-W2 report: bicycle design

Branch `p6-infra`. Only `tf_sim` was edited. New tests are in `crates/tf_sim/tests/street_bicycle.rs` (a new file, not `street_design.rs`, so other agents do not clash with it).

## API for W1 / W4 / W6

New design commands (`design.rs`, all in the design JSON):
- `SetSeparator { segment, boundary, separator }`: sets the separator on the line between strips `boundary-1` and `boundary` (both sides). Building lines are refused.
- `AddBicycleBox { node, segment, depth }` / `RemoveBicycleBox { node, segment }`.
- `SetBicycleLead { node, lead_s }`: leading bicycle green. Needs a signal plan from a template.

Changed commands (`SetStripUse`, `SetStripDirection`, `MoveBoundary`, `SetSpeedLimit`, `SetSeparator`) now check the bicycle rules on the result. A refused command changes nothing.
- Lane at least `MIN_BIKE_LANE_WIDTH` = 1.5 m, track at least `MIN_BIKE_TRACK_WIDTH` = 2.0 m (start values). A strip that was already too narrow is allowed if the edit does not make it narrower.
- Cyclists keep to the right of their own direction: a bicycle strip lies on the right of the car/bus lanes that run the other way. This covers two-way streets and the contraflow lane (left of a one-way carriageway).
- Against the traffic of a one-way street: limit 30 km/h or less (`MAX_CONTRAFLOW_LIMIT_KMH`). Source: French "double-sens cyclable" practice (decree 2008-754). I did NOT check it against the text.
- A track next to a car/bus lane needs a kerb, bollards or planter.
- `SetStripUse` to a lane/track sets default separators next to car lanes: paint for a lane (also next to parking), kerb for a track. Existing ones stay. Locked neighbours are left alone.

Queries on `Street` (model.rs):
- `boundary_crossability(segment, boundary, mode) -> Crossability {Free, Paint, Blocked}`; `crossable_by(segment, boundary, mode) -> bool` (true for Free and Paint); `crossable_between(segment, from_strip, to_strip, mode)` (also checks buffer strips: planting, median, sidewalk, terrace block wheels). Rules are in `Separator::crossability` (space.rs): paint = vehicles may cross against the rule (`Paint` is the flag for careless drivers), kerb blocks all vehicles incl. bicycles, bollards block motor only, planter blocks all. Building lines are Blocked.
- `bicycle_box(node, segment)`, `Street::bicycle_boxes`.
- `car_stop_distance(lane_id)` = lane length minus box depth (motor lanes of that approach); `bicycle_stop_distance(lane_id)` = lane length.

**Important for W4:** there is no `stop_line()` in the street model. The car stop line is `Approach::line_d = route.offsets[j]` in `run/mod.rs` (about line 543), i.e. the end of the road lane. I did NOT change the driving code. For the box to work, W4 must use `street.car_stop_distance(lane)` there, i.e. `line_d` = offsets[j] minus the box depth. With no box it is the same value, so hashes stay the same. Cyclists (W1) stop at `bicycle_stop_distance`. The box can overlap a zebra just outside the junction (not handled).

Signals: `SignalPlan.bicycle_lead_s` (serde default 0). `plan.light()` is for cars and starts `lead` seconds after the phase start; `plan.bicycle_light()` is green from the phase start. With lead 0 both are equal to the old behaviour (tested). The bicycle lane of an approach is in the same group as the cars of that approach; no separate group list. `until_conflict` / `walk` are unchanged (use the phase start: conservative for walkers). Limits: 0 to `MAX_BICYCLE_LEAD_S` = 6 s and below every green. `DEFAULT_BICYCLE_LEAD_S` = 4 s is a value for a UI.
Box rules: depth 3 to 5 m; junction must have signals (and the approach a green in the plan); approach needs a bicycle-capable and a motor lane; signals cannot be removed while a box stands.

## Shared files touched
- `model.rs`, `signals.rs`, `space.rs`, `design.rs`: as above.
- `run/mod.rs`: ONE line, `bicycle_lead_s: 0.0,` in the default `SignalPlan` literal (the new field). Nothing else.
- `tf_game` was not edited; its `done_text` has a wildcard so the new commands compile there (not built).

## Level 1 (Rue du Marché)
Checked, data is right, no change. Segment 3 (north to junction): bicycle lane strip 1 is Backward, so it runs north, from node 0 to node 3, on the east side (x = 77.75); cars go south. Segment 4's bicycle lane arrives from the south and continues through the junction straight into segment 3's lane (a movement with bicycle access exists). Test: `the_contraflow_lane_of_the_market_street_...`.

## Text for the plan
Section 3.1 add: separators decide crossing: paint = crossable against the rule, kerb = no vehicle, bollards = no motor vehicle, planter = nobody; strips: min widths 1.5 / 2.0 m; bicycle strips keep right of own direction; contraflow only on one-way streets at 30 km/h or less.
Section 4 add: bicycle box = 3-5 m on a signalled approach, car stop line moves back; leading bicycle green 0-6 s as a plan option.

## Tests
14 new tests in `street_bicycle.rs` (apply, refusal with reason and no change, JSON round-trip, undo, crossing rules, box stop line, lead, replay) plus unit tests in `space.rs` and `signals.rs`. Results: see the final message (tf_sim full run, clippy). Replay hashes were not edited.

## Final results (2026-11-18)
- cargo test -p tf_sim: all pass (replay hashes unchanged, not edited).
- cargo clippy -p tf_sim --all-targets -D warnings: clean. cargo fmt: done.
- cargo check -p tf_game: passes.
- Open points: W4 must use Street::car_stop_distance for the car stop line (run/mod.rs, Approach::line_d). W1 must use bicycle_stop_distance (replace TODO(W2 merge)). Contraflow 30 km/h limit not checked against the decree text. Box can overlap a zebra just outside the junction.
