//! Roads (`view/road_renderer.gd`): per segment a sidewalk band, a kerb band, the asphalt,
//! a tint along bus lanes and thin lane lines; per node discs at the same three levels, so
//! roads meeting there join without gaps. Each level has its own depth (`z::ROAD_*`): every
//! road's asphalt covers every road's kerb, so a minor road never cuts into a major one.
//!
//! Rebuilt by id: a `RoadNetworkChanged` names the segments and nodes that changed (ids
//! may be of things already gone); only those entities are despawned and built again.
//! `SimulationReset` rebuilds everything. Not drawn: GDScript's inside-corner fillets, road
//! markings art, grain, and elevation layers (the GDScript doesn't stack them either).

use std::collections::HashMap;

use bevy::prelude::*;
use tf_sim::city::lot_system::ROAD_CLEARANCE;
use tf_sim::ids::{NodeId, SegmentId};
use tf_sim::roads::lane_builder::{get_polyline, offset_polyline, trim_polyline};
use tf_sim::roads::{RoadNetwork, RoadNode, RoadSegment};

use super::{ViewMaterial, spawn_shape, z};
use crate::camera::sim_to_bevy;
use crate::draw::{ShapeBuilder, srgba};
use crate::events::{RoadNetworkChanged, SimulationReset};
use crate::runner::SimHandle;

/// Sidewalk beyond the kerb (lots start at the same distance from the road's edge).
pub const SIDEWALK_WIDTH: f32 = ROAD_CLEARANCE as f32;
/// Kerb band beyond the asphalt, each side.
pub const KERB_WIDTH: f32 = 1.5;
pub const SIDEWALK_COLOR: Color = Color::srgb(0.46, 0.47, 0.44);
pub const KERB_COLOR: Color = Color::srgb(0.62, 0.62, 0.59);
pub const BUS_LANE_COLOR: Color = Color::srgba(0.75, 0.22, 0.20, 0.7);
pub const CENTER_LINE_COLOR: Color = Color::srgb(0.93, 0.78, 0.25);
pub const LANE_LINE_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.55);
pub const CENTER_LINE_WIDTH: f32 = 1.4;
pub const LANE_LINE_WIDTH: f32 = 0.9;
pub const NODE_DISC_SEGMENTS: u32 = 24;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadSegmentView(pub SegmentId);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadNodeView(pub NodeId);

/// What is on screen, so a change touches only its own entities.
#[derive(Resource, Default, Debug)]
pub struct RoadViews {
    segments: HashMap<SegmentId, SegmentEntry>,
    nodes: HashMap<NodeId, Vec<Entity>>,
    /// How many segments and nodes were built (not counting despawns) so far: tests use it
    /// to check that an edit rebuilds only what it touched.
    pub segments_built: usize,
    pub nodes_built: usize,
}

#[derive(Debug)]
struct SegmentEntry {
    entities: Vec<Entity>,
    /// Its end nodes when built: their discs depend on it.
    nodes: [NodeId; 2],
}

impl RoadViews {
    pub fn segment_entities(&self, id: SegmentId) -> &[Entity] {
        self.segments.get(&id).map_or(&[], |e| &e.entities)
    }

    pub fn segment_ids(&self) -> impl Iterator<Item = SegmentId> + '_ {
        self.segments.keys().copied()
    }

    pub fn node_entities(&self, id: NodeId) -> &[Entity] {
        self.nodes.get(&id).map_or(&[], |e| e.as_slice())
    }
}

pub struct RoadViewPlugin;

impl Plugin for RoadViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RoadViews>()
            .add_systems(Update, update_roads);
    }
}

/// The shapes of one segment, one builder per depth.
#[derive(Default)]
pub struct SegmentShapes {
    pub sidewalk: ShapeBuilder,
    pub edge: ShapeBuilder,
    pub surface: ShapeBuilder,
    pub bus_lanes: ShapeBuilder,
    pub markings: ShapeBuilder,
}

fn to_bevy(points: &[Vec2]) -> Vec<Vec2> {
    points.iter().map(|&p| sim_to_bevy(p)).collect()
}

/// Bands along the centerline, lane lines between lanes (the centre line yellow on a
/// two-way road) cut back at junctions, and a tint over each bus lane.
pub fn segment_shapes(roads: &RoadNetwork, segment: &RoadSegment) -> SegmentShapes {
    let mut shapes = SegmentShapes::default();
    let points = get_polyline(segment.centerline());
    if points.len() < 2 {
        return shapes;
    }
    let road_type = segment.road_type;
    let width = road_type.total_width() as f32;
    let line = to_bevy(&points);
    shapes
        .sidewalk
        .polyline(&line, width + SIDEWALK_WIDTH * 2.0, SIDEWALK_COLOR);
    shapes.edge.polyline(
        &line,
        width + KERB_WIDTH * 2.0,
        if road_type.edge_color[3] > 0.0 {
            srgba(road_type.edge_color)
        } else {
            KERB_COLOR
        },
    );
    shapes
        .surface
        .polyline(&line, width, srgba(road_type.surface_color));

    // Lane lines, between lanes, in the part clear of the junctions.
    let trimmed = trim_polyline(
        &points,
        roads.junction_radius(segment.start_node_id),
        roads.junction_radius(segment.end_node_id),
    );
    if trimmed.len() >= 2 {
        let lanes = road_type.lane_count();
        let lane_width = road_type.lane_width as f32;
        for k in 1..lanes {
            let offset = k as f32 * lane_width - width * 0.5;
            let (color, w) = if !road_type.one_way && k == lanes / 2 {
                (CENTER_LINE_COLOR, CENTER_LINE_WIDTH)
            } else {
                (LANE_LINE_COLOR, LANE_LINE_WIDTH)
            };
            shapes.markings.polyline(
                &to_bevy(&offset_polyline(&trimmed, f64::from(offset))),
                w,
                color,
            );
        }
    }
    for lane_id in &segment.lane_ids {
        if let Some(lane) = roads.lanes.get(lane_id)
            && lane.bus_only
        {
            shapes.bus_lanes.polyline(
                &to_bevy(&get_polyline(lane.curve())),
                road_type.lane_width as f32,
                BUS_LANE_COLOR,
            );
        }
    }
    shapes
}

