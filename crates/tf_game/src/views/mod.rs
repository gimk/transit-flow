//! The views: the simulation drawn as flat shapes (plan §2.3, M9). Static things (ground,
//! grid, roads, lots, zones, highway markers) live here; they rebuild only what a change
//! message names. Moving things, transit, lights and overlays (M9.2, M9.3) are further
//! plugins added to `ViewsPlugin`, drawing with the same helpers:
//!
//! - `draw::ShapeBuilder` makes a mesh from any mix of polylines, polygons, rectangles and
//!   circles, each with its own colour; `spawn_shape` puts one on screen with the shared
//!   `ViewMaterial` at a depth from `z`.
//! - Views read the sim through `SimHandle` (`roads()` never waits; `get_mut()` waits for the
//!   worker, which is fine once per change message but not every frame).
//! - Positions come from the sim in pixels with y down: convert with
//!   `camera::sim_to_bevy` before drawing.

use bevy::prelude::*;

use crate::draw::ShapeBuilder;

pub mod agents;
pub mod externals;
pub mod grid;
pub mod ground;
pub mod lights;
pub mod lots;
pub mod moving;
pub mod overlays;
pub mod palette;
pub mod roads;
pub mod speed_signs;
pub mod street;
pub mod tool_preview;
pub mod transit;
pub mod walkers;
pub mod zones;

/// Draw order: higher z is drawn over lower. Every layer has its own band, so shapes
/// never z-fight; within one band (a single mesh) later shapes cover earlier ones.
/// M9.2 and M9.3 fill in the bands above the map.
pub mod z {
    pub const GROUND: f32 = 0.0;
    /// The map border and the building grid.
    pub const GRID: f32 = 1.0;
    /// Zoned cells without a building.
    pub const ZONES: f32 = 2.0;
    /// Buildings and their parking.
    pub const LOTS: f32 = 3.0;
    /// The street game's level (pivot plan): ground and blocks, strip surfaces, markings
    /// and movements, then obstacles. Under the old roads, which a level never has.
    pub const STREET_GROUND: f32 = 4.0;
    pub const STREET_SURFACE: f32 = 4.2;
    pub const STREET_MARKINGS: f32 = 4.4;
    pub const STREET_THINGS: f32 = 4.6;
    /// A road is drawn in layers, so segments meeting at a node blend: the sidewalks of all
    /// roads, then all kerbs, then all asphalt, then bus lanes, then lane lines.
    pub const ROAD_SIDEWALK: f32 = 10.0;
    pub const ROAD_EDGE: f32 = 10.2;
    pub const ROAD_SURFACE: f32 = 10.4;
    pub const ROAD_BUS_LANE: f32 = 10.6;
    pub const ROAD_MARKINGS: f32 = 10.8;
    /// Zebra stripes at bus stops (M9.3).
    pub const CROSSWALKS: f32 = 11.0;
    /// Highway signs on the map edge.
    pub const EXTERNALS: f32 = 12.0;
    /// Cars parked on lots (M9.2).
    pub const PARKED: f32 = 20.0;
    /// Bus lines and stops (M9.3).
    pub const TRANSIT_LINES: f32 = 30.0;
    pub const BUS_STOPS: f32 = 31.0;
    /// The congestion heatmap, under the vehicles (M9.3).
    pub const HEATMAP: f32 = 35.0;
    /// Moving vehicles (M9.2).
    pub const VEHICLES: f32 = 40.0;
    /// Pedestrians and waiting riders (M9.2).
    pub const PEDESTRIANS: f32 = 45.0;
    /// The street run's agents (pivot Phase 2): over the street level, under the old map.
    /// Signal marks at the approaches of signalled junctions (play mode): over the street.
    pub const SIGNALS: f32 = 5.0;
    /// Speed signs painted on the carriageway: over the markings and obstacles, under the
    /// signal marks and the agents.
    pub const SPEED_SIGNS: f32 = 4.8;
    pub const AGENTS: f32 = 6.0;
    /// The street run's pedestrians (pivot Phase 3): just over the cars, under the old
    /// map's parked cars and overlays.
    pub const WALKERS: f32 = 6.2;
    /// The marks of conflicts and contacts of a run (play mode), over the people.
    pub const INCIDENTS: f32 = 6.4;
    /// The ghost of what the active play tool would do, and the outline of the hovered agent.
    pub const PLAY_PREVIEW: f32 = 6.6;
    pub const HOVER_OUTLINE: f32 = 6.8;
    /// Light bars at junctions (M9.3).
    pub const LIGHTS: f32 = 50.0;
    /// Debug overlays: lane overlay, heatmap (M9.3).
    pub const OVERLAYS: f32 = 60.0;
    /// The active tool's preview (M10).
    pub const TOOL: f32 = 70.0;
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
            ground::GroundViewPlugin,
            grid::GridViewPlugin,
            zones::ZoneViewPlugin,
            lots::LotViewPlugin,
            roads::RoadViewPlugin,
            externals::ExternalViewPlugin,
            moving::MovingViewPlugin,
            transit::TransitViewPlugin,
            lights::LightViewPlugin,
            overlays::OverlayViewPlugin,
            tool_preview::ToolPreviewPlugin,
            street::StreetViewPlugin,
            speed_signs::SpeedSignPlugin,
            agents::AgentViewPlugin,
            walkers::WalkerViewPlugin,
        ));
    }
}
