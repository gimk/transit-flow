//! The views: the street level and its run drawn as flat shapes. Static things (ground,
//! strips, markings, signs) are built from `level::LoadedLevel`; moving things (cars,
//! riders, walkers) are one batched mesh each, rewritten every frame from the street
//! runner's snapshots. All draw with the same helpers:
//!
//! - `draw::ShapeBuilder` makes a mesh from any mix of polylines, polygons, rectangles and
//!   circles, each with its own colour; `spawn_shape` puts one on screen with the shared
//!   `ViewMaterial` at a depth from `z`.
//! - Positions come from the street in metres with y down: convert with
//!   `level::level_to_world` and `camera::sim_to_bevy` before drawing.

use bevy::prelude::*;

use crate::draw::ShapeBuilder;

pub mod agents;
pub mod batch;
pub mod palette;
pub mod speed_signs;
pub mod street;
pub mod walkers;

/// Draw order: higher z is drawn over lower. Every layer has its own band, so shapes
/// never z-fight; within one band (a single mesh) later shapes cover earlier ones.
pub mod z {
    /// The street level: ground and blocks, strip surfaces, markings and movements, then
    /// obstacles.
    pub const STREET_GROUND: f32 = 4.0;
    pub const STREET_SURFACE: f32 = 4.2;
    pub const STREET_MARKINGS: f32 = 4.4;
    pub const STREET_THINGS: f32 = 4.6;
    /// Signal marks at the approaches of signalled junctions (play mode): over the street.
    pub const SIGNALS: f32 = 5.0;
    /// The street run's agents: over the street level.
    pub const AGENTS: f32 = 6.0;
    /// The street run's pedestrians: just over the cars.
    pub const WALKERS: f32 = 6.2;
    /// The marks of conflicts and contacts of a run (play mode), over the people.
    pub const INCIDENTS: f32 = 6.4;
    /// The ghost of what the active play tool would do, and the outline of the hovered agent.
    pub const PLAY_PREVIEW: f32 = 6.6;
    pub const HOVER_OUTLINE: f32 = 6.8;
}

/// The one material every shape uses: white, so the mesh's vertex colours show as they are
/// (alpha blended).
#[derive(Resource, Clone, Debug)]
pub struct ViewMaterial(pub Handle<ColorMaterial>);

impl FromWorld for ViewMaterial {
    fn from_world(world: &mut World) -> Self {
        let mut materials = world.resource_mut::<Assets<ColorMaterial>>();
        ViewMaterial(materials.add(ColorMaterial::default()))
    }
}

/// Puts a built shape on screen at depth `z`. Returns the entity (add components, or
/// despawn it when the thing it shows goes away).
pub fn spawn_shape(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &ViewMaterial,
    shape: ShapeBuilder,
    z: f32,
) -> Entity {
    commands
        .spawn((
            Mesh2d(meshes.add(shape.build())),
            MeshMaterial2d(material.0.clone()),
            Transform::from_xyz(0.0, 0.0, z),
        ))
        .id()
}

/// Everything the static map shows.
pub struct ViewsPlugin;

impl Plugin for ViewsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewMaterial>().add_plugins((
            street::StreetViewPlugin,
            speed_signs::SpeedSignPlugin,
            agents::AgentViewPlugin,
            walkers::WalkerViewPlugin,
        ));
    }
}
