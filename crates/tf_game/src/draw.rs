//! Flat shapes as meshes (plan §2.3: Bevy's built-in 2D meshes and `ColorMaterial`, no
//! custom shaders or textures). A `ShapeBuilder` collects polylines with a width, filled
//! polygons, rectangles and circles, each with its own colour (written as vertex colours,
//! which `ColorMaterial` multiplies by its own white), and turns them into one `Mesh`. So a
//! whole road, lot or marker is one entity, and views rebuild only the ones a change touches.
//!
//! Points are in the **Bevy world** (y up): callers convert sim positions with
//! `camera::sim_to_bevy` first. Shapes are flat at z = 0; the entity's transform sets the
//! draw order (`views::z`).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// A mitre longer than this many half widths is cut back, so sharp bends don't spike.
pub const MITRE_LIMIT: f32 = 2.0;
/// Consecutive points closer than this are one point.
const MIN_SEGMENT: f32 = 1e-4;

#[derive(Clone, Debug, Default)]
pub struct ShapeBuilder {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// A colour from sRGB components, as a `Color` (for the `[f32; 4]` colours of the sim's
/// data files).
pub fn srgba(c: [f32; 4]) -> Color {
    Color::srgba(c[0], c[1], c[2], c[3])
}

impl ShapeBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    fn vertex(&mut self, p: Vec2, color: [f32; 4]) -> u32 {
        self.positions.push([p.x, p.y, 0.0]);
        self.colors.push(color);
        (self.positions.len() - 1) as u32
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend_from_slice(&[a, b, c]);
    }

    /// An open band of `width` along the points (a mitre at each bend, cut back at sharp
    /// ones; flat ends). Fewer than two distinct points draw nothing.
    pub fn polyline(&mut self, points: &[Vec2], width: f32, color: Color) -> &mut Self {
        self.band(points, width, color, false)
    }

    /// Like `polyline`, joined from the last point back to the first.
    pub fn closed_polyline(&mut self, points: &[Vec2], width: f32, color: Color) -> &mut Self {
        self.band(points, width, color, true)
    }

    #[allow(clippy::needless_range_loop)]
    fn band(&mut self, points: &[Vec2], width: f32, color: Color, closed: bool) -> &mut Self {
        let mut pts: Vec<Vec2> = Vec::with_capacity(points.len());
        for &p in points {
            if pts.last().is_none_or(|l| l.distance(p) > MIN_SEGMENT) {
                pts.push(p);
            }
        }
        if closed && pts.len() > 1 && pts[0].distance(pts[pts.len() - 1]) <= MIN_SEGMENT {
            pts.pop();
        }
        let n = pts.len();
        if n < 2 || (closed && n < 3) {
            return self;
        }
        let rgba = LinearRgba::from(color).to_f32_array();
        let half = width * 0.5;
        // Left normal of the segment starting at i.
        let segments = if closed { n } else { n - 1 };
        let normal = |i: usize| {
            let d = (pts[(i + 1) % n] - pts[i]).normalize();
            Vec2::new(-d.y, d.x)
        };
        let first = self.positions.len() as u32;
        for i in 0..n {
            let offset = if !closed && i == 0 {
                normal(0) * half
            } else if !closed && i == n - 1 {
                normal(n - 2) * half
            } else {
                let before = normal((i + n - 1) % n);
                let after = normal(i);
                let sum = before + after;
                if sum.length_squared() < 1e-6 {
                    // A full U-turn: no mitre exists.
                    after * half
                } else {
                    let mitre = sum.normalize();
                    // Length 1 / cos(half the turn), capped.
                    mitre * (half / mitre.dot(after).max(1.0 / MITRE_LIMIT))
                }
            };
            self.vertex(pts[i] + offset, rgba);
            self.vertex(pts[i] - offset, rgba);
        }
        for i in 0..segments {
            let a = first + 2 * i as u32;
            let b = first + 2 * ((i + 1) % n) as u32;
            // a: left, a + 1: right; the strip runs counter-clockwise.
            self.triangle(a + 1, b + 1, b);
            self.triangle(a + 1, b, a);
        }
        self
    }

