# Porting guide: GDScript → Rust

Read this before porting any sim class. It shows the conventions on a real example, `sim/sim_clock.gd` → `crates/tf_sim/src/clock.rs` (with `tests/unit/test_sim_clock.gd` → `crates/tf_sim/tests/sim_clock.rs`). Copy its shape. The plan's §2 and §4 and `docs/migration/NOTES.md` hold the rest of the rules.

## 1. One GDScript class → one Rust module

| GDScript | Rust |
|---|---|
| `sim/<folder>/<name>.gd`, `class_name Name` | `crates/tf_sim/src/<folder>/<name>.rs`, `pub struct Name`, re-exported from `src/<folder>.rs` |
| `## class doc` under `extends` | `//!` module doc: what the class is for (*why*), and its GDScript path in backticks |
| `## member doc` | `///` on the field or function, same wording |
| `const NAME := value` | `pub const NAME: Type = value;` inside `impl Name` (same name, same value). Enums and lookup tables become a Rust `enum` with methods (`PERIOD_NAMES` → `Period::name`) |
| `enum Period { A, B }` | `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub enum Period { A, B }` in CamelCase. Keep the integer values (`#[repr(i32)]`, `= n`) when they're saved or used as indices |
| `func get_day() -> int` | `pub fn day(&self) -> i32` (drop `get_`; keep other names as they are) |
| `func _private()` | `fn private()` (no underscore, no `pub`) |
| `static func f()` | an associated function, or a method on the type it's about (`period_for_hour` → `Period::for_hour`) |
| `_init(...)` | `pub fn new(...) -> Self`, or `Default` when there are no arguments |

Keep the order of functions and fields as in the GDScript. A reviewer reads both side by side.

## 2. Types

| GDScript | Rust |
|---|---|
| `float` variable | `f64` |
| `Vector2` | `glam::Vec2` (`f32`, like Godot) |
| `Vector2i` | `glam::IVec2` |
| `int` | `i32` for game values and ids (wrapped, see below), `i64` for `randi()` results, `usize` for indices and counts |
| ids (`int`, `-1` for none) | the newtypes in `ids.rs` (`LaneId`, `VehicleId`, ...), and `Option<LaneId>` instead of `-1` |
| `Dictionary[int, X]` keyed by id | `IndexMap<XId, X>`, removing with `shift_remove`, **never** `swap_remove` (Godot keeps insertion order, and results depend on it) |
| `Dictionary` used as a set | `IndexSet` |
| `Dictionary` used as a record (`{"lane_id": .., "distance": ..}`) | a small struct with named fields |
| `Array[X]` | `Vec<X>`; `PackedVector2Array` → `Vec<Vec2>`; `PackedFloat32Array` → `Vec<f32>` |
| object references (`Lane`, `Vehicle`) | the id, looked up in the owning system's map. No `Rc`, no `RefCell`, no lifetimes in structs |
| `Curve2D` | `geom::Polyline` (all `f32`, like Godot's) |
| `Geometry2D.*` | `geom::segment::*` |
| `RandomNumberGenerator` | `rng::SimRng::new(seed)` with the **same seed constant**: it reproduces Godot's numbers exactly |
| `INF` | `f64::INFINITY` |

Mixed arithmetic: where GDScript multiplies a `Vector2` by a `float`, Godot computes in `f32`. Write `vec * (x as f32)`.

GDScript built-ins that differ from Rust's:
- `int(x)` truncates toward zero: `x as i32`.
- `a % b` on ints truncates like Rust's `%`. `fmod` is Rust's `%` on floats. `fposmod` is `rem_euclid`.
- Integer `/` truncates, as in Rust.
- `snappedf`, `clampf`, `lerpf`, `move_toward`: write the same formula; don't swap in a library function with different edge cases.
- `Array.sort_custom` isn't stable, while `sort_by` is. Keep the same comparator. If a test depends on tie order, log it in `NOTES.md`.
- `for x in dict.values()` while the loop erases from `dict`: Godot iterates a snapshot in some cases and not others. Read carefully and collect the ids first (`let ids: Vec<_> = map.keys().copied().collect();`) when the GDScript erases inside the loop.

## 3. Signals

A system never calls another system. Where the GDScript emits a signal, the Rust method takes `out: &mut Outbox` and pushes a `SimEvent`:

```rust
// GDScript: day_started.emit(absolute_hour / 24 + 1)
out.push(SimEvent::DayStarted(absolute_hour / 24 + 1));
```

Where the GDScript connects to another system's signal, the Rust system gets an `on_*` method (it's already stubbed with the right name) and `Simulation::dispatch` calls it. Don't change the dispatch order. If you add an emit site, add a row for it to the "Event dispatch" table in `NOTES.md`, saying where it's delivered and whether that matches Godot.

When a method needs another system's data (`VehicleSystem` reads roads, lots and lights), pass it as a parameter. When it needs several, use a context struct built by `Simulation` from split borrows:

```rust
pub struct TrafficCtx<'a> {
    pub roads: &'a RoadNetwork,
    pub router: &'a mut Router,
    pub lots: &'a LotSystem,
    pub signals: &'a TrafficLights,
    pub crosswalks: &'a mut Crosswalks,
}
```

## 4. Tests

- One Rust file per GUT file, under `crates/tf_sim/tests/`, named without `test_` (`test_sim_clock.gd` → `sim_clock.rs`), starting with `//! Port of tests/unit/test_sim_clock.gd.`
- One `#[test] fn` per GUT test, same name without `test_` (`test_day_rolls_over_at_midnight` → `day_rolls_over_at_midnight`), same steps, same assertions, same order.
- `before_each` becomes a helper that builds the fixture, and `after_each` / `dispose()` disappear.
- `watch_signals` + `assert_signal_emit_count` / `assert_signal_emitted_with_parameters`: collect the `Outbox`, or `sim.take_view_events()`, and filter it (see `sim_clock.rs`).
- Float comparisons: `approx::assert_relative_eq!` or an explicit tolerance. GUT's `assert_almost_eq(a, b, e)` keeps its `e`.
- Unit tests for a module's own helpers can sit in a `#[cfg(test)] mod tests` at the bottom of the module (see `geom/segment.rs`).
- Values copied from Godot output go in a JSON file next to the test (`geom/godot_geometry2d.json`), together with the script that printed them in `migration/`.

## 5. Before you commit

```bash
scripts/check.sh
```

Then tick your work package in the plan, add its notes, and update the status line.
