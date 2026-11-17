//! The signal marks: a small disc at every approach of a junction under signals, on the
//! kerb before the junction edge. Grey while designing; in a run it shows the light the
//! approach has now (read from `StreetSnapshot::lights`, which the run's plan fills in, so
//! the view never reads the run). One mesh, rebuilt only when a mark or a colour changes.

use bevy::prelude::*;

use super::pick::{self, ApproachMark};
use crate::camera::sim_to_bevy;
use crate::draw::ShapeBuilder;
use crate::level::{LoadedLevel, PX_PER_M, level_to_world};
use crate::street_runner::{SignalColor, StreetRunner};
use crate::views::{ViewMaterial, spawn_shape, z};

pub const OFF_COLOR: Color = Color::srgb(0.55, 0.55, 0.58);
pub const GREEN_COLOR: Color = Color::srgb(0.2, 0.85, 0.3);
pub const AMBER_COLOR: Color = Color::srgb(1.0, 0.7, 0.1);
pub const RED_COLOR: Color = Color::srgb(0.9, 0.15, 0.12);
pub const HOUSING_COLOR: Color = Color::srgb(0.08, 0.08, 0.1);
/// Radius of the lit disc and of its dark housing, metres.
pub const DISC_RADIUS_M: f32 = 0.7;
pub const HOUSING_RADIUS_M: f32 = 0.95;

/// The one entity that holds every mark.
#[derive(Component)]
pub struct SignalMarks;

pub fn color_of(light: Option<SignalColor>) -> Color {
    match light {
        None => OFF_COLOR,
        Some(SignalColor::Green) => GREEN_COLOR,
        Some(SignalColor::Amber) => AMBER_COLOR,
        Some(SignalColor::Red) => RED_COLOR,
    }
}

/// The shape of the marks, each with its light (`None`: not lit).
pub fn build_marks(marks: &[(ApproachMark, Option<SignalColor>)]) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new();
    for (mark, light) in marks {
        let center = sim_to_bevy(level_to_world(mark.position));
        shape.circle(center, HOUSING_RADIUS_M * PX_PER_M, 16, HOUSING_COLOR);
        shape.circle(center, DISC_RADIUS_M * PX_PER_M, 16, color_of(*light));
    }
    shape
}

type Shown = Vec<(ApproachMark, Option<SignalColor>)>;

fn update_signal_marks(
    loaded: Res<LoadedLevel>,
    runner: Res<StreetRunner>,
    existing: Query<Entity, With<SignalMarks>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
    mut shown: Local<Option<Shown>>,
) {
    let marks: Shown = loaded
        .street
        .as_ref()
        .map(pick::approach_marks)
        .unwrap_or_default()
        .into_iter()
        .map(|mark| {
            let light = runner.is_running().then(|| {
                runner
                    .view_to()
                    .lights
                    .iter()
                    .find(|l| l.node == mark.node && l.segment == mark.segment)
                    .map(|l| l.color)
            });
            (mark, light.flatten())
        })
        .collect();
    if shown.as_ref() == Some(&marks) {
        return;
    }
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let shape = build_marks(&marks);
    if !shape.is_empty() {
        let entity = spawn_shape(&mut commands, &mut meshes, &material, shape, z::SIGNALS);
        commands.entity(entity).insert(SignalMarks);
    }
    *shown = Some(marks);
}

pub struct SignalMarksPlugin;

impl Plugin for SignalMarksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StreetRunner>()
            .add_systems(Update, update_signal_marks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_light_has_its_own_colour() {
        let colors = [
            color_of(None),
            color_of(Some(SignalColor::Green)),
            color_of(Some(SignalColor::Amber)),
            color_of(Some(SignalColor::Red)),
        ];
        for (i, a) in colors.iter().enumerate() {
            for b in &colors[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
