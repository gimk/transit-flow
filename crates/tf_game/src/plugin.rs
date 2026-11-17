//! `GamePlugin`: the shared shell of the street game: messages, input, the camera, the
//! loaded level and the street runner. Play mode (`play::PlayPlugin`) and the views
//! (`views::ViewsPlugin`) are added next to it.

use bevy::prelude::*;

use crate::camera::MapCameraPlugin;
use crate::events::EventsPlugin;
use crate::input::GameInputPlugin;
use crate::level::LevelPlugin;
use crate::street_runner::StreetRunPlugin;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            EventsPlugin,
            GameInputPlugin,
            MapCameraPlugin,
            LevelPlugin,
            StreetRunPlugin,
        ));
    }
}
