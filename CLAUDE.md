# Transit Flow

2D top-down street design game in **Rust + Bevy**. The player is an urban designer in Paris: each level is one real intersection with fixed buildings and kerb-to-kerb width; the player shares the space between cars, cyclists and pedestrians (strip use, rules, signals, crossings, obstacles, green areas), then starts the run and only watches. Annoyance, conflicts and injuries cost points; a death is a game over; free space, rest and green areas give a bonus. **Realism first, fun later**: it is a simulation before it is a game. Each star of a level is a load stage (more cars, pedestrians, cyclists on the same design, until it breaks).

- **Roadmap, design and phase status: [docs/URBANISM_PIVOT_PLAN.md](docs/URBANISM_PIVOT_PLAN.md)** (approved 2026-11-16). Read it before starting a phase, and update its **Status** line and phase details when a phase is done or the design changes.
- **The old city builder is gone** (pivot Phase 5). Its last commit is the git tag `city-builder-final` (local only); its roadmap [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) and [game_design_architecture_concept.md](game_design_architecture_concept.md) are kept as the record.
- History: the game was written in Godot 4.7 / GDScript and ported to Rust + Bevy ([docs/BEVY_MIGRATION_PLAN.md](docs/BEVY_MIGRATION_PLAN.md), done; decisions in [docs/migration/NOTES.md](docs/migration/NOTES.md), the historical log). The Godot project's last commit is the git tag `godot-final` (local only). The Godot golden files were retired with the old code in Phase 5.

## Commands

Cargo lives in `~/.cargo/bin`; `~/.zshrc` loads it. In a shell that doesn't, `source "$HOME/.cargo/env"` first (on the Windows laptop: `source ~/rust-gnu-tools/env.sh`).

- `cargo play`: starts the first level (`carrefour-ecole`). `cargo play -- --play <level-id>` opens a level: Design, Run, Report. Keys: C zebra, V speed limit, B bollards (drag along a kerb), L signals (wheel or +/- for green time), X separator (click a strip boundary: none, paint, kerb, bollards, planter), N bicycle box (click an approach), G leading bicycle green (click a junction; wheel or +/- for seconds), Esc no tool, Backspace / Ctrl+Z undo, Ctrl+Y redo, Enter run / stop / continue, `[` `]` or PageUp/PageDown choose a stage, Space pause, 1-4 speed x1-x8, WASD / arrows pan, wheel zooms, F11 fullscreen. Design and stars are saved in `<data dir>/designs/` (`TRANSIT_FLOW_DATA_DIR` overrides the data dir). Code: `crates/tf_game/src/play/`. `cargo play` is the alias in `.cargo/config.toml` for `cargo run -p tf_game --features dev`, which links Bevy dynamically for fast rebuilds. Arguments go after `--`. Release: `cargo run -p tf_game --release`.
- `scripts/check.sh`: `cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace`. **Run it before every commit**; it must exit 0. Run `cargo fmt` first if it complains about formatting.
- **Screenshot mode (visual checks):** `cargo play -- (--play <id> | --level <id>) --frames N --screenshot out.png [--zoom Z] [--run [--seed N] [--population careful|mixed|careless]]` runs N frames at a fixed 1/60 s step, writes a 1280x720 PNG of the window and exits 0 (bad args: usage, exit 2). `--play` shows play mode (with `--run`, Enter is pressed first); `--level` shows the level framed on its bounds (`--seed` and `--population` only with `--level --run`). A real window opens briefly. Put PNGs in a scratch dir, never commit them; look at them with the Read tool. Screenshots come out black if the display is asleep. Code: `crates/tf_game/src/capture.rs`.
- **Street run (headless):** `cargo run -p tf_sim --release --bin run_level -- <level-id> [--design <file.json>] [--seed <n>] [--population careful|mixed|careless] [--demand-scale <x>] [--events]`. It prints the summary, the event log hash and the speed against real time; `--events` prints the event log first.

## Disk hygiene

The developer's laptops have little storage. `target/` (git-ignored) grows without bound: a debug build can reach tens of GB. Everything in it is regenerable, so keep it small.

- **When:** at the end of a work session or a long task, and whenever `du -sh target` shows more than ~8 GB. Check with `du -sh target target/*`.
- **Routine clean-up:** `cargo clean --profile dev` (drops the debug build; the next `cargo play` recompiles once, slowly). Keep `target/release` unless space is critical: its first build is slow. `cargo clean` removes everything, release included.
- **Lighter option:** remove `target/debug/incremental`.
- **Scratch files:** screenshots, benchmark output and other temporary files go in the scratchpad or a temp dir, never in the repo; delete them when done.
- **Before deleting:** it is safe only for build output. Never delete saved designs in the data dir, or anything untracked you did not create. Tell the user what was freed.

## Architecture rules

