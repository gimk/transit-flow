//! Traffic lights (`view/intersection_renderer.gd`): a short bar across each lane that
//! ends at a light-controlled junction, at its stop line, in the colour of the best light any
//! of that lane's turns has (with protected left turns, the left lane of a road can be green
//! for going straight and red for turning).
//!
//! Where the bars stand depends only on the roads, so they are listed when the roads change
//! (`LightBars`); which colour each shows depends on the snapshot, so one mesh holding all
//! the bars is rewritten from `view_to` whenever a new snapshot arrives.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use tf_sim::ids::NodeId;
use tf_sim::roads::RoadNetwork;
use tf_sim::traffic::traffic_lights::Light;

use super::moving::Batch;
use super::{ViewMaterial, z};
use crate::camera::sim_to_bevy;
use crate::events::{RoadNetworkChanged, SimulationReset};
use crate::runner::SimHandle;
use crate::snapshot::{Color as SimColor, ViewSnapshot};

pub const GREEN: SimColor = [0.262_745_1, 0.627_451, 0.278_431_4, 1.0];
pub const AMBER: SimColor = [0.992_156_9, 0.847_058_8, 0.207_843_14, 1.0];
pub const RED: SimColor = [0.898_039_2, 0.223_529_41, 0.207_843_14, 1.0];
/// Thickness of a bar (px).
pub const BAR_WIDTH: f32 = 4.0;

pub fn light_color(light: Light) -> SimColor {
    match light {
        Light::Green => GREEN,
        Light::Amber => AMBER,
        Light::Red => RED,
    }
}

/// One stop line: where it is (Bevy world) and which signal phases its lane's turns use.
#[derive(Clone, Debug, PartialEq)]
pub struct LightBar {
    pub node: NodeId,
    pub center: Vec2,
    pub length: f32,
    /// Bevy-world angle of the bar (across the lane).
    pub angle: f32,
    pub phases: Vec<i32>,
}

/// Every stop line of every light-controlled junction.
pub fn light_bars(roads: &RoadNetwork) -> Vec<LightBar> {
    let mut bars = Vec::new();
    let mut nodes: Vec<_> = roads.nodes.values().filter(|n| n.has_lights()).collect();
    nodes.sort_by_key(|n| n.id);
    for node in nodes {
        for segment_id in &node.segment_ids {
            let Some(segment) = roads.segments.get(segment_id) else {
                continue;
            };
            for lane_id in &segment.lane_ids {
                let Some(lane) = roads.lanes.get(lane_id) else {
                    continue;
                };
                if lane.to_node_id != node.id {
                    continue;
                }
                let (origin, heading) = lane.sample_pose(lane.length);
                let across = Vec2::from_angle(heading).perp();
                bars.push(LightBar {
                    node: node.id,
                    center: sim_to_bevy(origin),
                    length: segment.road_type.lane_width as f32,
                    angle: sim_to_bevy(across).to_angle(),
                    phases: lane
                        .outgoing_lane_ids
                        .iter()
                        .filter_map(|id| roads.lanes.get(id))
                        .map(|connector| connector.signal_phase)
                        .collect(),
                });
            }
        }
    }
    bars
}

/// The best (most permissive) light the bar's lane has in `view`: green over amber over
/// red. A lane without turns shows red, as in the GDScript.
pub fn bar_light(view: &ViewSnapshot, bar: &LightBar) -> Light {
    let mut best = Light::Red;
    for &phase in &bar.phases {
        let light = view.light(bar.node, phase.max(0) as usize);
        if (light as u8) < (best as u8) {
            best = light;
        }
    }
    best
}

#[derive(Resource, Debug)]
pub struct LightViews {
    pub mesh: Handle<Mesh>,
    pub bars: Vec<LightBar>,
    batch: Batch,
    /// What the mesh shows: snapshot address and time, and which roads' bars.
    key: Option<(usize, u64, u64)>,
    bars_version: u64,
}

impl LightViews {
    /// Colour of each bar as last drawn is not stored; ask the snapshot with `bar_light`.
    pub fn bar_count(&self) -> usize {
        self.bars.len()
    }
}

impl FromWorld for LightViews {
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
        world.spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material),
            Transform::from_xyz(0.0, 0.0, z::LIGHTS),
            NoFrustumCulling,
        ));
        Self {
            mesh,
            bars: Vec::new(),
            batch: Batch::default(),
            key: None,
            bars_version: 0,
        }
    }
}

/// Fills `batch` with the bars, each in its current colour.
pub fn draw_bars(batch: &mut Batch, bars: &[LightBar], view: &ViewSnapshot) {
    batch.clear();
    for bar in bars {
        batch.quad(
            bar.center,
            Vec2::new(bar.length, BAR_WIDTH),
            bar.angle,
            light_color(bar_light(view, bar)),
        );
    }
}

fn update_lights(
    mut resets: MessageReader<SimulationReset>,
    mut changes: MessageReader<RoadNetworkChanged>,
    handle: Res<SimHandle>,
    mut views: ResMut<LightViews>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut built: Local<bool>,
) {
    let reset = resets.read().count() > 0;
    let edited = changes.read().count() > 0;
    let first = !std::mem::replace(&mut *built, true);
    let roads_changed = reset || edited || first;
    if roads_changed {
        views.bars = light_bars(&handle.roads().roads);
        views.bars_version += 1;
    }
    let view: &Arc<ViewSnapshot> = &handle.view_to;
    let key = (
        Arc::as_ptr(view) as usize,
        view.time.to_bits(),
        views.bars_version,
    );
    if views.key == Some(key) {
        return;
    }
    views.key = Some(key);
    let views = &mut *views;
    draw_bars(&mut views.batch, &views.bars, view);
    if let Some(mut mesh) = meshes.get_mut(&views.mesh) {
        views.batch.swap_into(&mut mesh);
    }
}

pub struct LightViewPlugin;

impl Plugin for LightViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LightViews>()
            .add_systems(Update, update_lights);
    }
}
