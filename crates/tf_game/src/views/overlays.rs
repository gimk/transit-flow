//! Debug overlays (`view/lane_overlay.gd`, `view/heatmap_overlay.gd`), both toggled with a
//! `DebugOverlayToggleRequested` message (F3 "lanes", F4 "heatmap") and answering with
//! `DebugOverlayChanged`.
//!
//! - **Lanes** (F3): every lane as a thin line with direction arrows and every node as a dot,
//!   drawn with gizmos (constant pixel width at any zoom). Forward lanes cyan, backward
//!   lanes orange, junction connectors violet. The lines are listed once per road version.
//! - **Heatmap** (F4): road lanes coloured by how much slower than free flow traffic moves
//!   there, from yellow to deep red, as a mesh under the vehicles. Free-flowing lanes are not
//!   drawn. The congestion comes from the monitor every 5 sim seconds, and the mesh is
//!   rebuilt when it or the roads change, at most every half real second, and only while
//!   the heatmap is shown.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use indexmap::IndexMap;
use tf_sim::ids::LaneId;
use tf_sim::roads::RoadNetwork;
use tf_sim::roads::lane_builder::get_polyline;

use super::moving::empty_batch_mesh;
use super::{ViewMaterial, z};
use crate::camera::sim_to_bevy;
use crate::draw::ShapeBuilder;
use crate::events::{DebugOverlayChanged, DebugOverlayToggleRequested};
use crate::runner::SimHandle;

pub const LANES_OVERLAY: &str = "lanes";
pub const HEATMAP_OVERLAY: &str = "heatmap";

pub const FORWARD_COLOR: Color = Color::srgba(0.3, 0.9, 1.0, 0.9);
pub const BACKWARD_COLOR: Color = Color::srgba(1.0, 0.65, 0.2, 0.9);
pub const CONNECTOR_COLOR: Color = Color::srgba(0.8, 0.55, 1.0, 0.8);
pub const NODE_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.8);
pub const ARROW_SPACING: f32 = 32.0;
pub const ARROW_SIZE: f32 = 4.0;
pub const NODE_RADIUS: f32 = 3.0;
/// Gizmo line width (screen px).
pub const LANE_LINE_PX: f32 = 1.5;

/// Real seconds between heatmap rebuilds.
pub const HEATMAP_INTERVAL: f64 = 0.5;
/// Congestion (extra travel time / free-flow time) drawn fully red.
pub const FULL_RED: f64 = 4.0;
pub const HEATMAP_WIDTH: f32 = 6.0;
const HEAT_LOW: [f32; 3] = [1.0, 0.85, 0.2];
const HEAT_HIGH: [f32; 3] = [0.85, 0.1, 0.1];
const HEAT_ALPHA: f32 = 0.6;

/// Colour of a lane `congestion` slow (0 free flow).
pub fn heat_color(congestion: f64) -> Color {
    let heat = (congestion / FULL_RED).clamp(0.0, 1.0) as f32;
    let mix = |i: usize| HEAT_LOW[i] + (HEAT_HIGH[i] - HEAT_LOW[i]) * heat;
    Color::srgba(mix(0), mix(1), mix(2), HEAT_ALPHA)
}

/// The heatmap's shapes: one band per congested lane that still exists.
pub fn heatmap_shape(roads: &RoadNetwork, congestion: &IndexMap<LaneId, f64>) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new();
    for (id, &value) in congestion {
        let Some(lane) = roads.lanes.get(id) else {
            continue;
        };
        let points: Vec<Vec2> = get_polyline(lane.curve())
            .into_iter()
            .map(sim_to_bevy)
            .collect();
        shape.polyline(&points, HEATMAP_WIDTH, heat_color(value));
    }
    shape
}

/// A lane's line, an arrow every `ARROW_SPACING`.
#[derive(Clone, Debug, Default)]
pub struct LaneLines {
    pub strips: Vec<(Vec<Vec2>, Color)>,
    /// Arrow triangles (closed strips of 4 points) with their colours.
    pub arrows: Vec<([Vec2; 4], Color)>,
    pub nodes: Vec<Vec2>,
}

pub fn lane_lines(roads: &RoadNetwork) -> LaneLines {
    let mut lines = LaneLines::default();
    let mut ids: Vec<LaneId> = roads.lanes.keys().copied().collect();
    ids.sort();
    for id in ids {
        let lane = &roads.lanes[&id];
        let points: Vec<Vec2> = get_polyline(lane.curve())
            .into_iter()
            .map(sim_to_bevy)
            .collect();
        let Some(segment) = lane.segment_id.and_then(|s| roads.segments.get(&s)) else {
            lines.strips.push((points, CONNECTOR_COLOR));
            continue;
        };
        let color = if lane.from_node_id == segment.start_node_id {
            FORWARD_COLOR
        } else {
            BACKWARD_COLOR
        };
        lines.strips.push((points, color));
        let mut distance = f64::from(ARROW_SPACING) * 0.5;
        while distance < lane.length {
            let (origin, heading) = lane.sample_pose(distance);
            let forward = sim_to_bevy(Vec2::from_angle(heading)) * ARROW_SIZE;
            let side = Vec2::new(-forward.y, forward.x);
            let o = sim_to_bevy(origin);
            let (a, b, c) = (o + forward, o - forward + side, o - forward - side);
            lines.arrows.push(([a, b, c, a], color));
            distance += f64::from(ARROW_SPACING);
        }
    }
    lines.nodes = roads
        .nodes
        .values()
        .map(|n| sim_to_bevy(n.position))
        .collect();
    lines
}