/// Discs over a node (sidewalk, kerb, asphalt) as wide as its widest road, in that road's
/// colours. `None` for a node no road reaches.
pub fn node_shapes(roads: &RoadNetwork, node: &RoadNode) -> Option<[ShapeBuilder; 3]> {
    let widest = node
        .segment_ids
        .iter()
        .filter_map(|id| roads.segments.get(id))
        .max_by(|a, b| {
            a.road_type
                .total_width()
                .total_cmp(&b.road_type.total_width())
        })?;
    let radius = widest.road_type.total_width() as f32 * 0.5;
    let center = sim_to_bevy(node.position);
    let mut sidewalk = ShapeBuilder::new();
    let mut edge = ShapeBuilder::new();
    let mut surface = ShapeBuilder::new();
    sidewalk.circle(
        center,
        radius + SIDEWALK_WIDTH,
        NODE_DISC_SEGMENTS,
        SIDEWALK_COLOR,
    );
    edge.circle(
        center,
        radius + KERB_WIDTH,
        NODE_DISC_SEGMENTS,
        srgba(widest.road_type.edge_color),
    );
    surface.circle(
        center,
        radius,
        NODE_DISC_SEGMENTS,
        srgba(widest.road_type.surface_color),
    );
    Some([sidewalk, edge, surface])
}

#[allow(clippy::too_many_arguments)]
fn update_roads(
    mut resets: MessageReader<SimulationReset>,
    mut changes: MessageReader<RoadNetworkChanged>,
    handle: Res<SimHandle>,
    mut views: ResMut<RoadViews>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
    mut built: Local<bool>,
) {
    let reset = resets.read().count() > 0 || !*built;
    *built = true;
    let mut segment_ids: Vec<SegmentId> = Vec::new();
    let mut node_ids: Vec<NodeId> = Vec::new();
    for change in changes.read() {
        segment_ids.extend(&change.segment_ids);
        node_ids.extend(&change.node_ids);
    }
    if !reset && segment_ids.is_empty() && node_ids.is_empty() {
        return;
    }
    let published = handle.roads();
    let roads = &published.roads;
    if reset {
        for entry in views.segments.drain().map(|(_, e)| e.entities) {
            entry.into_iter().for_each(|e| commands.entity(e).despawn());
        }
        for entities in views.nodes.drain().map(|(_, e)| e) {
            entities
                .into_iter()
                .for_each(|e| commands.entity(e).despawn());
        }
        segment_ids = roads.segments.keys().copied().collect();
        node_ids = roads.nodes.keys().copied().collect();
    }
    segment_ids.sort();
    segment_ids.dedup();

    // Segments: drop the old entities, build again if still there.
    for &id in &segment_ids {
        if let Some(old) = views.segments.remove(&id) {
            old.entities
                .into_iter()
                .for_each(|e| commands.entity(e).despawn());
            // The discs at its ends may have changed width.
            node_ids.extend(old.nodes);
        }
        let Some(segment) = roads.segments.get(&id) else {
            continue;
        };
        node_ids.extend([segment.start_node_id, segment.end_node_id]);
        let shapes = segment_shapes(roads, segment);
        let mut entities = Vec::new();
        for (shape, depth) in [
            (shapes.sidewalk, z::ROAD_SIDEWALK),
            (shapes.edge, z::ROAD_EDGE),
            (shapes.surface, z::ROAD_SURFACE),
            (shapes.bus_lanes, z::ROAD_BUS_LANE),
            (shapes.markings, z::ROAD_MARKINGS),
        ] {
            if shape.is_empty() {
                continue;
            }
            let entity = spawn_shape(&mut commands, &mut meshes, &material, shape, depth);
            commands.entity(entity).insert(RoadSegmentView(id));
            entities.push(entity);
        }
        views.segments_built += 1;
        views.segments.insert(
            id,
            SegmentEntry {
                entities,
                nodes: [segment.start_node_id, segment.end_node_id],
            },
        );
    }

    // Nodes: the discs of every node an edit touched.
    node_ids.sort();
    node_ids.dedup();
    for id in node_ids {
        if let Some(old) = views.nodes.remove(&id) {
            old.into_iter().for_each(|e| commands.entity(e).despawn());
        }
        let Some(shapes) = roads.nodes.get(&id).and_then(|n| node_shapes(roads, n)) else {
            continue;
        };
        let mut entities = Vec::new();
        for (shape, depth) in
            shapes
                .into_iter()
                .zip([z::ROAD_SIDEWALK, z::ROAD_EDGE, z::ROAD_SURFACE])
        {
            let entity = spawn_shape(&mut commands, &mut meshes, &material, shape, depth);
            commands.entity(entity).insert(RoadNodeView(id));
            entities.push(entity);
        }
        views.nodes_built += 1;
        views.nodes.insert(id, entities);
    }
}
