//! Transit Flow simulation: pure data and logic with no engine dependency, stepped at a
//! fixed tick, so it runs headless in tests and benchmarks. The game (`tf_game`) only
//! reads it through snapshots and changes it through design commands.
//!
//! `street` is the street design game (`docs/URBANISM_PIVOT_PLAN.md`): levels, designs and
//! runs. `geom` is the geometry it uses, `rng` the random stream each agent gets.
//! The old city builder (git tag `city-builder-final`) is gone.

pub mod geom;
pub mod rng;
pub mod street;