/// F3's state and its lines.
#[derive(Resource, Default, Debug)]
pub struct LaneOverlay {
    pub shown: bool,
    pub lines: LaneLines,
    /// Version of the roads the lines were listed from.
    version: Option<u64>,
}

/// F4's state.
#[derive(Resource, Debug)]
pub struct Heatmap {
    pub shown: bool,
    pub entity: Entity,
    mesh: Handle<Mesh>,
    /// What the mesh was built from: roads version and congestion.
    drawn: Option<(u64, IndexMap<LaneId, f64>)>,
    /// Real time of the last rebuild.
    last_build: f64,
    /// Rebuilds so far (tests).
    pub builds: usize,
}

impl FromWorld for Heatmap {
    fn from_world(world: &mut World) -> Self {
        let material = world.resource::<ViewMaterial>().0.clone();
        let mesh = world.resource_mut::<Assets<Mesh>>().add(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3])
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3])
            .with_inserted_indices(Indices::U32(vec![0, 1, 2])),
        );
        let entity = world
            .spawn((
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material),
                Transform::from_xyz(0.0, 0.0, z::HEATMAP),
                Visibility::Hidden,
                NoFrustumCulling,
            ))
            .id();
        Self {
            shown: false,
            entity,
            mesh,
            drawn: None,
            last_build: f64::NEG_INFINITY,
            builds: 0,
        }
    }
}

fn toggle_overlays(
    mut requests: MessageReader<DebugOverlayToggleRequested>,
    mut lanes: ResMut<LaneOverlay>,
    mut heatmap: ResMut<Heatmap>,
    mut visibility: Query<&mut Visibility>,
    mut changed: MessageWriter<DebugOverlayChanged>,
) {
    for request in requests.read() {
        match request.0.as_str() {
            LANES_OVERLAY => {
                lanes.shown = !lanes.shown;
                changed.write(DebugOverlayChanged {
                    overlay_id: LANES_OVERLAY.into(),
                    shown: lanes.shown,
                });
            }
            HEATMAP_OVERLAY => {
                heatmap.shown = !heatmap.shown;
                if let Ok(mut v) = visibility.get_mut(heatmap.entity) {
                    *v = if heatmap.shown {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                }
                // Draw at once when shown, not up to half a second later.
                heatmap.last_build = f64::NEG_INFINITY;
                changed.write(DebugOverlayChanged {
                    overlay_id: HEATMAP_OVERLAY.into(),
                    shown: heatmap.shown,
                });
            }
            _ => {}
        }
    }
}

fn update_lane_lines(handle: Res<SimHandle>, mut overlay: ResMut<LaneOverlay>) {
    if !overlay.shown {
        return;
    }
    let roads = handle.roads();
    if overlay.version != Some(roads.version) {
        overlay.version = Some(roads.version);
        overlay.lines = lane_lines(&roads.roads);
    }
}

fn update_heatmap(
    handle: Res<SimHandle>,
    time: Res<Time<Real>>,
    mut heatmap: ResMut<Heatmap>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let now = time.elapsed_secs_f64();
    if !heatmap.shown || now - heatmap.last_build < HEATMAP_INTERVAL {
        return;
    }
    let roads = handle.roads();
    let congestion = &handle.view_to.congestion;
    if let Some((version, drawn)) = &heatmap.drawn
        && *version == roads.version
        && drawn == congestion
    {
        return;
    }
    heatmap.last_build = now;
    heatmap.drawn = Some((roads.version, congestion.clone()));
    heatmap.builds += 1;
    let shape = heatmap_shape(&roads.roads, congestion);
    // Bevy warns about a mesh without vertices on every frame.
    let mesh = if shape.is_empty() {
        empty_batch_mesh()
    } else {
        shape.build()
    };
    if let Some(mut slot) = meshes.get_mut(&heatmap.mesh) {
        *slot = mesh;
    }
}

/// F3, drawn every frame with gizmos while shown.
fn draw_lane_overlay(overlay: Res<LaneOverlay>, mut gizmos: Gizmos) {
    if !overlay.shown {
        return;
    }
    let lines = &overlay.lines;
    for (points, color) in &lines.strips {
        gizmos.linestrip_2d(points.iter().copied(), *color);
    }
    for (arrow, color) in &lines.arrows {
        gizmos.linestrip_2d(arrow.iter().copied(), *color);
    }
    for &node in &lines.nodes {
        gizmos.circle_2d(node, NODE_RADIUS, NODE_COLOR);
    }
}

pub struct OverlayViewPlugin;

impl Plugin for OverlayViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LaneOverlay>()
            .init_resource::<Heatmap>()
            .add_systems(
                Update,
                (toggle_overlays, update_lane_lines, update_heatmap).chain(),
            );
        // Gizmos exist only in the full app (tests run without rendering).
        if app.is_plugin_added::<bevy::gizmos::GizmoPlugin>() {
            app.add_systems(Update, draw_lane_overlay.after(update_lane_lines))
                .add_systems(Startup, |mut store: ResMut<GizmoConfigStore>| {
                    store.config_mut::<DefaultGizmoConfigGroup>().0.line.width = LANE_LINE_PX;
                });
        }
    }
}
