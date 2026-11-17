//! The street run's agents (pivot plan Phase 2): each car an oriented rectangle in
//! metres (drawn at `PX_PER_M`), with a small dark mark at the front so the heading shows.
//! **One entity, one mesh**, rewritten in place every frame from the snapshots around the
//! `StreetRunner`'s render clock, like `moving`.
//!
//! The colour tells the driving style, so a mix of drivers is readable at a glance: careful
//! blue, resident green-grey, commuter grey, taxi yellow, delivery white, careless red.
//! A waiting agent is darker, a crashed one magenta.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;

use super::moving::{Batch, empty_batch_mesh};
use super::{ViewMaterial, z};
use crate::camera::sim_to_bevy;
use crate::level::{PX_PER_M, level_to_world};
use crate::snapshot::{Color as SimColor, lerp_angle};
use crate::street_runner::{StreetRunner, StreetSnapshot};

// Darker and more saturated than the light map (`palette`), so a car reads on the blue-grey
// carriageway; delivery is charcoal because white would vanish on it.
pub const CAREFUL_COLOR: SimColor = [0.10, 0.36, 0.84, 1.0];
pub const RESIDENT_COLOR: SimColor = [0.12, 0.55, 0.34, 1.0];
pub const COMMUTER_COLOR: SimColor = [0.36, 0.40, 0.48, 1.0];
pub const TAXI_COLOR: SimColor = [0.94, 0.70, 0.02, 1.0];
pub const DELIVERY_COLOR: SimColor = [0.17, 0.18, 0.23, 1.0];
pub const CARELESS_COLOR: SimColor = [0.86, 0.10, 0.10, 1.0];
pub const CRASHED_COLOR: SimColor = [1.0, 0.0, 1.0, 1.0];
/// The front mark (the windscreen): light, so it shows on every body colour.
pub const FRONT_COLOR: SimColor = [0.94, 0.95, 0.97, 1.0];
/// A waiting agent's colour is multiplied by this.
pub const WAITING_SHADE: f32 = 0.6;
/// The front mark's length (along the heading) and its inset from the nose, metres.
const MARK_LENGTH_M: f32 = 0.5;
const MARK_INSET_M: f32 = 0.3;

/// The colour of an archetype's name (`Agent::archetype`); grey for an unknown one.
pub fn archetype_color(archetype: &str) -> SimColor {
    match archetype {
        "careful" => CAREFUL_COLOR,
        "resident" => RESIDENT_COLOR,
        "commuter" => COMMUTER_COLOR,
        "taxi" => TAXI_COLOR,
        "delivery" => DELIVERY_COLOR,
        "careless" => CARELESS_COLOR,
        _ => COMMUTER_COLOR,
    }
}

/// The batched mesh and its buffer.
#[derive(Resource, Debug)]
pub struct AgentViews {
    pub mesh: Handle<Mesh>,
    batch: Batch,
    /// Cars drawn in the last frame.
    pub drawn: usize,
}

impl FromWorld for AgentViews {
    fn from_world(world: &mut World) -> Self {
        let material = world.resource::<ViewMaterial>().0.clone();
        let mesh = world.resource_mut::<Assets<Mesh>>().add(empty_batch_mesh());
        world.spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material),
            Transform::from_xyz(0.0, 0.0, z::AGENTS),
            // The mesh changes every frame, and so would its bounds.
            NoFrustumCulling,
        ));
        Self {
            mesh,
            batch: Batch::default(),
            drawn: 0,
        }
    }
}

/// Fills `batch` with the agents of `to`, `alpha` of the way from their pose in `from`.
/// Agents new in `to` stand where `to` has them; those only in `from` are gone. Returns
/// how many cars were drawn.
pub fn draw_agents(
    batch: &mut Batch,
    from: &StreetSnapshot,
    to: &StreetSnapshot,
    alpha: f32,
) -> usize {
    batch.clear();
    for pose in &to.agents {
        let (pos, angle) = match from.index_of(pose.id) {
            Some(j) => {
                let before = &from.agents[j];
                (
                    before.pos.lerp(pose.pos, alpha),
                    lerp_angle(before.dir.to_angle(), pose.dir.to_angle(), alpha),
                )
            }
            None => (pose.pos, pose.dir.to_angle()),
        };
        let color = if pose.crashed {
            CRASHED_COLOR
        } else {
            let base = archetype_color(pose.archetype);
            if pose.waiting {
                [
                    base[0] * WAITING_SHADE,
                    base[1] * WAITING_SHADE,
                    base[2] * WAITING_SHADE,
                    base[3],
                ]
            } else {
                base
            }
        };
        let center = sim_to_bevy(level_to_world(pos));
        // The sim turns clockwise on screen (y down); the Bevy world is y up.
        let bevy_angle = -angle;
        let size = Vec2::new(pose.length, pose.width) * PX_PER_M;
        batch.quad(center, size, bevy_angle, color);
        let forward = Vec2::from_angle(bevy_angle);
        let nose = (pose.length * 0.5 - MARK_INSET_M) * PX_PER_M;
        batch.quad(
            center + forward * nose,
            Vec2::new(MARK_LENGTH_M, pose.width * 0.8) * PX_PER_M,
            bevy_angle,
            FRONT_COLOR,
        );
    }
    to.agents.len()
}

fn update_agents(
    runner: Res<StreetRunner>,
    mut views: ResMut<AgentViews>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let views = &mut *views;
    views.drawn = if runner.is_running() {
        draw_agents(
            &mut views.batch,
            runner.view_from(),
            runner.view_to(),
            runner.view_alpha(),
        )
    } else {
        views.batch.clear();
        0
    };
    if let Some(mut mesh) = meshes.get_mut(&views.mesh) {
        views.batch.swap_into(&mut mesh);
    }
}

pub struct AgentViewPlugin;

impl Plugin for AgentViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StreetRunner>()
            .init_resource::<AgentViews>()
            .add_systems(Update, update_agents);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_archetype_has_its_own_colour() {
        let names = [
            "careful", "resident", "commuter", "taxi", "delivery", "careless",
        ];
        for (i, a) in names.iter().enumerate() {
            for b in &names[i + 1..] {
                assert_ne!(archetype_color(a), archetype_color(b), "{a} vs {b}");
            }
        }
    }
}
