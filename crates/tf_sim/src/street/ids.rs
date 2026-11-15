//! Ids of the street model. Nodes, segments and junctions use the level's own `i32` ids
//! (authored and stable); lanes are derived, so they get ids from a counter, and a
//! rebuilt lane gets a new one.

use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct StreetLaneId(pub i32);

impl std::fmt::Display for StreetLaneId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
