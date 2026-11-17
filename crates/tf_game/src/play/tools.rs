//! The tools of play mode. A tool turns a click on the street (metres) into one
//! `DesignCommand`; `PlayState` applies it and keeps it in the design. To add a tool (the
//! next brief adds bollards and signals): add a variant here, with its label, its action
//! (a key of `KeyBindings`) and its arm in `click`; nothing else changes.
//!
//! Keys are the old game's tool keys, reused because `KeyBindings` allows no key twice and
//! the old tools are off in play mode: `C` (`ToolCommercial`) is the zebra, `V`
//! (`RoadCurveMode`) the speed limit.

use bevy::prelude::Vec2;
use tf_sim::street::{DesignCommand, Street};

use super::pick;
use crate::input::Action;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayTool {
    Zebra,
    SpeedLimit,
}

impl PlayTool {
    pub const ALL: [PlayTool; 2] = [PlayTool::Zebra, PlayTool::SpeedLimit];

    pub fn label(self) -> &'static str {
        match self {
            PlayTool::Zebra => "Zebra crossing",
            PlayTool::SpeedLimit => "Speed limit",
        }
    }

    /// What a click does, for the help text.
    pub fn hint(self) -> &'static str {
        match self {
            PlayTool::Zebra => "click the road to add a zebra, click a zebra to remove it",
            PlayTool::SpeedLimit => "click a street to change its limit (20, 30, 50 km/h)",
        }
    }

    /// The key that selects the tool (pressed again: no tool).
    pub fn action(self) -> Action {
        match self {
            PlayTool::Zebra => Action::ToolCommercial,
            PlayTool::SpeedLimit => Action::RoadCurveMode,
        }
    }

    /// The key's name for the help line.
    pub fn key_name(self) -> &'static str {
        match self {
            PlayTool::Zebra => "C",
            PlayTool::SpeedLimit => "V",
        }
    }

    /// The edit a click at `p` makes with this tool, or `None` if there is nothing there.
    pub fn click(self, street: &Street, p: Vec2) -> Option<DesignCommand> {
        match self {
            PlayTool::Zebra => pick::zebra_command(street, p),
            PlayTool::SpeedLimit => pick::speed_command(street, p),
        }
    }

    /// A line for the player after `command` was kept.
    pub fn done_text(command: &DesignCommand) -> String {
        match command {
            DesignCommand::AddCrossing { crossing } => {
                format!(
                    "Zebra added at {:.1} m on segment {}.",
                    crossing.at, crossing.segment
                )
            }
            DesignCommand::RemoveCrossing { id } => format!("Crossing {id} removed."),
            DesignCommand::SetSpeedLimit { segment, kmh } => {
                format!("Segment {segment}: limit {kmh} km/h.")
            }
            other => format!("Edit made: {other:?}"),
        }
    }
}
