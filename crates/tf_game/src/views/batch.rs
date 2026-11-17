//! Batched flat shapes for the moving views: one mesh holds every car (or every walker),
//! its vertex buffers are rewritten each frame and swapped into the mesh, so no entity is
//! spawned or despawned per agent. Also `lerp_angle`, which the views use to turn an agent
//! between two snapshots along the shorter arc. Colours are sRGB `[f32; 4]` (`Rgba`).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

/// An sRGB colour as RGBA `f32`s.
pub type Rgba = [f32; 4];

/// Interpolates between two angles along the shorter arc.
pub fn lerp_angle(from: f32, to: f32, weight: f32) -> f32 {
    use std::f32::consts::TAU;
    let difference = (to - from) % TAU;
    let shortest = (2.0 * difference) % TAU - difference;
    from + shortest * weight
}

/// Vertex buffers for many shapes, written fresh each frame and swapped into a mesh.
#[derive(Default, Debug)]
pub struct Batch {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// sRGB `[f32; 4]` to the linear vertex colour `ColorMaterial` wants.
fn linear(c: Rgba) -> [f32; 4] {
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
    pub fn quad(&mut self, center: Vec2, size: Vec2, angle: f32, color: Rgba) {
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
    pub fn dot(&mut self, center: Vec2, radius: f32, color: Rgba) {
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
    pub fn circle(&mut self, center: Vec2, radius: f32, segments: u32, color: Rgba) {
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

    /// A ring (an annulus) of outer `radius` and `thickness` inward: `segments` quads, each
    /// two triangles. Used for marks that must not hide the body under them.
    pub fn ring(&mut self, center: Vec2, radius: f32, thickness: f32, segments: u32, color: Rgba) {
        let segments = segments.max(3);
        let inner = (radius - thickness).max(0.0);
        let rgba = linear(color);
        let first = self.positions.len() as u32;
        for k in 0..segments {
            let a = std::f32::consts::TAU * k as f32 / segments as f32;
            let (s, c) = a.sin_cos();
            self.positions
                .push([center.x + c * radius, center.y + s * radius, 0.0]);
            self.colors.push(rgba);
            self.positions
                .push([center.x + c * inner, center.y + s * inner, 0.0]);
            self.colors.push(rgba);
        }
        for k in 0..segments {
            let outer_index = first + 2 * k;
            let inner_index = outer_index + 1;
            let next_outer = first + 2 * ((k + 1) % segments);
            let next_inner = next_outer + 1;
            self.indices.extend_from_slice(&[
                outer_index,
                next_outer,
                next_inner,
                outer_index,
                next_inner,
                inner_index,
            ]);
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
