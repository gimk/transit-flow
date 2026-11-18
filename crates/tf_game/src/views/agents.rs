//! The street run's agents (pivot plan Phase 2): each car an oriented rectangle in
//! metres (drawn at `PX_PER_M`), with a small dark mark at the front so the heading shows.
//! **One entity, one mesh**, rewritten in place every frame from the snapshots around the
//! `StreetRunner`'s render clock, like `moving`.
//!
//! The colour tells the driving style, so a mix of drivers is readable at a glance: careful
//! blue, resident green-grey, commuter grey, taxi yellow, delivery white, careless red.
//! A waiting agent is darker, a crashed one magenta.
//!
//! A rider (`Mode::Bicycle`) is drawn as its own shape in the same mesh: the narrow body of
//! the bicycle (1.8 m x 0.6 m, the size the run gives it) with the rider as a dark dot on
//! it, and the same light front mark as a car so the heading reads. Its colours are a violet
//! family, so a rider never reads as a car or as a walker, and its body is at the side
//! position the run gives it (`AgentPose::lateral`, already in `pos`).

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;
use tf_sim::street::level::Mode;

use super::batch::{Batch, Rgba, empty_batch_mesh, lerp_angle};
use super::{ViewMaterial, z};
use crate::camera::sim_to_bevy;
use crate::level::{PX_PER_M, level_to_world};
use crate::street_runner::{StreetRunner, StreetSnapshot};

// Darker and more saturated than the light map (`palette`), so a car reads on the blue-grey
// carriageway; delivery is charcoal because white would vanish on it.
pub const CAREFUL_COLOR: Rgba = [0.10, 0.36, 0.84, 1.0];
pub const RESIDENT_COLOR: Rgba = [0.12, 0.55, 0.34, 1.0];
pub const COMMUTER_COLOR: Rgba = [0.36, 0.40, 0.48, 1.0];
pub const TAXI_COLOR: Rgba = [0.94, 0.70, 0.02, 1.0];
pub const DELIVERY_COLOR: Rgba = [0.17, 0.18, 0.23, 1.0];
pub const CARELESS_COLOR: Rgba = [0.86, 0.10, 0.10, 1.0];
pub const CRASHED_COLOR: Rgba = [1.0, 0.0, 1.0, 1.0];
/// The front mark (the windscreen): light, so it shows on every body colour.
pub const FRONT_COLOR: Rgba = [0.94, 0.95, 0.97, 1.0];
/// A waiting agent's colour is multiplied by this.
pub const WAITING_SHADE: f32 = 0.6;
/// The front mark's length (along the heading) and its inset from the nose, metres.
const MARK_LENGTH_M: f32 = 0.5;
const MARK_INSET_M: f32 = 0.3;

// The riders: one violet family, brighter and more saturated than the walkers' indigo, so a
// rider reads as a rider and never as a car (blue, green, grey, yellow, charcoal, red) or as
// a walker (teal, indigo, olive, pink, brown, orange). The archetype is the shade.
pub const RIDE_CAREFUL_COLOR: Rgba = [0.58, 0.18, 0.92, 1.0];
pub const RIDE_TOURIST_COLOR: Rgba = [0.78, 0.42, 1.00, 1.0];
pub const RIDE_COMMUTER_COLOR: Rgba = [0.50, 0.05, 0.85, 1.0];
pub const RIDE_DELIVERY_COLOR: Rgba = [0.68, 0.25, 0.95, 1.0];
pub const RIDE_CARGO_COLOR: Rgba = [0.80, 0.60, 0.98, 1.0];
pub const RIDE_CARELESS_COLOR: Rgba = [0.92, 0.08, 0.72, 1.0];
/// The rider on the bicycle: dark, like the walkers' heading mark.
pub const RIDER_COLOR: Rgba = [0.10, 0.08, 0.14, 1.0];
/// The rider's dot: its radius, metres.
const RIDER_RADIUS_M: f32 = 0.22;
/// The rider's dot sits this far forward of the middle of the body, metres (a rider sits
/// over the middle of a bicycle, a little forward).
const RIDER_OFFSET_M: f32 = 0.15;
/// The front mark of a rider: shorter than a car's, and this share of the body's width.
const RIDER_MARK_LENGTH_M: f32 = 0.3;
const RIDER_MARK_WIDTH_SHARE: f32 = 0.7;
/// Sides of the rider's dot: 10 keeps it round at the high zoom steps.
const RIDER_SIDES: u32 = 10;

