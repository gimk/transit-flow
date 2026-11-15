//! The street game's level (pivot plan Phase 1): `LoadedLevel` holds the `Street` built
//! from a level file and a design, and the views draw it. Nothing runs on it yet: agents
//! arrive in Phase 2. The old city builder's sim keeps running beside it, untouched.
//!
//! The street model is in metres; the map camera and the views work in world pixels, so
//! the level is drawn at `PX_PER_M` pixels per metre (the camera's zoom steps were made
//! for pixel sizes, and at 1 px per metre a 1.5 m bicycle lane would be a hairline).

use bevy::prelude::*;
use tf_sim::street::level::Bounds;
use tf_sim::street::{Design, LevelFile, Street};

use crate::camera::{MapCamera, ZOOM_LEVELS};

pub const PX_PER_M: f32 = 8.0;
/// The framed level fills at most this part of the window.
pub const FRAME_MARGIN: f32 = 0.95;

/// Street position (metres, y down) to sim world pixels (y down).
pub fn level_to_world(p: Vec2) -> Vec2 {
    p * PX_PER_M
}

#[derive(Resource, Default, Debug)]
pub struct LoadedLevel {
    pub street: Option<Street>,
    /// The camera zoom to use when the level is framed; `None` fits the level.
    pub zoom: Option<f32>,
}

impl LoadedLevel {
    /// Loads a shipped level with an empty design.
    pub fn load(id: &str) -> Result<Street, String> {
        let level = LevelFile::load(id).map_err(|error| error.to_string())?;
        Street::from_level_and_design(&level, &Design::new(id)).map_err(|error| error.to_string())
    }
}

/// The biggest zoom step that shows all of `bounds` in `viewport`, and the centre.
pub fn frame(bounds: &Bounds, viewport: Vec2) -> (Vec2, f32) {
    let size = bounds.size() * PX_PER_M;
    let fits = |zoom: f32| {
        size.x * zoom <= viewport.x * FRAME_MARGIN && size.y * zoom <= viewport.y * FRAME_MARGIN
    };
    let zoom = ZOOM_LEVELS
        .iter()
        .rev()
        .copied()
        .find(|zoom| fits(*zoom))
        .unwrap_or(ZOOM_LEVELS[0]);
    (level_to_world(bounds.center()), zoom)
}

/// Frames the camera on a level when one is loaded.
fn frame_new_level(level: Res<LoadedLevel>, mut cameras: Query<&mut MapCamera>) {
    if !level.is_changed() {
        return;
    }
    let Some(street) = &level.street else { return };
    for mut camera in &mut cameras {
        let (center, zoom) = frame(&street.bounds, camera.viewport);
        camera.zoom = level.zoom.unwrap_or(zoom);
        camera.focus_on(center);
    }
}

pub struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadedLevel>()
            .add_systems(Update, frame_new_level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_level_loads_and_frames() {
        let street = LoadedLevel::load("carrefour-ecole").expect("loads");
        // 150 m at 8 px/m = 1200 px: zoom 0.5 fits 720 px of height, 0.75 does not.
        let (center, zoom) = frame(&street.bounds, Vec2::new(1280.0, 720.0));
        assert_eq!(center, Vec2::new(600.0, 600.0));
        assert_eq!(zoom, 0.5);
        assert!(LoadedLevel::load("nowhere").is_err());
    }
}
