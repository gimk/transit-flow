//! The marks of a run on the map: a yellow disc where a light conflict happened, orange for
//! a serious one, red for a contact (with a dark ring when someone was hurt). They are read
//! from `StreetRunner::incidents`, stay after the run (through the report and the next
//! design) and go when the next run starts. One mesh, rebuilt when the list changes.

use bevy::prelude::*;
use tf_sim::street::run::Outcome;

use crate::camera::sim_to_bevy;
use crate::draw::ShapeBuilder;
use crate::level::{PX_PER_M, level_to_world};
use crate::street_runner::{Incident, IncidentKind, StreetRunner};
use crate::views::{ViewMaterial, spawn_shape, z};

pub const LIGHT_COLOR: Color = Color::srgba(1.0, 0.92, 0.2, 0.75);
pub const SERIOUS_COLOR: Color = Color::srgba(1.0, 0.55, 0.1, 0.8);
pub const CONTACT_COLOR: Color = Color::srgba(0.9, 0.1, 0.1, 0.85);
pub const RING_COLOR: Color = Color::srgba(0.1, 0.0, 0.0, 0.9);
/// Radius of a mark, metres.
pub const LIGHT_RADIUS_M: f32 = 1.0;
pub const SERIOUS_RADIUS_M: f32 = 1.4;
pub const CONTACT_RADIUS_M: f32 = 1.8;
const RING_EXTRA_M: f32 = 0.35;
const SEGMENTS: u32 = 20;

/// The one entity that holds every mark.
#[derive(Component)]
pub struct IncidentMarks;

/// The shape of all the marks.
pub fn build_marks(incidents: &[Incident]) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new();
    for incident in incidents {
        let center = sim_to_bevy(level_to_world(incident.position));
        let (radius, color, ring) = match incident.kind {
            IncidentKind::LightConflict => (LIGHT_RADIUS_M, LIGHT_COLOR, false),
            IncidentKind::SeriousConflict => (SERIOUS_RADIUS_M, SERIOUS_COLOR, false),
            IncidentKind::Contact(outcome) => (
                CONTACT_RADIUS_M,
                CONTACT_COLOR,
                outcome != Outcome::NoInjury,
            ),
        };
        if ring {
            shape.circle(
                center,
                (radius + RING_EXTRA_M) * PX_PER_M,
                SEGMENTS,
                RING_COLOR,
            );
        }
        shape.circle(center, radius * PX_PER_M, SEGMENTS, color);
    }
    shape
}

fn update_marks(
    runner: Res<StreetRunner>,
    marks: Query<Entity, With<IncidentMarks>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
    mut shown: Local<u64>,
) {
    if *shown == runner.incident_version() {
        return;
    }
    *shown = runner.incident_version();
    for entity in &marks {
        commands.entity(entity).despawn();
    }
    let shape = build_marks(runner.incidents());
    if !shape.is_empty() {
        let entity = spawn_shape(&mut commands, &mut meshes, &material, shape, z::INCIDENTS);
        commands.entity(entity).insert(IncidentMarks);
    }
}

pub struct MarkersPlugin;

impl Plugin for MarkersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StreetRunner>()
            .add_systems(Update, update_marks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incident(kind: IncidentKind) -> Incident {
        Incident {
            tick: 0,
            position: Vec2::new(10.0, 20.0),
            kind,
        }
    }

    #[test]
    fn a_mark_is_a_disc_and_a_hurt_contact_has_a_ring() {
        let none = build_marks(&[]);
        assert!(none.is_empty());
        let light = build_marks(&[incident(IncidentKind::LightConflict)]).triangle_count();
        let hit = build_marks(&[incident(IncidentKind::Contact(Outcome::NoInjury))]);
        let hurt = build_marks(&[incident(IncidentKind::Contact(Outcome::SeriousInjury))]);
        assert_eq!(light, SEGMENTS as usize);
        assert_eq!(hit.triangle_count(), light);
        assert_eq!(hurt.triangle_count(), 2 * light);
    }
}
