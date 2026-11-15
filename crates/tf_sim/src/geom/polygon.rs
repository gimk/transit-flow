//! Simple polygons and circles for the street game (pivot plan §3.3): junction areas,
//! walkable plazas, obstacles, green areas and building blocks. Unit-free: the street
//! model uses metres. A polygon is a closed list of points (the last point joins the
//! first), either winding, not self-intersecting.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::rect::Rect2;
use super::segment::closest_point_to_segment;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Polygon {
    pub points: Vec<Vec2>,
}

impl Polygon {
    pub fn new(points: Vec<Vec2>) -> Self {
        Self { points }
    }

    /// The axis-aligned rectangle from `min` to `max`.
    pub fn rect(min: Vec2, max: Vec2) -> Self {
        Self::new(vec![
            min,
            Vec2::new(max.x, min.y),
            max,
            Vec2::new(min.x, max.y),
        ])
    }

    /// Each edge as (start, end), the closing edge last.
    pub fn edges(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        let n = self.points.len();
        (0..n).map(move |i| (self.points[i], self.points[(i + 1) % n]))
    }

    /// Positive when the points turn clockwise on screen (y down), negative otherwise.
    pub fn signed_area(&self) -> f32 {
        self.edges().map(|(a, b)| a.perp_dot(b)).sum::<f32>() * 0.5
    }

    pub fn area(&self) -> f32 {
        self.signed_area().abs()
    }

    /// Area-weighted centre; the first point for a degenerate polygon.
    pub fn centroid(&self) -> Vec2 {
        let area = self.signed_area();
        if area.abs() < f32::EPSILON {
            return self.points.first().copied().unwrap_or(Vec2::ZERO);
        }
        let sum: Vec2 = self.edges().map(|(a, b)| (a + b) * a.perp_dot(b)).sum();
        sum / (6.0 * area)
    }

    /// Even-odd test; points exactly on an edge may go either way.
    pub fn contains(&self, point: Vec2) -> bool {
        let mut inside = false;
        for (a, b) in self.edges() {
            if (a.y > point.y) != (b.y > point.y) {
                let x = a.x + (point.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if point.x < x {
                    inside = !inside;
                }
            }
        }
        inside
    }

    /// Distance from `point` to the outline (0 on an edge, also positive inside).
    pub fn distance_to_edge(&self, point: Vec2) -> f32 {
        self.edges()
            .map(|(a, b)| closest_point_to_segment(point, a, b).distance(point))
            .fold(f32::INFINITY, f32::min)
    }

    pub fn bounds(&self) -> Rect2 {
        let mut points = self.points.iter();
        let Some(&first) = points.next() else {
            return Rect2::new(Vec2::ZERO, Vec2::ZERO);
        };
        points.fold(Rect2::new(first, Vec2::ZERO), |rect, &p| rect.expand(p))
    }

    /// Distance along `polyline` at which it first crosses the outline, walking from its
    /// start; `None` if it never does. Used to trim street lanes at a junction's edge.
    pub fn first_exit_along(&self, polyline: &[Vec2]) -> Option<f32> {
        let mut walked = 0.0;
        for pair in polyline.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            let mut best: Option<f32> = None;
            for (a, b) in self.edges() {
                if let Some(t) = segment_intersection(p, q, a, b) {
                    best = Some(best.map_or(t, |best: f32| best.min(t)));
                }
            }
            if let Some(t) = best {
                return Some(walked + p.distance(q) * t);
            }
            walked += p.distance(q);
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Circle {
    pub center: Vec2,
    pub radius: f32,
}

impl Circle {
    pub fn contains(&self, point: Vec2) -> bool {
        self.center.distance_squared(point) <= self.radius * self.radius
    }
}

/// Where segment `p`-`q` crosses segment `a`-`b`, as a fraction of `p`-`q` (0..=1);
/// `None` when they don't cross or are parallel.
pub fn segment_intersection(p: Vec2, q: Vec2, a: Vec2, b: Vec2) -> Option<f32> {
    let r = q - p;
    let s = b - a;
    let denominator = r.perp_dot(s);
    if denominator.abs() < 1e-9 {
        return None;
    }
    let t = (a - p).perp_dot(s) / denominator;
    let u = (a - p).perp_dot(r) / denominator;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Polygon {
        Polygon::rect(Vec2::ZERO, Vec2::new(10.0, 10.0))
    }

    #[test]
    fn area_and_centroid_of_a_square() {
        let square = square();
        assert_eq!(square.area(), 100.0);
        assert_eq!(square.centroid(), Vec2::new(5.0, 5.0));
        let mut reversed = square.clone();
        reversed.points.reverse();
        assert_eq!(reversed.signed_area(), -square.signed_area());
    }

    #[test]
    fn contains_inside_not_outside() {
        let square = square();
        assert!(square.contains(Vec2::new(5.0, 5.0)));
        assert!(!square.contains(Vec2::new(11.0, 5.0)));
        assert!(!square.contains(Vec2::new(5.0, -0.5)));
        // Concave: an L shape leaves its notch out.
        let l = Polygon::new(vec![
            Vec2::ZERO,
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 4.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(4.0, 10.0),
            Vec2::new(0.0, 10.0),
        ]);
        assert!(l.contains(Vec2::new(2.0, 8.0)));
        assert!(!l.contains(Vec2::new(8.0, 8.0)));
    }

    #[test]
    fn distance_to_edge_and_bounds() {
        let square = square();
        assert_eq!(square.distance_to_edge(Vec2::new(5.0, 2.0)), 2.0);
        assert_eq!(square.distance_to_edge(Vec2::new(13.0, 5.0)), 3.0);
        let bounds = square.bounds();
        assert_eq!(
            (bounds.position, bounds.size),
            (Vec2::ZERO, Vec2::splat(10.0))
        );
    }

    #[test]
    fn first_exit_along_a_polyline() {
        let square = square();
        let line = [
            Vec2::new(5.0, 5.0),
            Vec2::new(5.0, 8.0),
            Vec2::new(20.0, 8.0),
        ];
        assert_eq!(square.first_exit_along(&line), Some(8.0));
        assert_eq!(
            square.first_exit_along(&[Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0)]),
            None
        );
    }

    #[test]
    fn circle_contains() {
        let circle = Circle {
            center: Vec2::new(1.0, 1.0),
            radius: 2.0,
        };
        assert!(circle.contains(Vec2::new(2.0, 2.0)));
        assert!(!circle.contains(Vec2::new(4.0, 1.0)));
    }
}
