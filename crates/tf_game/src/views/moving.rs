//! The moving views (M9.2): vehicles as rotated rectangles, cars parked on their lots, and
//! pedestrians and waiting riders as dots. Each is **one entity with one mesh**, rewritten in
//! place every frame from the snapshots around the render clock (`SimHandle::view_from` /
//! `view_to` / `view_alpha`): no entity is spawned or despawned per vehicle, and the vertex
//! buffers are swapped into the mesh instead of reallocated.
//!
//! Colours: cars shades of green, trucks brown, buses their line's colour (all from
//! `ViewSnapshot::colors`); F2 (`"lateness"` overlay) paints vehicles green, yellow, red by
//! how late they are. Not drawn: lamps, headlights, indicators, the riders' walk to the bus.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use tf_sim::ids::{LotId, PedestrianId};
use tf_sim::traffic::vehicle::{CAR_LENGTH, CAR_WIDTH};

use super::lots::{LotViews, lot_layout, stall_count};
use super::{ViewMaterial, z};
use crate::camera::sim_to_bevy;
use crate::events::{DebugOverlayChanged, DebugOverlayToggleRequested};
use crate::runner::SimHandle;
use crate::snapshot::{Color as SimColor, ViewSnapshot, car_color, lerp_angle};

pub const LATENESS_OVERLAY: &str = "lateness";
/// `VehicleRenderer.ON_TIME_COLOR` / `LATE_COLOR` / `VERY_LATE_COLOR`.
pub const ON_TIME_COLOR: SimColor = [0.4, 0.733_333_3, 0.415_686_27, 1.0];
pub const LATE_COLOR: SimColor = [0.992_156_9, 0.847_058_8, 0.207_843_14, 1.0];
pub const VERY_LATE_COLOR: SimColor = [0.937_254_9, 0.325_490_2, 0.313_725_5, 1.0];
/// Radius of a pedestrian's dot (px).
pub const PEDESTRIAN_RADIUS: f32 = 2.4;
/// People's clothes, one picked per person (`PedestrianRenderer.CLOTHES`).
pub const CLOTHES: [SimColor; 10] = [
    [0.898, 0.224, 0.208, 1.0],
    [0.118, 0.533, 0.898, 1.0],
    [0.992, 0.847, 0.208, 1.0],
    [0.557, 0.141, 0.667, 1.0],
    [0.263, 0.627, 0.278, 1.0],
    [0.984, 0.549, 0.0, 1.0],
    [0.925, 0.937, 0.945, 1.0],
    [0.427, 0.298, 0.255, 1.0],
    [0.0, 0.675, 0.757, 1.0],
    [0.224, 0.286, 0.671, 1.0],
];
/// Riders waiting at a stop: where they stand (`TransitRenderer`'s constants).
pub const RIDERS_OFFSET: f32 = 17.0;
pub const RIDER_SPACING: f32 = 5.0;
pub const BAY_SIGN_SHIFT: f32 = 8.0;
pub const MAX_DRAWN_RIDERS: i32 = 12;

/// Whether F2's lateness colouring is on.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct ShowLateness(pub bool);

/// Colour of a vehicle `lateness` (0 on time, 1 impatient) late: green to yellow to red.
pub fn lateness_color(lateness: f32) -> SimColor {
    let mix = |a: SimColor, b: SimColor, t: f32| {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            1.0,
        ]
    };
    if lateness < 0.5 {
        mix(ON_TIME_COLOR, LATE_COLOR, lateness * 2.0)
    } else {
        mix(LATE_COLOR, VERY_LATE_COLOR, (lateness - 0.5) * 2.0)
    }
}