    /// A straight band from `a` to `b`.
    pub fn line(&mut self, a: Vec2, b: Vec2, width: f32, color: Color) -> &mut Self {
        self.polyline(&[a, b], width, color)
    }

    /// A filled simple polygon (convex or concave, either winding), by ear clipping.
    /// Self-intersecting or degenerate outlines give a best effort.
    pub fn polygon(&mut self, points: &[Vec2], color: Color) -> &mut Self {
        let mut pts: Vec<Vec2> = Vec::with_capacity(points.len());
        for &p in points {
            if pts.last().is_none_or(|l| l.distance(p) > MIN_SEGMENT) {
                pts.push(p);
            }
        }
        if pts.len() > 1 && pts[0].distance(pts[pts.len() - 1]) <= MIN_SEGMENT {
            pts.pop();
        }
        if pts.len() < 3 {
            return self;
        }
        let rgba = LinearRgba::from(color).to_f32_array();
        let first = self.positions.len() as u32;
        for &p in &pts {
            self.vertex(p, rgba);
        }
        let mut ring: Vec<usize> = (0..pts.len()).collect();
        if signed_area(&pts) < 0.0 {
            ring.reverse();
        }
        while ring.len() > 3 {
            let m = ring.len();
            let ear = (0..m).find(|&i| {
                let (a, b, c) = (ring[(i + m - 1) % m], ring[i], ring[(i + 1) % m]);
                cross(pts[a], pts[b], pts[c]) > 1e-9
                    && ring
                        .iter()
                        .filter(|&&k| k != a && k != b && k != c)
                        .all(|&k| !in_triangle(pts[k], pts[a], pts[b], pts[c]))
            });
            // No ear in a degenerate outline: clip the first corner anyway.
            let i = ear.unwrap_or(0);
            let (a, b, c) = (ring[(i + m - 1) % m], ring[i], ring[(i + 1) % m]);
            self.triangle(first + a as u32, first + b as u32, first + c as u32);
            ring.remove(i);
        }
        self.triangle(
            first + ring[0] as u32,
            first + ring[1] as u32,
            first + ring[2] as u32,
        );
        self
    }

    /// An axis-aligned rectangle between two corners.
    pub fn rect(&mut self, min: Vec2, max: Vec2, color: Color) -> &mut Self {
        self.polygon(
            &[min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)],
            color,
        )
    }

    /// A rectangle `size` wide and tall, centred on `center` and turned by `angle` radians
    /// (counter-clockwise in the Bevy world): vehicles, signs, bars.
    pub fn quad(&mut self, center: Vec2, size: Vec2, angle: f32, color: Color) -> &mut Self {
        let (s, c) = angle.sin_cos();
        let (hx, hy) = (size.x * 0.5, size.y * 0.5);
        let corner = |x: f32, y: f32| center + Vec2::new(c * x - s * y, s * x + c * y);
        self.polygon(
            &[
                corner(-hx, -hy),
                corner(hx, -hy),
                corner(hx, hy),
                corner(-hx, hy),
            ],
            color,
        )
    }

    /// The border of an axis-aligned rectangle, `thickness` wide and inside it.
    pub fn rect_outline(
        &mut self,
        min: Vec2,
        max: Vec2,
        thickness: f32,
        color: Color,
    ) -> &mut Self {
        let t = thickness.min((max - min).min_element() * 0.5);
        self.rect(min, Vec2::new(max.x, min.y + t), color)
            .rect(Vec2::new(min.x, max.y - t), max, color)
            .rect(
                Vec2::new(min.x, min.y + t),
                Vec2::new(min.x + t, max.y - t),
                color,
            )
            .rect(
                Vec2::new(max.x - t, min.y + t),
                Vec2::new(max.x, max.y - t),
                color,
            )
    }

    /// A filled circle: a fan of `segments` triangles (at least 3).
    pub fn circle(&mut self, center: Vec2, radius: f32, segments: u32, color: Color) -> &mut Self {
        let segments = segments.max(3);
        let rgba = LinearRgba::from(color).to_f32_array();
        let hub = self.vertex(center, rgba);
        let first = self.positions.len() as u32;
        for k in 0..segments {
            let a = std::f32::consts::TAU * k as f32 / segments as f32;
            self.vertex(center + Vec2::from_angle(a) * radius, rgba);
        }
        for k in 0..segments {
            self.triangle(hub, first + k, first + (k + 1) % segments);
        }
        self
    }

    /// Adds another builder's shapes.
    pub fn append(&mut self, other: &ShapeBuilder) -> &mut Self {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.colors.extend_from_slice(&other.colors);
        self.indices.extend(other.indices.iter().map(|i| i + base));
        self
    }

    pub fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

