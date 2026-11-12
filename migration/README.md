# Migration data

This folder holds what is left of the Godot → Rust + Bevy migration (`docs/BEVY_MIGRATION_PLAN.md`).

- `golden/*.json`: reference data dumped from the Godot build. They are **regression data**: the Rust tests (`crates/tf_sim/tests/*_golden.rs`, the `*_trace.rs` files, `crates/tf_game/tests/playtest_parity.rs`) read them and compare the Rust sim against what Godot produced. Do not edit them by hand.
- `baseline.md`: Godot's behaviour metrics (Town day, Bottleneck, stress grids) with the tolerance bands the port had to meet. `cargo run -p tf_sim --release --bin run_baseline` measures the Rust numbers next to them.

The Godot scripts that produced these files were deleted at the cutover (M12.1). They exist only in the git tag `godot-final` (`git checkout godot-final`; Godot 4.7.2 is needed to run them):

| Script (in `godot-final`) | Produced |
|---|---|
| `migration/export_golden.gd` | `golden/{bottleneck,bottleneck_bypass,bottleneck_bus_lanes,town,junctions,grid,edits}.json`: road, lane, connector, conflict-zone, plot, walk network, transit and route dumps of fixed layouts |
| `migration/run_baseline.gd` | `baseline.md` |
| `migration/probes/traffic_trace.gd` | `golden/traffic_trace.json` |
| `migration/probes/bus_trace.gd` | `golden/bus_trace.json` |
| `migration/probes/citizen_trace.gd` | `golden/citizen_trace.json` |
| `migration/probes/town_trace.gd` | `golden/town_trace.json` |
| `migration/probes/playtest_trace.gd` (+ `.tscn`) | `golden/playtest_trace.json` |
| `migration/probes/geom_probe.gd` | `crates/tf_sim/src/geom/godot_geometry2d.json` (`Geometry2D` results) |
| `migration/probes/rng_probe.gd` | the RNG values pinned in the `tf_sim` RNG tests |
| `migration/probes/town_benchmark.gd` | the GDScript timing quoted in the plan (counterpart of the `town_benchmark` binary) |

Since Godot is gone, the goldens can only be regenerated from that tag. A change to the sim that breaks them must be deliberate, and the goldens updated knowingly.
