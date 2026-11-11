//! `Curve2D`: a port of the parts of Godot 4.7's `Curve2D` the sim uses, baking included,
//! so lanes have exactly Godot's baked points, lengths and sampled poses. Godot bakes a
//! curve into its control points plus midpoints from recursive Bézier subdivision
//! (`_bake_segment2d_even_length`): a piece is halved while its chord is longer than
//! `bake_interval`, at most 10 times. Even a straight polyline gets midpoints, which only
//! matters for sampling headings. Checked against Godot's own output in
//! `tests/curve_baking.rs` (the golden files' lanes).
//!
//! Unlike Godot's, a Rust `Curve2D` is built once (`new` / `from_points`) and baked right
//! away. The sim never edits a curve in place; it replaces it.

use glam::Vec2;

use super::gd::{GdVec2, is_zero_approx};

/// Godot's default `Curve2D.bake_interval`.
pub const DEFAULT_BAKE_INTERVAL: f32 = 5.0;
/// `_tessellate_even_length(10, bake_interval)`.
const MAX_BAKE_STAGES: u32 = 10;

/// One control point: its position and its in / out handles (relative to it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurvePoint {
    pub position: Vec2,
    pub point_in: Vec2,
    pub point_out: Vec2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Curve2D {
    points: Vec<CurvePoint>,
    bake_interval: f32,
    baked_points: Vec<Vec2>,
    baked_distances: Vec<f32>,
    baked_forward: Vec<Vec2>,
}

impl Curve2D {
    /// A curve through `points`, baked with the default interval.
    pub fn new(points: Vec<CurvePoint>) -> Self {
        Self::with_bake_interval(points, DEFAULT_BAKE_INTERVAL)
    }

    /// A polyline: `add_point(p)` for each point, no handles.
    pub fn from_points(points: &[Vec2]) -> Self {
        Self::new(
            points
                .iter()
                .map(|&position| CurvePoint {
                    position,
                    ..CurvePoint::default()
                })
                .collect(),
        )
    }

    pub fn with_bake_interval(points: Vec<CurvePoint>, bake_interval: f32) -> Self {
        let mut curve = Self {
            points,
            bake_interval,
            baked_points: Vec::new(),
            baked_distances: Vec::new(),
            baked_forward: Vec::new(),
        };
        curve.bake();
        curve
    }

    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    pub fn point_position(&self, i: usize) -> Vec2 {
        self.points[i].position
    }

    pub fn point_in(&self, i: usize) -> Vec2 {
        self.points[i].point_in
    }

    pub fn point_out(&self, i: usize) -> Vec2 {
        self.points[i].point_out
    }

    pub fn points(&self) -> &[CurvePoint] {
        &self.points
    }

    pub fn bake_interval(&self) -> f32 {
        self.bake_interval
    }

    /// `get_baked_points()`.
    pub fn baked_points(&self) -> &[Vec2] {
        &self.baked_points
    }

    /// `get_baked_length()`.
    pub fn baked_length(&self) -> f32 {
        self.baked_distances.last().copied().unwrap_or(0.0)
    }

