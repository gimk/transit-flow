//! Transit Flow: the Bevy app as a library (the binary is `main.rs`), so integration tests
//! can use it. Game shell so far (migration work packages M8.1 and M8.2): the threaded simulation
//! runner (`runner`), the snapshots views draw from (`snapshot`), the message bus
//! (`events`), keys and pointer (`input`), the map camera (`camera`), game commands and saves (`game`) and the plugin wiring them (`plugin`).
//! See `docs/BEVY_MIGRATION_PLAN.md` §2.3.

pub mod camera;
pub mod capture;
pub mod events;
pub mod game;
pub mod input;
pub mod plugin;
pub mod runner;
pub mod snapshot;
