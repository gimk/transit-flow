//! The signal marks: a small disc at every approach of a junction under signals, on the
//! kerb before the junction edge. Grey while designing; in a run it shows the light the
//! approach has now (read from `StreetSnapshot::lights`, which the run's plan fills in, so
//! the view never reads the run). One mesh, rebuilt only when a mark or a colour changes.
//!
//! A junction with a leading bicycle green has a second, smaller light under the car light:
//! the light of the bicycle lane (`SignalPlan::bicycle_light`), which is green before the
//! cars' light when the plan has a lead. While designing there is no run to read, so it
//! shows the fixed colour that says the lead is set.

use bevy::prelude::*;

use super::pick::{self, ApproachMark};
use crate::camera::sim_to_bevy;
use crate::draw::ShapeBuilder;
use crate::level::{LoadedLevel, PX_PER_M, level_to_world};
use crate::street_runner::{SignalColor, StreetRunner};
use crate::views::{ViewMaterial, spawn_shape, z};

pub const OFF_COLOR: Color = Color::srgb(0.50, 0.53, 0.58);
pub const GREEN_COLOR: Color = Color::srgb(0.2, 0.85, 0.3);
pub const AMBER_COLOR: Color = Color::srgb(1.0, 0.7, 0.1);
pub const RED_COLOR: Color = Color::srgb(0.9, 0.15, 0.12);
pub const HOUSING_COLOR: Color = Color::srgb(0.08, 0.08, 0.1);
/// Radius of the lit disc and of its dark housing, metres.
pub const DISC_RADIUS_M: f32 = 0.7;
pub const HOUSING_RADIUS_M: f32 = 0.95;

/// The small light of a leading bicycle green while designing: its colour is fixed then
/// (there is no run to read), its place is under the car light.
pub const BIKE_LIGHT_COLOR: Color = Color::srgb(0.2, 0.7, 0.55);
pub const BIKE_DISC_RADIUS_M: f32 = 0.4;
pub const BIKE_HOUSING_RADIUS_M: f32 = 0.6;
pub const BIKE_LIGHT_OFFSET_M: f32 = 1.7;

/// The one entity that holds every mark.
#[derive(Component)]
pub struct SignalMarks;

/// What a mark shows in a run: the light of the cars of the approach, and the light of its
/// bicycle lane (`ApproachLight::color` and `ApproachLight::bicycle`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lit {
    pub car: SignalColor,
    pub bicycle: SignalColor,
}

pub fn color_of(light: Option<SignalColor>) -> Color {
    match light {
        None => OFF_COLOR,
        Some(SignalColor::Green) => GREEN_COLOR,
        Some(SignalColor::Amber) => AMBER_COLOR,
        Some(SignalColor::Red) => RED_COLOR,
    }
}

/// The colour of the small bicycle light: the light of the bicycle lane in a run, and the
/// fixed colour of the design phase (no run to read).
pub fn bike_color(light: Option<Lit>) -> Color {
    match light {
        Some(lit) => color_of(Some(lit.bicycle)),
        None => BIKE_LIGHT_COLOR,
    }
}

/// The shape of the marks, each with its lights (`None`: not lit, or no run).
pub fn build_marks(marks: &[(ApproachMark, Option<Lit>)]) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new();
    for (mark, light) in marks {
        let center = sim_to_bevy(level_to_world(mark.position));
        shape.circle(center, HOUSING_RADIUS_M * PX_PER_M, 16, HOUSING_COLOR);
        shape.circle(
            center,
            DISC_RADIUS_M * PX_PER_M,
            16,
            color_of(light.as_ref().map(|lit| lit.car)),
        );
        // A junction with a leading bicycle green: a small second light under the first
        // (street y is down, Bevy y is up), the bicycles' own, green before the cars'.
        if mark.bicycle_lead_s > 0.0 {
            let bike = center - Vec2::new(0.0, BIKE_LIGHT_OFFSET_M * PX_PER_M);
            shape.circle(bike, BIKE_HOUSING_RADIUS_M * PX_PER_M, 12, HOUSING_COLOR);
            shape.circle(bike, BIKE_DISC_RADIUS_M * PX_PER_M, 12, bike_color(*light));
        }
    }
    shape
}

type Shown = Vec<(ApproachMark, Option<Lit>)>;

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
                    .map(|l| Lit {
                        car: l.color,
                        bicycle: l.bicycle,
                    })
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

    fn mark(lead_s: f32) -> ApproachMark {
        ApproachMark {
            node: 0,
            segment: 1,
            position: Vec2::ZERO,
            bicycle_lead_s: lead_s,
        }
    }

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

    #[test]
    fn a_lead_adds_a_second_light_and_a_run_colours_it() {
        let lit = Lit {
            car: SignalColor::Red,
            bicycle: SignalColor::Green,
        };
        let plain = build_marks(&[(mark(0.0), Some(lit))]).vertex_count();
        let with_lead = build_marks(&[(mark(4.0), Some(lit))]).vertex_count();
        assert!(with_lead > plain, "a second light: {with_lead} / {plain}");
        // The bicycle light follows the run: green while the cars are red, then red.
        assert_eq!(bike_color(Some(lit)), GREEN_COLOR);
        let later = Lit {
            car: SignalColor::Green,
            bicycle: SignalColor::Red,
        };
        assert_eq!(bike_color(Some(later)), RED_COLOR);
        // While designing there is no run to read: the fixed colour says the lead is set.
        assert_eq!(bike_color(None), BIKE_LIGHT_COLOR);
    }
}
