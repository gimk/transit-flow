//! The tools of play mode. A tool turns a click (or a drag) on the street (metres) into one
//! `DesignCommand`; `PlayState` applies it and keeps it in the design. To add a tool: add
//! a variant here, with its label, its action (a key of `KeyBindings`) and its arms in
//! `click` or `release`; nothing else changes.
//!
//! Keys are the old game's tool keys, reused because `KeyBindings` allows no key twice and
//! the old tools are off in play mode: `C` (`ToolCommercial`) is the zebra, `V`
//! (`RoadCurveMode`) the speed limit, `B` (`ToolBulldoze`) the bollards, `L`
//! (`ToolSignals`) the signals.

use bevy::prelude::Vec2;
use tf_sim::street::design::{Side, SignalTemplate};
use tf_sim::street::level::JunctionControl;
use tf_sim::street::{DesignCommand, Street};

use super::pick;
use crate::input::Action;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayTool {
    Zebra,
    SpeedLimit,
    Bollards,
    Signals,
}

impl PlayTool {
    pub const ALL: [PlayTool; 4] = [
        PlayTool::Zebra,
        PlayTool::SpeedLimit,
        PlayTool::Bollards,
        PlayTool::Signals,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PlayTool::Zebra => "Zebra crossing",
            PlayTool::SpeedLimit => "Speed limit",
            PlayTool::Bollards => "Bollards",
            PlayTool::Signals => "Signals",
        }
    }

    /// What a click does, for the help text.
    pub fn hint(self) -> &'static str {
        match self {
            PlayTool::Zebra => "click the road to add a zebra, click a zebra to remove it",
            PlayTool::SpeedLimit => "click a street to change its limit (20, 30, 50 km/h)",
            PlayTool::Bollards => {
                "drag along a sidewalk edge to place bollards, click a row to remove it"
            }
            PlayTool::Signals => {
                "click a junction: priority, lights, lights with a walk phase; wheel or +/- on lights: green time"
            }
        }
    }

    /// The key that selects the tool (pressed again: no tool).
    pub fn action(self) -> Action {
        match self {
            PlayTool::Zebra => Action::ToolCommercial,
            PlayTool::SpeedLimit => Action::RoadCurveMode,
            PlayTool::Bollards => Action::ToolBulldoze,
            PlayTool::Signals => Action::ToolSignals,
        }
    }

    /// The key's name for the help line.
    pub fn key_name(self) -> &'static str {
        match self {
            PlayTool::Zebra => "C",
            PlayTool::SpeedLimit => "V",
            PlayTool::Bollards => "B",
            PlayTool::Signals => "L",
        }
    }

    /// The tool acts when the button comes up (after a drag) and not when it goes down.
    pub fn is_drag(self) -> bool {
        matches!(self, PlayTool::Bollards)
    }

    /// The edit a click at `p` makes with this tool, or `None` if there is nothing there.
    /// For a drag tool this is not used: see `release`.
    pub fn click(self, street: &Street, p: Vec2) -> Option<DesignCommand> {
        match self {
            PlayTool::Zebra => pick::zebra_command(street, p),
            PlayTool::SpeedLimit => pick::speed_command(street, p),
            PlayTool::Signals => pick::signals_click_command(street, p),
            PlayTool::Bollards => None,
        }
    }

    /// The edit of a drag tool when the button comes up at `end` after going down at
    /// `start` (a short drag is a click).
    pub fn release(self, street: &Street, start: Vec2, end: Vec2) -> Option<DesignCommand> {
        match self {
            PlayTool::Bollards => pick::bollards_release(street, start, end),
            _ => self.click(street, end),
        }
    }

    /// What to tell the player when a click or drag found nothing to do.
    pub fn nothing_text(self) -> &'static str {
        match self {
            PlayTool::Bollards => "Drag along one sidewalk edge, or click a row to remove it.",
            PlayTool::Signals => "Click inside a junction.",
            _ => "Nothing to change there.",
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
            DesignCommand::AddBollards {
                segment,
                side,
                from,
                to,
            } => format!(
                "Bollards on the {} of segment {segment}, {from:.1} to {to:.1} m.",
                match side {
                    Side::Left => "left",
                    Side::Right => "right",
                }
            ),
            DesignCommand::RemoveBollards { id } => format!("Bollard row {id} removed."),
            DesignCommand::SetJunctionControl {
                node,
                control,
                template,
            } => {
                let what = match (control, template) {
                    (JunctionControl::PriorityToRight, _) => "priority to the right".to_string(),
                    (_, Some(SignalTemplate::TwoPhase { green_s })) => {
                        format!("lights, green {green_s:.0} s")
                    }
                    (_, Some(SignalTemplate::TwoPhaseWithWalk { green_s, walk_s })) => {
                        format!("lights, green {green_s:.0} s, walk {walk_s:.0} s")
                    }
                    (_, None) => "lights".to_string(),
                };
                format!("Junction {node}: {what}.")
            }
            other => format!("Edit made: {other:?}"),
        }
    }
}