    /// `Curve2D::_bake`.
    fn bake(&mut self) {
        let points = &self.points;
        match points.len() {
            0 => return,
            1 => {
                self.baked_points = vec![points[0].position];
                self.baked_distances = vec![0.0];
                self.baked_forward = vec![Vec2::new(0.0, 0.1)];
                return;
            }
            _ => {}
        }
        // Tessellate the curve into (almost) even length pieces.
        let mut baked = vec![points[0].position];
        let mut forward = vec![tangent(&points[0], &points[1], 0.0)];
        for pair in points.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let mut midpoints: Vec<(f32, Vec2)> = Vec::new();
            subdivide(
                &mut midpoints,
                0.0,
                1.0,
                a,
                b,
                0,
                MAX_BAKE_STAGES,
                self.bake_interval,
            );
            // Godot keeps them in an ordered map keyed by t.
            midpoints.sort_by(|x, y| x.0.total_cmp(&y.0));
            for (t, point) in midpoints {
                baked.push(point);
                forward.push(tangent(a, b, t));
            }
            baked.push(b.position);
            forward.push(tangent(a, b, 1.0));
        }
        let mut distances = Vec::with_capacity(baked.len());
        distances.push(0.0f32);
        for i in 0..baked.len() - 1 {
            let next = distances[i] + baked[i].gd_distance_to(baked[i + 1]);
            distances.push(next);
        }
        self.baked_points = baked;
        self.baked_distances = distances;
        self.baked_forward = forward;
    }

    /// `Curve2D::_find_interval`: the baked piece holding `offset` and how far along it.
    fn find_interval(&self, offset: f32) -> (usize, f32) {
        let distances = &self.baked_distances;
        let mut start = 0;
        let mut end = distances.len();
        let mut idx = (end + start) / 2;
        // Binary search to find baked points.
        while start < idx {
            if offset <= distances[idx] {
                end = idx;
            } else {
                start = idx;
            }
            idx = (end + start) / 2;
        }
        let offset_begin = distances[idx];
        let offset_end = distances[idx + 1];
        let idx_interval = offset_end - offset_begin;
        if idx_interval < f32::EPSILON {
            // For a very short interval, 0.5 is a reasonable choice (Godot also logs an
            // error here).
            return (idx, 0.5);
        }
        (idx, (offset - offset_begin) / idx_interval)
    }

    /// `sample_baked(offset)`: the point `offset` along, clamped to the curve.
    pub fn sample_baked(&self, offset: f32) -> Vec2 {
        match self.baked_points.len() {
            0 => Vec2::ZERO,
            1 => self.baked_points[0],
            _ => {
                let (idx, frac) = self.find_interval(offset.clamp(0.0, self.baked_length()));
                self.baked_points[idx].gd_lerp(self.baked_points[idx + 1], frac)
            }
        }
    }

    /// `sample_baked_with_rotation(offset)`: the point and its heading in radians
    /// (`Transform2D.get_rotation()`), headings interpolated between baked points.
    pub fn sample_baked_with_rotation(&self, offset: f32) -> (Vec2, f32) {
        match self.baked_points.len() {
            0 => (Vec2::ZERO, 0.0),
            1 => (self.baked_points[0], 0.0),
            _ => {
                let (idx, frac) = self.find_interval(offset.clamp(0.0, self.baked_length()));
                let position = self.baked_points[idx].gd_lerp(self.baked_points[idx + 1], frac);
                let forward = self.baked_forward[idx]
                    .gd_slerp(self.baked_forward[idx + 1], frac)
                    .gd_normalized();
                (position, forward.y.atan2(forward.x))
            }
        }
    }

    /// `get_closest_offset(point)`: the offset of the point on the curve closest to
    /// `point` (the first one, if several are as close).
    pub fn closest_offset(&self, to_point: Vec2) -> f32 {
        let points = &self.baked_points;
        if points.len() < 2 {
            return 0.0;
        }
        let mut nearest = 0.0;
        let mut nearest_distance = -1.0;
        for i in 0..points.len() - 1 {
            let interval = self.baked_distances[i + 1] - self.baked_distances[i];
            let origin = points[i];
            let direction = (points[i + 1] - origin) / interval;
            let d = (to_point - origin).dot(direction).clamp(0.0, interval);
            let projection = origin + direction * d;
            let distance = projection.distance_squared(to_point);
            if nearest_distance < 0.0 || distance < nearest_distance {
                nearest = self.baked_distances[i] + d;
                nearest_distance = distance;
            }
        }
        nearest
    }
}

/// `Curve2D::_bake_segment2d_even_length` for the piece from `a` to `b`, between curve
/// parameters `begin` and `end`.
#[allow(clippy::too_many_arguments)]
fn subdivide(
    bake: &mut Vec<(f32, Vec2)>,
    begin: f32,
    end: f32,
    a: &CurvePoint,
    b: &CurvePoint,
    depth: u32,
    max_depth: u32,
    length: f32,
) {
    let (control_1, control_2) = (a.position + a.point_out, b.position + b.point_in);
    let at = |t: f32| {
        a.position
            .gd_bezier_interpolate(control_1, control_2, b.position, t)
    };
    let chord = at(begin).gd_distance_to(at(end));
    if chord > length && depth < max_depth {
        let middle = (begin + end) * 0.5;
        bake.push((middle, at(middle)));
        subdivide(bake, begin, middle, a, b, depth + 1, max_depth, length);
        subdivide(bake, middle, end, a, b, depth + 1, max_depth, length);
    }
}

/// `_calculate_tangent` for the piece from `a` to `b` at parameter `t`.
fn tangent(a: &CurvePoint, b: &CurvePoint, t: f32) -> Vec2 {
    let (begin, end) = (a.position, b.position);
    let (control_1, control_2) = (begin + a.point_out, end + b.point_in);
    // Handle corner cases.
    if is_zero_approx(t - 0.0) && control_1.gd_is_equal_approx(begin) {
        return (end - begin).gd_normalized();
    }
    if is_zero_approx(t - 1.0) && control_2.gd_is_equal_approx(end) {
        return (end - begin).gd_normalized();
    }
    begin
        .gd_bezier_derivative(control_1, control_2, end, t)
        .gd_normalized()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_straight_polyline_bakes_halving_until_pieces_fit_the_interval() {
        // Without handles the Bézier still eases in and out (x = 20 (3t² - 2t³)), so
        // halving t gives pieces of 3.1, 6.9, 6.9, 3.1 px; the middle ones are halved
        // again: 7 points, as Godot bakes it.
        let curve = Curve2D::from_points(&[Vec2::ZERO, Vec2::new(20.0, 0.0)]);
        assert_eq!(curve.baked_points().len(), 7);
        assert_eq!(curve.baked_length(), 20.0);
        assert_eq!(curve.sample_baked(7.5), Vec2::new(7.5, 0.0));
        assert_eq!(curve.sample_baked(99.0), Vec2::new(20.0, 0.0));
        assert_eq!(curve.closest_offset(Vec2::new(12.0, 4.0)), 12.0);
    }

    #[test]
    fn single_and_empty_curves() {
        assert_eq!(Curve2D::from_points(&[]).baked_length(), 0.0);
        let dot = Curve2D::from_points(&[Vec2::ONE]);
        assert_eq!(dot.sample_baked(3.0), Vec2::ONE);
        assert_eq!(dot.closest_offset(Vec2::ZERO), 0.0);
    }
}
