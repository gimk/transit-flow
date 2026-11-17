//! The street design game (`docs/URBANISM_PIVOT_PLAN.md`): a level is one real intersection;
//! the player designs how its width is shared, then a run of agents plays on it. Metres and
//! seconds, real speeds; y down. A run is a pure function of level, design and seed.
//!
//! - `astar`: the A* search the walkers use over their area graph.
//! - `space`: strips and profiles (the cross-section of a street).
//! - `level`: the level file (RON, embedded) and its loader.
//! - `design`: the player's edits as a command list (JSON).
//! - `lanes`: lanes on strips and the movements through junctions.
//! - `model`: `Street`, the space of one level with a design applied.
//! - `signals`: signal plans (fixed cycles of phases per approach).
//! - `speed`: the speed a street asks for by its look (width, side friction, sight).
//! - `run`: one run of a level: the agents, their decisions, conflicts and contacts.
//! - `score`: score, verdict and report of a star stage, read from the event log.

pub mod astar;
pub mod design;
pub mod ids;
pub mod lanes;
pub mod level;
pub mod model;
pub mod run;
pub mod score;
pub mod signals;
pub mod space;
pub mod speed;

pub use design::{Design, DesignCommand};
pub use ids::{AgentId, StreetLaneId};
pub use level::LevelFile;
pub use model::{EditError, Street, StreetChange};
