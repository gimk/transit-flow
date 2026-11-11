//! Transit Flow simulation: the port of the Godot project's `sim/` folder.
//!
//! Pure data and logic, stepped at a fixed tick, with no engine dependency, so it runs
//! headless in tests and benchmarks. The game (`tf_game`) only reads it through view
//! snapshots and changes it through command methods. Porting rules and the
//! architecture are in `docs/BEVY_MIGRATION_PLAN.md`.
//!
//! Modules are empty until their work package ports them (see the plan's §6).

pub mod city;
pub mod clock;
pub mod debug;
pub mod geom;
pub mod rng;
pub mod roads;
pub mod routing;
pub mod save;
pub mod simulation;
pub mod traffic;
pub mod transit;
pub mod walking;