/// Vertex buffers for many shapes, written fresh each frame and swapped into a mesh.
#[derive(Default, Debug)]
pub struct Batch {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// sRGB `[f32; 4]` to the linear vertex colour `ColorMaterial` wants.
fn linear(c: SimColor) -> [f32; 4] {
    LinearRgba::from(Color::srgba(c[0], c[1], c[2], c[3])).to_f32_array()
}

impl Batch {
    pub fn clear(&mut self) {
        self.positions.clear();
        self.colors.clear();
        self.indices.clear();
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// A rectangle `size` (length along `angle`, width across) turned by `angle` radians
    /// (counter-clockwise, Bevy world): 4 vertices, 2 triangles.
    pub fn quad(&mut self, center: Vec2, size: Vec2, angle: f32, color: SimColor) {
        let (s, c) = angle.sin_cos();
        let (hx, hy) = (size.x * 0.5, size.y * 0.5);
        let base = self.positions.len() as u32;
        let rgba = linear(color);
        for (x, y) in [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)] {
            self.positions
                .push([center.x + c * x - s * y, center.y + s * x + c * y, 0.0]);
            self.colors.push(rgba);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// A hexagonal dot: 6 vertices, 4 triangles.
    pub fn dot(&mut self, center: Vec2, radius: f32, color: SimColor) {
        let base = self.positions.len() as u32;
        let rgba = linear(color);
        for k in 0..6 {
            let a = std::f32::consts::TAU * k as f32 / 6.0;
            self.positions.push([
                center.x + a.cos() * radius,
                center.y + a.sin() * radius,
                0.0,
            ]);
            self.colors.push(rgba);
        }
        for k in 1..5 {
            self.indices
                .extend_from_slice(&[base, base + k, base + k + 1]);
        }
    }

    /// A filled circle: a hub and `segments` rim vertices (at least 3), `segments`
    /// triangles.
    pub fn circle(&mut self, center: Vec2, radius: f32, segments: u32, color: SimColor) {
        let segments = segments.max(3);
        let rgba = linear(color);
        let hub = self.positions.len() as u32;
        self.positions.push([center.x, center.y, 0.0]);
        self.colors.push(rgba);
        let first = self.positions.len() as u32;
        for k in 0..segments {
            let a = std::f32::consts::TAU * k as f32 / segments as f32;
            self.positions.push([
                center.x + a.cos() * radius,
                center.y + a.sin() * radius,
                0.0,
            ]);
            self.colors.push(rgba);
        }
        for k in 0..segments {
            self.indices
                .extend_from_slice(&[hub, first + k, first + (k + 1) % segments]);
        }
    }

    /// Moves the buffers into `mesh` (and takes its old ones back, to be cleared and
    /// reused next frame), so nothing is allocated once the buffers are big enough.
    pub fn swap_into(&mut self, mesh: &mut Mesh) {
        if self.positions.is_empty() {
            // Bevy warns about (and skips) a mesh without vertices every frame: a triangle
            // without area draws nothing.
            self.positions.extend([[0.0; 3]; 3]);
            self.colors.extend([[0.0; 4]; 3]);
            self.indices.extend([0, 1, 2]);
        }
        if let Some(VertexAttributeValues::Float32x3(values)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        {
            std::mem::swap(values, &mut self.positions);
        }
        if let Some(VertexAttributeValues::Float32x4(values)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
        {
            std::mem::swap(values, &mut self.colors);
        }
        if let Some(Indices::U32(values)) = mesh.indices_mut() {
            std::mem::swap(values, &mut self.indices);
        }
    }
}

/// An empty mesh with the attributes a `Batch` fills in.
pub fn empty_batch_mesh() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3])
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3])
    .with_inserted_indices(Indices::U32(vec![0, 1, 2]))
}

/// The three batched entities and their buffers.
#[derive(Resource, Debug)]
pub struct MovingViews {
    pub vehicles: Handle<Mesh>,
    pub parked: Handle<Mesh>,
    pub pedestrians: Handle<Mesh>,
    vehicle_batch: Batch,
    parked_batch: Batch,
    pedestrian_batch: Batch,
    /// Interpolated poses of the vehicles by index in `view_to` (scratch, reused).
    poses: Vec<(Vec2, f32)>,
    /// What the parked cars were drawn from (they only change with a new snapshot or a lot
    /// edit): the snapshot's address and the lots' revision.
    parked_key: Option<(usize, u64, u64)>,
}

impl FromWorld for MovingViews {
    fn from_world(world: &mut World) -> Self {
        let material = world.resource::<ViewMaterial>().0.clone();
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let handles = [(); 3].map(|_| meshes.add(empty_batch_mesh()));
        let [vehicles, parked, pedestrians] = handles;
        for (handle, depth) in [
            (&vehicles, z::VEHICLES),
            (&parked, z::PARKED),
            (&pedestrians, z::PEDESTRIANS),
        ] {
            world.spawn((
                Mesh2d(handle.clone()),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(0.0, 0.0, depth),
                // The mesh changes every frame, and so would its bounds.
                NoFrustumCulling,
            ));
        }
        Self {
            vehicles,
            parked,
            pedestrians,
            vehicle_batch: Batch::default(),
            parked_batch: Batch::default(),
            pedestrian_batch: Batch::default(),
            poses: Vec::new(),
            parked_key: None,
        }
    }
}

/// Fills `batch` with the vehicles of `to`, `alpha` of the way from their place in `from`.
/// Vehicles new in `to` stand where `to` has them; those only in `from` are gone.
pub fn draw_vehicles(
    batch: &mut Batch,
    poses: &mut Vec<(Vec2, f32)>,
    from: &ViewSnapshot,
    to: &ViewSnapshot,
    alpha: f32,
    lateness: bool,
) {
    batch.clear();
    let n = to.positions.len();
    poses.clear();
    poses.resize(n, (Vec2::ZERO, 0.0));
    for (&id, &i) in &to.vehicle_index {
        poses[i] = match from.index_of(id) {
            Some(j) => (
                from.positions[j].lerp(to.positions[i], alpha),
                lerp_angle(from.rotations[j], to.rotations[i], alpha),
            ),
            None => (to.positions[i], to.rotations[i]),
        };
    }
    batch.positions.reserve(4 * n);
    batch.colors.reserve(4 * n);
    batch.indices.reserve(6 * n);
    for (i, &(position, rotation)) in poses.iter().enumerate() {
        let color = if lateness {
            lateness_color(to.lateness[i])
        } else {
            to.colors[i]
        };
        // The sim turns clockwise on screen (y down); the Bevy world is y up.
        batch.quad(sim_to_bevy(position), to.sizes[i], -rotation, color);
    }
}

