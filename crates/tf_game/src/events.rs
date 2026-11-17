//! The game's message bus: one Bevy `Message` per thing the views and the UI react to.
//! Messages are buffered and written on the main thread only.

use bevy::prelude::*;
use glam::Vec2;

/// Ask the map camera to centre on a world position.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct CameraFocusRequested(pub Vec2);

/// Registers every message type.
pub struct EventsPlugin;

impl Plugin for EventsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CameraFocusRequested>();
    }
}
