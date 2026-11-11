//! `Polyline`: the Rust stand-in for a `Curve2D` once Godot has baked it. Every lane,
//! sidewalk and road centreline in the sim is a `Curve2D` that is only ever read through
//! its baked points (`sample_baked`, `sample_baked_with_rotation`, `get_closest_offset`,
//! `get_baked_length`, `get_baked_points`), so a polyline with cumulative lengths covers
//! all of it.
//!
//! What this type does **not** do yet is reproduce Godot's baking: Godot resamples a
//! curve into points `bake_interval` (5 px) apart, and connectors are cubic Béziers
//! (`LaneBuilder.build_connector_curve`). Turning control points into the same baked
//! points as Godot is work package M1.2 (`geom::bake`, checked against the golden files);
//! until then `Polyline::new` takes points that are already baked.

use glam::Vec2;

/// A polyline with its cumulative lengths, sampled by distance along it.
#[derive(Clone, Debug, PartialEq)]
pub struct Polyline {
    points: Vec<Vec2>,
    /// `distances[i]`: length along the polyline from its start to `points[i]`.
    distances: Vec<f32>,
}

impl Polyline {
    /// A polyline through `points`, in order. Needs at least one point.
    pub fn new(points: Vec<Vec2>) -> Self {
        assert!(!points.is_empty(), "a polyline needs at least one point");
        let mut distances = Vec::with_capacity(points.len());
        let mut total = 0.0;
        distances.push(0.0);
        for pair in points.windows(2) {
            total += pair[0].distance(pair[1]);
            distances.push(total);
        }
        Self { points, distances }
    }

    /// `get_baked_points()`.
    pub fn points(&self) -> &[Vec2] {
        &self.points
    }

    /// `get_baked_length()`.
    pub fn length(&self) -> f32 {
        *self.distances.last().expect("never empty")
    }

    /// The segment index `i` (from `points[i]` to `points[i + 1]`) holding `distance`,
    /// and how far along it (0 to 1). `distance` is clamped to the polyline.
    fn locate(&self, distance: f32) -> (usize, f32) {
        if self.points.len() == 1 {
            return (0, 0.0);
        }
        let distance = distance.clamp(0.0, self.length());
        // Last segment whose start is at or before `distance`.
        let i = self
            .distances
            .partition_point(|&d| d <= distance)
            .saturating_sub(1)
            .min(self.points.len() - 2);
        let span = self.distances[i + 1] - self.distances[i];
        let t = if span > 0.0 {
            (distance - self.distances[i]) / span
        } else {
            0.0
        };
        (i, t)
    }

    /// `sample_baked(distance)`: the point `distance` along, clamped to the ends.
    pub fn sample(&self, distance: f32) -> Vec2 {
        let (i, t) = self.locate(distance);
        match self.points.get(i + 1) {
            Some(&next) => self.points[i].lerp(next, t),
            None => self.points[i],
        }
    }

    /// `sample_baked_with_rotation(distance)`: the point and the heading (radians, as
    /// `Vector2.angle()`) of the segment it lies on. Godot interpolates headings between
    /// baked points; whether that matters here is checked against the golden data in
    /// M1.2.
    pub fn sample_with_rotation(&self, distance: f32) -> (Vec2, f32) {
        let (i, t) = self.locate(distance);
        match self.points.get(i + 1) {
            Some(&next) => {
                let from = self.points[i];
                (from.lerp(next, t), (next - from).to_angle())
            }
            None => (self.points[i], 0.0),
        }
    }

    /// `get_closest_offset(point)`: the distance along the polyline of the point on it
    /// closest to `point` (the first one, if several are as close).
    pub fn closest_offset(&self, point: Vec2) -> f32 {
        if self.points.len() == 1 {
            return 0.0;
        }
        let mut best_distance_sq = f32::INFINITY;
        let mut best_offset = 0.0;
        for (i, pair) in self.points.windows(2).enumerate() {
            let closest = super::segment::closest_point_to_segment(point, pair[0], pair[1]);
            let distance_sq = closest.distance_squared(point);
            if distance_sq < best_distance_sq {
                best_distance_sq = distance_sq;
                best_offset = self.distances[i] + pair[0].distance(closest);
            }
        }
        best_offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn l_shape() -> Polyline {
        Polyline::new(vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 20.0),
        ])
    }

    #[test]
    fn length_adds_up_the_segments() {
        assert_relative_eq!(l_shape().length(), 30.0);
        assert_relative_eq!(Polyline::new(vec![Vec2::ONE]).length(), 0.0);
    }

    #[test]
    fn sample_walks_along_and_clamps_at_the_ends() {
        let line = l_shape();
        assert_eq!(line.sample(5.0), Vec2::new(5.0, 0.0));
        assert_eq!(line.sample(10.0), Vec2::new(10.0, 0.0));
        assert_eq!(line.sample(15.0), Vec2::new(10.0, 5.0));
        assert_eq!(line.sample(-3.0), Vec2::ZERO);
        assert_eq!(line.sample(99.0), Vec2::new(10.0, 20.0));
    }

    #[test]
    fn rotation_follows_the_segment() {
        let line = l_shape();
        assert_relative_eq!(line.sample_with_rotation(5.0).1, 0.0);
        assert_relative_eq!(
            line.sample_with_rotation(15.0).1,
            std::f32::consts::FRAC_PI_2
        );
    }

    #[test]
    fn closest_offset_projects_onto_the_nearest_segment() {
        let line = l_shape();
        assert_relative_eq!(line.closest_offset(Vec2::new(4.0, -3.0)), 4.0);
        assert_relative_eq!(line.closest_offset(Vec2::new(14.0, 12.0)), 22.0);
        assert_relative_eq!(line.closest_offset(Vec2::new(-5.0, -5.0)), 0.0);
    }
}
