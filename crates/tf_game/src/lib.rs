//! Transit Flow: the Bevy app as a library (the binary is `main.rs`), so integration tests
//! can use it. The street design game (`docs/URBANISM_PIVOT_PLAN.md`): the loaded level
//! (`level`) and its views (`views`), the street runner that steps a run on a worker thread
//! (`street_runner`), play mode (`play`: design, run, report), keys and pointer (`input`),
//! the map camera (`camera`), flat-shape drawing (`draw`), the message bus (`events`), the
//! plugin that wires the shell (`plugin`) and the screenshot mode (`capture`).

pub mod camera;
pub mod capture;
pub mod draw;
pub mod events;
pub mod input;
pub mod level;
pub mod play;
pub mod plugin;
pub mod street_runner;
pub mod views;
