//! Transit Flow simulation: the port of the Godot project's `sim/` folder (last seen in the git tag `godot-final`).
//!
//! Pure data and logic, stepped at a fixed tick, with no engine dependency, so it runs
//! headless in tests and benchmarks. The game (`tf_game`) only reads it through view
//! snapshots and changes it through command methods. Porting rules and the
//! architecture are in `docs/BEVY_MIGRATION_PLAN.md`; how to port a class is in
//! `docs/migration/PORTING_GUIDE.md`.
//!
//! `street` is the new street design game (`docs/URBANISM_PIVOT_PLAN.md`), written next to
//! the old city builder, which stays frozen until the pivot's Phase 5 removes it.

pub mod city;
pub mod clock;
pub mod debug;
pub mod events;
pub mod geom;
pub mod grid;
pub mod ids;
pub mod rng;
pub mod roads;
pub mod routing;
pub mod save;
pub mod simulation;
pub mod street;
pub mod traffic;
pub mod transit;
pub mod walking;

pub use simulation::Simulation;
