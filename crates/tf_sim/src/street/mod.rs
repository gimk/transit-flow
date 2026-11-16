//! The street design game (`docs/URBANISM_PIVOT_PLAN.md`): new code, written next to the
//! old city builder, which stays frozen until the pivot's Phase 5. It may depend only on the
//! modules the plan keeps or adapts (so far `geom`, and `roads::lane_builder` and
//! `roads::lane::Turn` used as they are), never on the dropped ones. Metres and seconds, real speeds;
//! y down like the rest of the sim.
//!
//! - `space`: strips and profiles (the cross-section of a street).
//! - `level`: the level file (RON, embedded) and its loader.
//! - `design`: the player's edits as a command list (JSON).
//! - `lanes`: lanes on strips and the movements through junctions.
//! - `model`: `Street`, the space of one level with a design applied.
//! - `signals`: signal plans (fixed cycles of phases per approach).
//! - `run`: one run of a level: the agents, their decisions, conflicts and contacts.

pub mod design;
pub mod ids;
pub mod lanes;
pub mod level;
pub mod model;
pub mod run;
pub mod signals;
pub mod space;

pub use design::{Design, DesignCommand};
pub use ids::{AgentId, StreetLaneId};
pub use level::LevelFile;
pub use model::{EditError, Street, StreetChange};