/// Where rider `k` of `waiting` stands at stop `i` of a snapshot (sim coordinates):
/// `TransitRenderer.rider_spot`.
pub fn rider_spot(view: &ViewSnapshot, i: usize, k: i32, waiting: i32) -> Vec2 {
    let along = view.stop_directions[i];
    let out = Vec2::new(-along.y, along.x);
    let beyond = if view.stop_bays[i] {
        BAY_SIGN_SHIFT
    } else {
        0.0
    };
    let offset = (k as f32 - (waiting - 1) as f32 * 0.5) * RIDER_SPACING;
    view.stop_positions[i] + out * (RIDERS_OFFSET + beyond) + along * offset
}

/// Fills `batch` with the people walking (interpolated) and the riders waiting at stops.
pub fn draw_pedestrians(batch: &mut Batch, from: &ViewSnapshot, to: &ViewSnapshot, alpha: f32) {
    batch.clear();
    let clothes = |seed: i32| CLOTHES[(seed.unsigned_abs() as usize) % CLOTHES.len()];
    for (&PedestrianId(id), &i) in &to.pedestrian_index {
        let now = to.pedestrian_positions[i];
        let at = match from.pedestrian_index.get(&PedestrianId(id)) {
            Some(&j) => from.pedestrian_positions[j].lerp(now, alpha),
            None => now,
        };
        batch.dot(
            sim_to_bevy(at),
            PEDESTRIAN_RADIUS,
            clothes(id.wrapping_mul(31)),
        );
    }
    for i in 0..to.stop_positions.len() {
        let waiting = to.stop_waiting[i].min(MAX_DRAWN_RIDERS);
        for k in 0..waiting {
            let seed = to.stop_ids[i].0.wrapping_mul(97).wrapping_add(k);
            batch.dot(
                sim_to_bevy(rider_spot(to, i, k, waiting)),
                PEDESTRIAN_RADIUS,
                clothes(seed),
            );
        }
    }
}

/// Fills `batch` with the citizens' cars parked in the stalls of the lots on screen, nose
/// in (facing the building), as `LotRenderer._update_parked`.
pub fn draw_parked(batch: &mut Batch, lots: &LotViews, parked: &HashMap<LotId, i32>) {
    batch.clear();
    let size = Vec2::new(CAR_LENGTH as f32, CAR_WIDTH as f32);
    for (id, shape) in lots.shapes() {
        let (Some(door), Some(&count)) = (shape.door, parked.get(&id)) else {
            continue;
        };
        let layout = lot_layout(shape.plot, door, stall_count(shape.lot_type));
        // Facing the building, i.e. against the direction of the front (sim, y down).
        let heading = (-layout.front).to_angle();
        for (i, stall) in layout.stalls.iter().take(count.max(0) as usize).enumerate() {
            batch.quad(
                sim_to_bevy(stall.center()),
                size,
                -heading,
                car_color(id.0.wrapping_mul(7).wrapping_add(i as i32)),
            );
        }
    }
}

fn update_moving(
    handle: Res<SimHandle>,
    lots: Res<LotViews>,
    lateness: Res<ShowLateness>,
    mut views: ResMut<MovingViews>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let views = &mut *views;
    let (from, to, alpha) = (&handle.view_from, &handle.view_to, handle.view_alpha);
    draw_vehicles(
        &mut views.vehicle_batch,
        &mut views.poses,
        from,
        to,
        alpha,
        lateness.0,
    );
    if let Some(mut mesh) = meshes.get_mut(&views.vehicles) {
        views.vehicle_batch.swap_into(&mut mesh);
    }
    draw_pedestrians(&mut views.pedestrian_batch, from, to, alpha);
    if let Some(mut mesh) = meshes.get_mut(&views.pedestrians) {
        views.pedestrian_batch.swap_into(&mut mesh);
    }
    let key = (
        std::sync::Arc::as_ptr(to) as usize,
        to.time.to_bits(),
        lots.revision,
    );
    if views.parked_key != Some(key) {
        views.parked_key = Some(key);
        draw_parked(&mut views.parked_batch, &lots, &to.parked);
        if let Some(mut mesh) = meshes.get_mut(&views.parked) {
            views.parked_batch.swap_into(&mut mesh);
        }
    }
}

/// F2.
fn toggle_lateness(
    mut requests: MessageReader<DebugOverlayToggleRequested>,
    mut lateness: ResMut<ShowLateness>,
    mut changed: MessageWriter<DebugOverlayChanged>,
) {
    for request in requests.read() {
        if request.0 == LATENESS_OVERLAY {
            lateness.0 = !lateness.0;
            changed.write(DebugOverlayChanged {
                overlay_id: LATENESS_OVERLAY.into(),
                shown: lateness.0,
            });
        }
    }
}

pub struct MovingViewPlugin;

impl Plugin for MovingViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowLateness>()
            .init_resource::<MovingViews>()
            .add_systems(Update, (toggle_lateness, update_moving).chain());
    }
}
