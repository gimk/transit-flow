//! Transit Flow: the Bevy app as a library (the binary is `main.rs`), so integration tests
//! can use it. Game shell so far (migration work packages M8.1 and M8.2): the threaded simulation
//! runner (`runner`), the snapshots views draw from (`snapshot`), the message bus
//! (`events`), keys and pointer (`input`), the map camera (`camera`), game commands and saves (`game`) and the plugin wiring them (`plugin`), flat-shape drawing (`draw`) and the map views (`views`, M9). The street game of the pivot
//! (`docs/URBANISM_PIVOT_PLAN.md`) adds the loaded level (`level`) and its view (`views::street`).
//! See `docs/BEVY_MIGRATION_PLAN.md` §2.3.

pub mod bench;
pub mod camera;
pub mod capture;
pub mod draw;
pub mod events;
pub mod game;
pub mod input;
pub mod level;
pub mod plugin;
pub mod runner;
pub mod snapshot;
pub mod status;
pub mod tools;
pub mod views;
