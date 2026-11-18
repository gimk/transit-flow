//! The player's design (pivot plan §3.4): a small, separate file holding the list of edit
//! commands on top of a level. Loading a design replays its commands on a fresh `Street`,
//! which also gives undo and redo (drop or restore the last command). JSON, like saves.

use serde::{Deserialize, Serialize};

use super::level::{CrossingDef, JunctionControl, Sign, TurnBan};
use super::space::{Direction, Separator, StripUse};

/// A side of a segment, looking along it from its `from` node to its `to` node; the same
/// convention as the profile, whose strips run from the left building line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
}

/// A ready-made signal plan the player picks instead of editing phases (pivot plan §4).
/// Opposite arms share a phase; green times are in seconds, from `MIN_GREEN_S` to
/// `MAX_GREEN_S`. Amber and all-red keep the defaults of `signals.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SignalTemplate {
    TwoPhase {
        green_s: f32,
    },
    /// `TwoPhase` plus a last phase where every approach is red and pedestrians walk.
    TwoPhaseWithWalk {
        green_s: f32,
        walk_s: f32,
    },
}

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
    /// Sets the separator on the boundary between strips `boundary - 1` and `boundary`
    /// (both sides of the line, so the two strips agree). A bicycle track next to a car or
    /// bus lane keeps a physical separator (kerb, bollards or planter).
    SetSeparator {
        segment: i32,
        boundary: usize,
        separator: Separator,
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
    /// A row of bollards ("potelets") along the kerb of one side of a segment, from `from`
    /// to `to` metres along the centerline from the `from` node. The row gets the next
    /// bollard-row id (1, 2, ... in order of placement, so ids are stable through a replay).
    AddBollards {
        segment: i32,
        side: Side,
        from: f32,
        to: f32,
    },
    /// Takes a row of bollards away, by row id.
    RemoveBollards {
        id: i32,
    },
    /// Sets how a junction is controlled. `Signals` with a template builds that plan;
    /// without one the run's default plan is used. `PriorityToRight` takes any plan away
    /// and needs no template.
    SetJunctionControl {
        node: i32,
        control: JunctionControl,
        #[serde(default)]
        template: Option<SignalTemplate>,
    },
    /// A bicycle box (*sas vélo*) on the approach `segment` of the signalled junction
    /// `node`: cyclists wait in a box `depth` metres deep at the crossing, and the car stop
    /// line moves back by `depth`.
    AddBicycleBox {
        node: i32,
        segment: i32,
        depth: f32,
    },
    RemoveBicycleBox {
        node: i32,
        segment: i32,
    },
    /// A leading bicycle green: at the start of each phase the bicycles of its approaches
    /// have green `lead_s` seconds before the cars do. 0 takes it away. Needs a signal
    /// plan from a template.
    SetBicycleLead {
        node: i32,
        lead_s: f32,
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
