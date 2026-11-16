//! The player's design (pivot plan §3.4): a small, separate file holding the list of edit
//! commands on top of a level. Loading a design replays its commands on a fresh `Street`,
//! which also gives undo and redo (drop or restore the last command). JSON, like saves.

use serde::{Deserialize, Serialize};

use super::level::{CrossingDef, Sign, TurnBan};
use super::space::{Direction, StripUse};

/// One edit of the street space. Segments and nodes are the level's ids (stable);
/// strips are indices into the segment's profile, left building line first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DesignCommand {
    /// Changes what a strip is for. A strip that becomes a travel strip gets the
    /// direction given (a non-travel strip loses its direction).
    SetStripUse {
        segment: i32,
        strip: usize,
        #[serde(rename = "use")]
        use_: StripUse,
        direction: Direction,
    },
    SetStripDirection {
        segment: i32,
        strip: usize,
        direction: Direction,
    },
    /// Moves the boundary between strips `boundary - 1` and `boundary` by `delta` metres
    /// (positive = to the right): one widens by what the other loses, so the total width
    /// stays fixed.
    MoveBoundary {
        segment: i32,
        boundary: usize,
        delta: f32,
    },
    SetSpeedLimit {
        segment: i32,
        kmh: f32,
    },
    BanTurn {
        node: i32,
        ban: TurnBan,
    },
    UnbanTurn {
        node: i32,
        ban: TurnBan,
    },
    SetSign {
        node: i32,
        segment: i32,
        sign: Sign,
    },
    /// Places a crossing; it gets the next crossing id (the level's crossings have the
    /// ids 1, 2, ... in file order, so ids are stable through a replay).
    AddCrossing {
        crossing: CrossingDef,
    },
    /// Takes a crossing away (the level's own too), by id.
    RemoveCrossing {
        id: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Design {
    pub version: u32,
    pub level: String,
    pub commands: Vec<DesignCommand>,
}

impl Design {
    /// Bump when the format changes incompatibly.
    pub const VERSION: u32 = 1;

    pub fn new(level: &str) -> Self {
        Self {
            version: Self::VERSION,
            level: level.to_string(),
            commands: Vec::new(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a design always serializes")
    }

    pub fn from_json(text: &str) -> Result<Design, String> {
        let design: Design = serde_json::from_str(text).map_err(|error| error.to_string())?;
        if design.version != Self::VERSION {
            return Err(format!(
                "design version {} is not supported (expected {})",
                design.version,
                Self::VERSION
            ));
        }
        Ok(design)
    }
}