- **`tf_sim` is pure and Bevy-free.** Plain structs, no ECS, no rendering, no threads, no wall-clock time, no `unsafe`. Dependencies: `glam`, `serde` / `serde_json`, `ron`, `indexmap`; every version is pinned in the root `Cargo.toml` (`glam` must stay the version Bevy uses). It must be steppable headless from tests. Bevy is used only in `tf_game`; don't upgrade it casually (exact pin). Add a Bevy feature only when code needs one.
- **Determinism.** A run is a pure function of `level + design + seed`. Each agent gets its own `SimRng` stream (`rng`, a PCG32 seeded from `(run seed, agent id)`), so one more agent does not shift the draws of the others; agents update in id order from the state of the previous tick; nothing from the frame (real delta, batch size) enters the sim. Every id-keyed collection is an `IndexMap`; removals use `shift_remove`, never `swap_remove`; no `HashMap` iteration in sim logic. New street code uses no `sin` / `cos` / `exp` / `powf`, so that every platform gives the same bits. `f64` for distances and times held in agents, `f32` / `Vec2` for geometry.
- **Units.** Metres and seconds with real speeds (injury depends on real speed); a level is about 150 m × 150 m. The views draw at `PX_PER_M` pixels per metre.
- **Street model (`tf_sim::street`).** A level is a RON file in `crates/tf_sim/data/levels/` (registered in `street::level::LEVELS`); the player's design is a list of `DesignCommand`s (JSON). `Street::from_level_and_design` replays it; `Street::apply` is the only way to edit and rebuilds the lanes of the touched segments and the movements of their junctions. Lanes and movements are derived, get new ids when rebuilt, and are never saved. Lane geometry is in `geom::lanes` (offset polylines, connector curves, conflict zones); `street::astar` is the A* the walkers use.
- **Street run (`tf_sim::street::run`).** A run is `StreetRun::new(level, design, options)`, then `step()` at 20 Hz. Agents decide from past snapshots, in id order, each with its own `SimRng`. The event log is the result: score, report and replay hashes are read from it. Replay hashes are in `crates/tf_sim/tests/street_run.rs`; a changed hash means behaviour changed, and the commit must say it is deliberate. Vehicles are in `StreetRun::agents`, pedestrians in `StreetRun::walkers` (ids from `WALKER_ID_BASE`, so a walker never shifts the id or the stream of a vehicle); all the draws of a walker are made at spawn. Crossings are data (`Street::crossings`, `DesignCommand::AddCrossing` / `RemoveCrossing`).
- **Threaded runner (`tf_game::street_runner`).** `StreetRunner` moves the run by value to a worker thread, which steps a batch (at most `MAX_TICKS_PER_BATCH`), `LEAD_TICKS` ahead of the screen, and sends back a `StreetSnapshot` after each tick. No locks. Views read the snapshots around the render clock (interpolating), never the run. Without a worker (`threaded: false`: tests, screenshots) batches run inline.
- **Messages and input.** Bevy `Message`s (not observer `Event`s) in `tf_game::events`. Systems read `ActionInput` (`input::KeyBindings` maps actions to physical `KeyCode`s, so WASD = ZQSD on AZERTY) and the `Pointer` resource (world position, buttons), never raw keys or the window. Camera state is in world pixels, y down; the Bevy world is y up (`camera::sim_to_bevy` negates y).
- **Views only read.** They draw simple flat shapes with Bevy's 2D meshes and one `ColorMaterial` (`draw::ShapeBuilder`: vertex colours, no shaders, no textures); depths are the constants in `views::z`. Static shapes are rebuilt when the loaded level changes; moving things (cars, walkers) are one batched mesh each (`views::batch`), rewritten in place every frame. The art direction is to be redone later.
- Only add a feature or retune a constant on purpose (the sim is balanced by playtest); behaviour changes are the user's call.

## Layout

```
Cargo.toml, rust-toolchain.toml   workspace; every dependency version pinned in [workspace.dependencies]
.cargo/config.toml                the `cargo play` alias
crates/tf_sim/                    the simulation library (no Bevy)
  src/  rng (SimRng), geom/ (Curve2D, polygons, segment maths, lanes: polyline helpers),
        street/ (level, design, space, lanes, model, signals, speed, astar, score, run/: the agents and their decisions),
        bin/run_level.rs (headless run)
  data/levels/*.ron               the levels
  tests/                          integration tests, street_*.rs (replay hashes in street_run.rs)
crates/tf_game/                   the Bevy app, binary `transit-flow`
  src/  main.rs, plugin.rs (GamePlugin), street_runner.rs (threaded run), level.rs (LoadedLevel, framing),
        events.rs, input.rs, camera.rs, draw.rs (shape helpers), capture.rs (screenshot mode),
        play/ (design, run and report UI: tools, pick, preview, inspect, signals, markers, file),
        views/ (street, speed_signs, agents, walkers, batch, palette; z constants in views/mod.rs)
  tests/app/                      headless app tests, ONE test executable `app` (main.rs lists the modules); common/ has the Harness
docs/                             URBANISM_PIVOT_PLAN.md (roadmap), IMPLEMENTATION_PLAN.md and BEVY_MIGRATION_PLAN.md (records), migration/ (NOTES, PORTING_GUIDE)
scripts/check.sh                  fmt + clippy + tests
```

## Code style

- `cargo fmt` (rustfmt defaults, `.editorconfig` for the rest) and `clippy -D warnings`; no `unsafe`; `unwrap()` only where a broken invariant should crash (prefer `expect("why")`).
- `//!` under a module and `///` on public items explain the role and the *why* (the rule, the reason for an order), not the *what*.
- Constants `UPPER_SNAKE`; events and messages named in the past tense.
- New sim logic gets tests: next to the code for small units, in `crates/tf_sim/tests/` for behaviour (one `street_*.rs` per topic); `crates/tf_game/tests/app/` for play mode, views and keys (one module per file, added in `main.rs`; run one with `cargo test -p tf_game --test app play::`).

## Git rules

- **Never add Claude as co-author.** No `Co-Authored-By: Claude ...` trailer, no "Generated with Claude Code" line, in commits or PR descriptions. (Also enforced by `.claude/settings.json` -> `attribution`.)
- Commit identity is set in the repo's local config: `gimk <a.pouligny@gmail.com>`. Don't change it.
- Run `scripts/check.sh` before committing.