/// The colour of an archetype's name (`Agent::archetype`); grey for an unknown one.
pub fn archetype_color(archetype: &str) -> Rgba {
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

/// The colour of a rider archetype's name (`Agent::archetype` of a `Mode::Bicycle` agent);
/// the careful rider's violet for an unknown one.
pub fn rider_color(archetype: &str) -> Rgba {
    match archetype {
        "careful cyclist" => RIDE_CAREFUL_COLOR,
        "velib tourist" => RIDE_TOURIST_COLOR,
        "bike commuter" => RIDE_COMMUTER_COLOR,
        "delivery rider" => RIDE_DELIVERY_COLOR,
        "cargo bike" => RIDE_CARGO_COLOR,
        "careless cyclist" => RIDE_CARELESS_COLOR,
        _ => RIDE_CAREFUL_COLOR,
    }
}

/// What `draw_agents` put in the batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drawn {
    /// Every agent drawn (cars, vans, buses and riders).
    pub agents: usize,
    /// The riders among them.
    pub riders: usize,
}

/// The batched mesh and its buffer.
#[derive(Resource, Debug)]
pub struct AgentViews {
    pub mesh: Handle<Mesh>,
    batch: Batch,
    /// Agents drawn in the last frame.
    pub drawn: usize,
    /// Riders drawn in the last frame.
    pub riders: usize,
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
            riders: 0,
        }
    }
}

/// Fills `batch` with the agents of `to`, `alpha` of the way from their pose in `from`.
/// Agents new in `to` stand where `to` has them; those only in `from` are gone.
pub fn draw_agents(
    batch: &mut Batch,
    from: &StreetSnapshot,
    to: &StreetSnapshot,
    alpha: f32,
) -> Drawn {
    batch.clear();
    let mut drawn = Drawn::default();
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
        let rider = pose.mode == Mode::Bicycle;
        let base = if rider {
            rider_color(pose.archetype)
        } else {
            archetype_color(pose.archetype)
        };
        let color = if pose.crashed {
            CRASHED_COLOR
        } else if pose.waiting {
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
        // The sim turns clockwise on screen (y down); the Bevy world is y up.
        let bevy_angle = -angle;
        let forward = Vec2::from_angle(bevy_angle);
        let size = Vec2::new(pose.length, pose.width) * PX_PER_M;
        batch.quad(center, size, bevy_angle, color);
        if rider {
            // The rider on the bicycle, then a short front mark so the heading reads.
            batch.circle(
                center + forward * (RIDER_OFFSET_M * PX_PER_M),
                RIDER_RADIUS_M * PX_PER_M,
                RIDER_SIDES,
                RIDER_COLOR,
            );
            let nose = (pose.length * 0.5 - RIDER_MARK_LENGTH_M * 0.5) * PX_PER_M;
            batch.quad(
                center + forward * nose,
                Vec2::new(RIDER_MARK_LENGTH_M, pose.width * RIDER_MARK_WIDTH_SHARE) * PX_PER_M,
                bevy_angle,
                FRONT_COLOR,
            );
            drawn.riders += 1;
        } else {
            let nose = (pose.length * 0.5 - MARK_INSET_M) * PX_PER_M;
            batch.quad(
                center + forward * nose,
                Vec2::new(MARK_LENGTH_M, pose.width * 0.8) * PX_PER_M,
                bevy_angle,
                FRONT_COLOR,
            );
        }
        drawn.agents += 1;
    }
    drawn
}

fn update_agents(
    runner: Res<StreetRunner>,
    mut views: ResMut<AgentViews>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let views = &mut *views;
    let drawn = if runner.is_running() {
        draw_agents(
            &mut views.batch,
            runner.view_from(),
            runner.view_to(),
            runner.view_alpha(),
        )
    } else {
        views.batch.clear();
        Drawn::default()
    };
    views.drawn = drawn.agents;
    views.riders = drawn.riders;
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
    use crate::views::walkers::walker_color;

    const DRIVERS: [&str; 6] = [
        "careful", "resident", "commuter", "taxi", "delivery", "careless",
    ];
    const RIDERS: [&str; 6] = [
        "careful cyclist",
        "velib tourist",
        "bike commuter",
        "delivery rider",
        "cargo bike",
        "careless cyclist",
    ];
    const WALKERS: [&str; 6] = [
        "careful walker",
        "commuter on foot",
        "resident on foot",
        "child",
        "older person",
        "hurried walker",
    ];

    #[test]
    fn every_archetype_has_its_own_colour() {
        for (i, a) in DRIVERS.iter().enumerate() {
            for b in &DRIVERS[i + 1..] {
                assert_ne!(archetype_color(a), archetype_color(b), "{a} vs {b}");
            }
        }
    }

    #[test]
    fn every_rider_archetype_has_its_own_colour() {
        for (i, a) in RIDERS.iter().enumerate() {
            for b in &RIDERS[i + 1..] {
                assert_ne!(rider_color(a), rider_color(b), "{a} vs {b}");
            }
        }
    }

    #[test]
    fn a_rider_colour_is_never_a_car_or_a_walker_colour() {
        for rider in RIDERS {
            for driver in DRIVERS {
                assert_ne!(
                    rider_color(rider),
                    archetype_color(driver),
                    "{rider} / {driver}"
                );
            }
            for walker in WALKERS {
                assert_ne!(
                    rider_color(rider),
                    walker_color(walker),
                    "{rider} / {walker}"
                );
            }
        }
    }
}
