# W6 part 1 report: views and tools for the bicycle design

Branch `p6-views`. Only `tf_game` and `CLAUDE.md` changed. `tf_sim` is not touched.

## Keys (free keys; the user decides the final ones)
- `X` separator: click a strip boundary. It cycles none, paint, kerb, bollards, planter.
- `N` bicycle box: click an approach. No box there: add one (4 m). A box there: remove it.
- `G` leading bicycle green: click a junction to switch it on (4 s) or off. Wheel or `+` / `-` change it by 1 s (0 to 6 s).
- Same as the old tools: the key again or Esc drops the tool. Backspace / Ctrl+Z undo, Ctrl+Y redo.
- A refused command shows its reason ("Not possible here: ..."). The ghost under the pointer shows the same reason before the click.

## Files
- `views/street.rs`: `bicycle_box_shapes` (green area on each motor lane of the approach, plus car stop line at the back of the box). Drawn in the surface and marking layers.
- `views/palette.rs`: `BIKE_BOX` colour.
- `play/pick.rs`: `boundary_at`, `next_separator`, `separator_command`, `approach_at`, `box_command`, `lead_click_command`, `lead_step_command`. `ApproachMark` has a new field `bicycle_lead_s`.
- `play/tools.rs`: 3 new `PlayTool` variants (labels, hints, `done_text`).
- `play/preview.rs`: ghosts for the 3 tools.
- `play/signals.rs`: a second small light under the car light at each approach when the lead is above 0. It is static: the run snapshot has only car lights.
- `play/mod.rs`: wheel / `+` / `-` also work for the lead tool; camera wheel lock too.
- `input.rs`: 3 new actions and keys.
- `CLAUDE.md`: key list updated.
- `tests/app/bicycle_tools.rs` (7 tests, listed in `main.rs`): keys, separator apply / undo / redo / no-boundary, separator refusal (planter to none on a track), box add / remove / undo / redo and the view layers, box refusal without signals (also the ghost note), lead apply / step / clamp / undo, lead refusal without a plan.

## Results
- `cargo fmt`, `cargo clippy -p tf_game --all-targets -- -D warnings`: clean.
- `cargo test -p tf_game`: all pass (46 unit, 54 app).

## Screenshot
Level 1, signals, boxes on segments 1 and 2, lead 4 s, planter / bollards / kerb separators. It showed the boxes with stop line, the separators and the bicycle lights. The PNG was in the scratchpad and is deleted, as the brief says.

## Open points
- The box can overlap the zebra just outside the junction (known W2 point). It does on level 1.
- Boxes can only go on approaches with a bicycle-capable lane; on level 1 a car lane counts, so segments 1 and 2 work. Segments 3 and 4 were refused in my first try (reason was shown).
- The box tool picks the approach by the nearer end of the clicked segment. Inside the junction it picks the nearest arm.
- Bicycle light does not change colour in a run (needs the cyclist work, W1, and the snapshot).
- Cyclists are not drawn.