/// Twice the signed area of the corner a-b-c (positive: counter-clockwise).
fn cross(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    (b - a).perp_dot(c - b)
}

fn signed_area(pts: &[Vec2]) -> f32 {
    let n = pts.len();
    (0..n)
        .map(|i| pts[i].perp_dot(pts[(i + 1) % n]))
        .sum::<f32>()
        * 0.5
}

/// `p` inside (or on the edge of) the counter-clockwise triangle.
fn in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    (b - a).perp_dot(p - a) >= -1e-9
        && (c - b).perp_dot(p - b) >= -1e-9
        && (a - c).perp_dot(p - c) >= -1e-9
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri_area_sum(b: &ShapeBuilder) -> f32 {
        b.indices()
            .chunks(3)
            .map(|t| {
                let p = |i: u32| {
                    let v = b.positions()[i as usize];
                    Vec2::new(v[0], v[1])
                };
                (p(t[1]) - p(t[0])).perp_dot(p(t[2]) - p(t[0])) * 0.5
            })
            .sum()
    }

    fn xy(b: &ShapeBuilder) -> Vec<Vec2> {
        b.positions()
            .iter()
            .map(|v| Vec2::new(v[0], v[1]))
            .collect()
    }

    #[test]
    fn a_straight_polyline_is_a_quad_with_the_right_corners() {
        let mut b = ShapeBuilder::new();
        b.polyline(
            &[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)],
            4.0,
            Color::WHITE,
        );
        assert_eq!((b.vertex_count(), b.triangle_count()), (4, 2));
        let mut corners = xy(&b);
        corners.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        assert_eq!(
            corners,
            vec![
                Vec2::new(0.0, -2.0),
                Vec2::new(0.0, 2.0),
                Vec2::new(10.0, -2.0),
                Vec2::new(10.0, 2.0)
            ]
        );
        // Counter-clockwise: the triangles' areas add up to the quad's.
        assert!((tri_area_sum(&b) - 40.0).abs() < 1e-4);
    }

    #[test]
    fn a_bend_adds_a_vertex_pair_and_a_mitre() {
        let mut b = ShapeBuilder::new();
        b.polyline(
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(10.0, 10.0),
            ],
            2.0,
            Color::WHITE,
        );
        assert_eq!((b.vertex_count(), b.triangle_count()), (6, 4));
        // The outer corner of a right-angle bend sits at (11, -1) and the inner at (9, 1).
        let pts = xy(&b);
        assert!(pts.iter().any(|p| p.distance(Vec2::new(11.0, -1.0)) < 1e-4));
        assert!(pts.iter().any(|p| p.distance(Vec2::new(9.0, 1.0)) < 1e-4));
        assert!((tri_area_sum(&b) - 40.0).abs() < 1e-3);
    }

    #[test]
    fn a_hairpin_does_not_spike() {
        let mut b = ShapeBuilder::new();
        b.polyline(
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(0.0, 0.1),
            ],
            2.0,
            Color::WHITE,
        );
        let longest = xy(&b)[2..4]
            .iter()
            .map(|p| p.distance(Vec2::new(10.0, 0.0)))
            .fold(0.0, f32::max);
        assert!(longest <= 1.0 * MITRE_LIMIT + 1e-3);
    }

    #[test]
    fn degenerate_polylines_draw_nothing() {
        let mut b = ShapeBuilder::new();
        b.polyline(&[Vec2::ONE], 2.0, Color::WHITE).polyline(
            &[Vec2::ONE, Vec2::ONE],
            2.0,
            Color::WHITE,
        );
        assert!(b.is_empty());
    }

    #[test]
    fn a_closed_polyline_has_no_ends() {
        let mut b = ShapeBuilder::new();
        let square = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ];
        b.closed_polyline(&square, 2.0, Color::WHITE);
        assert_eq!((b.vertex_count(), b.triangle_count()), (8, 8));
        // The ring between the 12x12 outer and 8x8 inner squares.
        assert!((tri_area_sum(&b) - (144.0 - 64.0)).abs() < 1e-3);
    }

    #[test]
    fn a_convex_polygon_is_a_fan() {
        let mut b = ShapeBuilder::new();
        // Clockwise on purpose.
        b.polygon(
            &[
                Vec2::new(0.0, 0.0),
                Vec2::new(0.0, 4.0),
                Vec2::new(4.0, 4.0),
                Vec2::new(4.0, 0.0),
            ],
            Color::WHITE,
        );
        assert_eq!((b.vertex_count(), b.triangle_count()), (4, 2));
        assert!((tri_area_sum(&b) - 16.0).abs() < 1e-4);
    }

    #[test]
    fn a_concave_polygon_covers_exactly_its_area() {
        let mut b = ShapeBuilder::new();
        // An L: 3 unit squares.
        let l = [
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(2.0, 1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(1.0, 2.0),
            Vec2::new(0.0, 2.0),
        ];
        b.polygon(&l, Color::WHITE);
        assert_eq!(b.triangle_count(), 4);
        assert!((tri_area_sum(&b) - 3.0).abs() < 1e-5);
        // Every triangle is counter-clockwise and none covers the notch at (1.5, 1.5).
        for t in b.indices().chunks(3) {
            let p = |i: u32| xy(&b)[i as usize];
            let (a, bb, c) = (p(t[0]), p(t[1]), p(t[2]));
            assert!((bb - a).perp_dot(c - a) > 0.0);
            assert!(!in_triangle(Vec2::new(1.5, 1.5), a, bb, c));
        }
    }

    #[test]
    fn rects_quads_and_circles() {
        let mut b = ShapeBuilder::new();
        b.rect(Vec2::new(1.0, 1.0), Vec2::new(3.0, 4.0), Color::WHITE);
        assert_eq!((b.vertex_count(), b.triangle_count()), (4, 2));
        assert!((tri_area_sum(&b) - 6.0).abs() < 1e-5);

        let mut q = ShapeBuilder::new();
        q.quad(
            Vec2::new(5.0, 5.0),
            Vec2::new(4.0, 2.0),
            std::f32::consts::FRAC_PI_2,
            Color::WHITE,
        );
        // Turned a quarter: 2 wide, 4 tall.
        let pts = xy(&q);
        let (min, max) = pts.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| {
            (lo.min(*p), hi.max(*p))
        });
        assert!((max - min - Vec2::new(2.0, 4.0)).length() < 1e-4);
        assert!(((min + max) / 2.0 - Vec2::new(5.0, 5.0)).length() < 1e-4);

        let mut c = ShapeBuilder::new();
        c.circle(Vec2::ZERO, 2.0, 16, Color::WHITE);
        assert_eq!((c.vertex_count(), c.triangle_count()), (17, 16));
        let area = tri_area_sum(&c);
        assert!(area < std::f32::consts::PI * 4.0 && area > 12.0);
    }

    #[test]
    fn an_outline_is_a_ring_and_builders_append() {
        let mut o = ShapeBuilder::new();
        o.rect_outline(Vec2::ZERO, Vec2::new(10.0, 10.0), 1.0, Color::WHITE);
        assert!((tri_area_sum(&o) - (100.0 - 64.0)).abs() < 1e-4);
        let mut b = ShapeBuilder::new();
        b.rect(Vec2::ZERO, Vec2::ONE, Color::WHITE);
        b.append(&o);
        assert_eq!(b.triangle_count(), 2 + o.triangle_count());
        assert!(b.indices().iter().all(|&i| (i as usize) < b.vertex_count()));
        let mesh = b.build();
        assert_eq!(mesh.count_vertices(), 4 + o.vertex_count());
    }
}
