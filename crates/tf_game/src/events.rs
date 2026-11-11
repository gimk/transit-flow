//! The game's signal bus (`core/events.gd`, autoload "Events"): one Bevy `Message` per
//! Godot signal, same names (CamelCase). Views and UI read them with `MessageReader`.
//! Messages are buffered, which is what Godot's signals were used as here (nobody needs
//! to react inside the emitter's call), and they are written on the main thread only.
//!
//! The ones relayed from the simulation (`SimulationReset`, `HourChanged`, `DayStarted`,
//! `RoadNetworkChanged`, `LotsChanged`, `ZonesChanged`, `ExternalsChanged`) come from the
//! runner (`runner::SimHandle::advance`); the rest are sent by the tools, UI and camera
//! that own them (M8.2, M10). `dot_band_changed` is not ported: the dot display isn't
//! (plan §1).

use bevy::prelude::*;
use glam::Vec2;
use tf_sim::ids::{NodeId, SegmentId};

/// A new `Simulation` instance replaced the previous one (new game / load).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct SimulationReset;

#[derive(Message, Clone, Debug, PartialEq)]
pub struct SpeedChanged {
    pub speed_index: usize,
    pub multiplier: u32,
}

#[derive(Message, Clone, Debug, PartialEq)]
pub struct HourChanged(pub i32);

#[derive(Message, Clone, Debug, PartialEq)]
pub struct DayStarted(pub i32);

/// Relayed from `RoadNetwork.changed`. Ids may refer to things that were just removed.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct RoadNetworkChanged {
    pub node_ids: Vec<NodeId>,
    pub segment_ids: Vec<SegmentId>,
}

/// Relayed from `LotSystem.changed` (once per frame at most).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct LotsChanged;

/// Relayed from `ZoneSystem.changed` (once per frame at most).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct ZonesChanged;

/// Relayed from `ExternalConnections.changed`.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct ExternalsChanged;

/// UI asks for a tool by id (`None`: no tool); the tool manager answers with
/// `ToolChanged`.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct ToolRequested(pub Option<String>);

#[derive(Message, Clone, Debug, PartialEq)]
pub struct ToolChanged(pub Option<String>);

/// UI asks for a tool in a given variant (for example the road tool and a road type id;
/// `None`: the tool as it is). Unlike `ToolRequested` it never toggles the tool off.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct ToolVariantRequested {
    pub tool_id: String,
    pub variant: Option<String>,
}

/// A tool switched variant (from the UI or its shortcut).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct ToolVariantChanged {
    pub tool_id: String,
    pub variant: Option<String>,
}

/// Contextual help or error text for the active tool (empty hides it).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct ToolHintChanged(pub String);

/// Debug views (overlay ids "lanes", "heatmap"). The UI asks with
/// `DebugOverlayToggleRequested`; the overlay answers with `DebugOverlayChanged` so
/// keys and indicators stay in sync.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct DebugOverlayToggleRequested(pub String);

#[derive(Message, Clone, Debug, PartialEq)]
pub struct DebugOverlayChanged {
    pub overlay_id: String,
    pub shown: bool,
}

/// Ask the map camera to centre on a world position.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct CameraFocusRequested(pub Vec2);

/// Short message shown briefly to the player (for example "Game saved").
#[derive(Message, Clone, Debug, PartialEq)]
pub struct NotificationPosted(pub String);

/// Registers every message type.
pub struct EventsPlugin;

impl Plugin for EventsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SimulationReset>()
            .add_message::<SpeedChanged>()
            .add_message::<HourChanged>()
            .add_message::<DayStarted>()
            .add_message::<RoadNetworkChanged>()
            .add_message::<LotsChanged>()
            .add_message::<ZonesChanged>()
            .add_message::<ExternalsChanged>()
            .add_message::<ToolRequested>()
            .add_message::<ToolChanged>()
            .add_message::<ToolVariantRequested>()
            .add_message::<ToolVariantChanged>()
            .add_message::<ToolHintChanged>()
            .add_message::<DebugOverlayToggleRequested>()
            .add_message::<DebugOverlayChanged>()
            .add_message::<CameraFocusRequested>()
            .add_message::<NotificationPosted>();
    }
}
