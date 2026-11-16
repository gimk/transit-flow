//! The street run's pedestrians (pivot plan Phase 3): each walker a filled circle of its
//! real radius in metres (drawn at `PX_PER_M`), with a small dark mark toward the side it
//! walks to, so the heading reads. **One entity, one mesh**, rewritten in place every
//! frame from the snapshots around the `StreetRunner`'s render clock, like `agents`.
//!
//! The colour tells the archetype, so the crowd of a level is readable at a glance:
//! teal for a careful walker, indigo for a commuter, olive for a resident, pink for a
//! child, brown for an older person, orange for a hurried one (the warm colour, like the
//! careless driver). A walker waiting at a kerb is darker.
//!
//! A body of 0.25 m is 2 px at the default framing of a level: small, but the circle is
//! drawn with 12 sides, so it still reads at the zoom steps that show one junction arm.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;

use super::agents::WAITING_SHADE;
use super::moving::{Batch, empty_batch_mesh};
use super::{ViewMaterial, z};
use crate::camera::sim_to_bevy;
use crate::level::{PX_PER_M, level_to_world};
use crate::snapshot::{Color as SimColor, lerp_angle};
use crate::street_runner::{StreetRunner, StreetSnapshot};

pub const WALK_CAREFUL_COLOR: SimColor = [0.10, 0.62, 0.62, 1.0];
pub const WALK_COMMUTER_COLOR: SimColor = [0.42, 0.30, 0.72, 1.0];
pub const WALK_RESIDENT_COLOR: SimColor = [0.68, 0.72, 0.30, 1.0];
pub const WALK_CHILD_COLOR: SimColor = [0.95, 0.45, 0.75, 1.0];
pub const WALK_ELDERLY_COLOR: SimColor = [0.62, 0.45, 0.30, 1.0];
pub const WALK_HURRIED_COLOR: SimColor = [0.98, 0.58, 0.10, 1.0];
/// The mark of the side a walker goes to.
pub const WALK_MARK_COLOR: SimColor = [0.1, 0.1, 0.12, 1.0];
/// Circle sides. 12 keeps a 0.25 m body round at the high zoom steps.
const SIDES: u32 = 12;
/// The mark sits this far out along the heading, as a share of the radius, and its size
/// along / across the heading, metres.
const MARK_OFFSET: f32 = 0.75;
const MARK_LENGTH_M: f32 = 0.15;
const MARK_WIDTH_SHARE: f32 = 1.2;

/// The colour of a walker archetype's name (`Walker::archetype`); grey for an unknown one.
pub fn walker_color(archetype: &str) -> SimColor {
    match archetype {
        "careful walker" => WALK_CAREFUL_COLOR,
        "commuter on foot" => WALK_COMMUTER_COLOR,
        "resident on foot" => WALK_RESIDENT_COLOR,
        "child" => WALK_CHILD_COLOR,
        "older person" => WALK_ELDERLY_COLOR,
        "hurried walker" => WALK_HURRIED_COLOR,
        _ => WALK_COMMUTER_COLOR,
    }
}

/// The batched mesh and its buffer.
#[derive(Resource, Debug)]
pub struct WalkerView {
    pub mesh: Handle<Mesh>,
    batch: Batch,
    /// Walkers drawn in the last frame.
    pub drawn: usize,
}

impl FromWorld for WalkerView {
    fn from_world(world: &mut World) -> Self {
        let material = world.resource::<ViewMaterial>().0.clone();
        let mesh = world.resource_mut::<Assets<Mesh>>().add(empty_batch_mesh());
        world.spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material),
            Transform::from_xyz(0.0, 0.0, z::WALKERS),
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

/// Fills `batch` with the walkers of `to`, `alpha` of the way from their pose in `from`.
/// Walkers new in `to` stand where `to` has them; those only in `from` are gone. Returns
/// how many walkers were drawn.
pub fn draw_walkers(
    batch: &mut Batch,
    from: &StreetSnapshot,
    to: &StreetSnapshot,
    alpha: f32,
) -> usize {
    batch.clear();
    for pose in &to.walkers {
        let (pos, dir) = match from.walker_index_of(pose.id) {
            Some(j) => {
                let before = &from.walkers[j];
                (
                    before.pos.lerp(pose.pos, alpha),
                    lerp_dir(before.dir, pose.dir, alpha),
                )
            }
            None => (pose.pos, pose.dir),
        };
        let base = walker_color(pose.archetype);
        let color = if pose.waiting {
            [
                base[0] * WAITING_SHADE,
                base[1] * WAITING_SHADE,
                base[2] * WAITING_SHADE,
                base[3],
            ]
        } else {
            base
        };
        let center = sim_to_bevy(level_to_world(pos));
        let radius = pose.radius * PX_PER_M;
        batch.circle(center, radius, SIDES, color);
        if dir != Vec2::ZERO {
            // The sim turns clockwise on screen (y down); the Bevy world is y up.
            let angle = -dir.to_angle();
            let forward = Vec2::from_angle(angle);
            batch.quad(
                center + forward * (radius * MARK_OFFSET),
                Vec2::new(MARK_LENGTH_M, pose.radius * MARK_WIDTH_SHARE) * PX_PER_M,
                angle,
                WALK_MARK_COLOR,
            );
        }
    }
    to.walkers.len()
}

/// The heading half way between two poses: the same angle when the walker keeps walking
/// the same way, and the one it had when the other pose has none (a body standing still
/// keeps the mark of the side it goes to).
fn lerp_dir(from: Vec2, to: Vec2, alpha: f32) -> Vec2 {
    match (from == Vec2::ZERO, to == Vec2::ZERO) {
        (false, false) => {
            let angle = lerp_angle(from.to_angle(), to.to_angle(), alpha);
            Vec2::from_angle(angle)
        }
        (true, false) => to,
        (false, true) => from,
        (true, true) => Vec2::ZERO,
    }
}

fn update_walkers(
    runner: Res<StreetRunner>,
    mut views: ResMut<WalkerView>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let views = &mut *views;
    views.drawn = if runner.is_running() {
        draw_walkers(
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

pub struct WalkerViewPlugin;

impl Plugin for WalkerViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WalkerView>()
            .add_systems(Update, update_walkers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::agents::archetype_color;

    const WALKERS: [&str; 6] = [
        "careful walker",
        "commuter on foot",
        "resident on foot",
        "child",
        "older person",
        "hurried walker",
    ];
    const DRIVERS: [&str; 6] = [
        "careful", "resident", "commuter", "taxi", "delivery", "careless",
    ];

    #[test]
    fn every_walker_archetype_has_its_own_colour() {
        for (i, a) in WALKERS.iter().enumerate() {
            for b in &WALKERS[i + 1..] {
                assert_ne!(walker_color(a), walker_color(b), "{a} vs {b}");
            }
        }
    }

    #[test]
    fn a_walker_colour_is_never_a_car_colour() {
        for walker in WALKERS {
            for driver in DRIVERS {
                assert_ne!(
                    walker_color(walker),
                    archetype_color(driver),
                    "{walker} / {driver}"
                );
            }
        }
    }
}
