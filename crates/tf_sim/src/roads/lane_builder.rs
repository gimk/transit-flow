//! `LaneBuilder` (`sim/roads/lane_builder.gd`): lane geometry: road lanes from a
//! segment's centerline, and connector curves through junctions. Road lanes are offset
//! *polylines*, not offset Bézier control points: an offset Bézier is not a Bézier,
//! while sampling keeps straight and curved roads on one code path. Traffic drives on
//! the right: lanes sit to the right of their direction of travel.
//!
//! Number types follow the GDScript: its `float`s are `f64`, `Vector2` maths is `f32`.

use glam::Vec2;

use super::road_type::RoadType;
use crate::geom::segment::{closest_point_to_segment, closest_points_between_segments};
use crate::geom::{Curve2D, CurvePoint, GdVec2};

pub const DUPLICATE_POINT_EPSILON: f64 = 0.01;
/// Caps how far a corner point may be pushed out, so sharp bends don't produce spikes.
pub const MIN_MITRE_COSINE: f64 = 0.25;
/// Trimming never shortens a lane below this, even on very short segments.
pub const MIN_TRIMMED_LENGTH: f64 = 4.0;

/// One lane of a road: "forward" lanes run from the centerline's first point to its
/// last.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneSpec {
    pub forward: bool,
    pub index: i32,
    pub curve: Curve2D,
}

/// One entry per lane. `start_trim` / `end_trim` cut lanes back from the segment's start
/// / end node, leaving room for the junction's connector lanes.
pub fn build_lane_specs(
    centerline: &Curve2D,
    road_type: &RoadType,
    start_trim: f64,
    end_trim: f64,
) -> Vec<LaneSpec> {
    let forward_points = get_polyline(centerline);
    let mut backward_points = forward_points.clone();
    backward_points.reverse();
    let mut specs = Vec::new();
    if road_type.one_way {
        // All lanes forward, centered on the centerline; index 0 is the leftmost lane
        // (where left turns start, like the center lane of a two-way road).
        let half_width = road_type.total_width() * 0.5;
        for index in 0..road_type.lanes_per_direction {
            let offset = (f64::from(index) + 0.5) * road_type.lane_width - half_width;
            let forward = trim_polyline(
                &offset_polyline(&forward_points, offset),
                start_trim,
                end_trim,
            );
            specs.push(LaneSpec {
                forward: true,
                index,
                curve: polyline_to_curve(&forward),
            });
        }
        return specs;
    }
    for index in 0..road_type.lanes_per_direction {
        let offset = (f64::from(index) + 0.5) * road_type.lane_width;
        let forward = trim_polyline(
            &offset_polyline(&forward_points, offset),
            start_trim,
            end_trim,
        );
        let backward = trim_polyline(
            &offset_polyline(&backward_points, offset),
            end_trim,
            start_trim,
        );
        specs.push(LaneSpec {
            forward: true,
            index,
            curve: polyline_to_curve(&forward),
        });
        specs.push(LaneSpec {
            forward: false,
            index,
            curve: polyline_to_curve(&backward),
        });
    }
    specs
}

/// Smooth curve from the end of `from_curve` to the start of `to_curve`, leaving and
/// arriving tangent to both. Turns aim both handles at the point where the two lanes'
/// lines meet (a quadratic curve expressed as a cubic); straight continuations stay
/// straight; U-turns become a near-semicircle.
pub fn build_connector_curve(from_curve: &Curve2D, to_curve: &Curve2D) -> Curve2D {
    let from_points = get_polyline(from_curve);
    let to_points = get_polyline(to_curve);
    let p0 = from_points[from_points.len() - 1];
    let p1 = to_points[0];
    let d0 =
        (from_points[from_points.len() - 1] - from_points[from_points.len() - 2]).gd_normalized();
    let d1 = (to_points[1] - to_points[0]).gd_normalized();
    let chord = f64::from(p0.gd_distance_to(p1));

    let mut h0 = chord * 0.5;
    let mut h1 = chord * 0.5;
    let denominator = f64::from(d0.gd_cross(d1));
    if denominator.abs() > 0.0001 {
        // Solve p0 + t·d0 = p1 - s·d1 for the corner point of the turn.
        let w = p1 - p0;
        let t = f64::from(w.gd_cross(d1)) / denominator;
        let s = -f64::from(w.gd_cross(d0)) / denominator;
        if t > 0.0 && s > 0.0 {
            h0 = t * 2.0 / 3.0;
            h1 = s * 2.0 / 3.0;
        }
    } else if d0.dot(d1) > 0.0 {
        h0 = 0.0;
        h1 = 0.0;
    } else {
        // Cubic approximation of a semicircle of radius chord / 2.
        h0 = chord * 2.0 / 3.0;
        h1 = h0;
    }

    Curve2D::new(vec![
        CurvePoint {
            position: p0,
            point_in: Vec2::ZERO,
            point_out: d0 * h0 as f32,
        },
        CurvePoint {
            position: p1,
            point_in: -d1 * h1 as f32,
            point_out: Vec2::ZERO,
        },
    ])
}

/// The curve's shape as a polyline: its control points when it has no handles (exact
/// for grid roads), its baked points otherwise.
pub fn get_polyline(curve: &Curve2D) -> Vec<Vec2> {
    let source: Vec<Vec2> = if has_handles(curve) {
        curve.baked_points().to_vec()
    } else {
        curve.points().iter().map(|point| point.position).collect()
    };
    let mut points: Vec<Vec2> = Vec::with_capacity(source.len());
    for point in source {
        match points.last() {
            Some(&last) if f64::from(last.gd_distance_to(point)) <= DUPLICATE_POINT_EPSILON => {}
            _ => points.push(point),
        }
    }
    points
}

/// Shifts every point sideways by `offset`; positive = right of the direction of travel.
pub fn offset_polyline(points: &[Vec2], offset: f64) -> Vec<Vec2> {
    let count = points.len();
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let normal = if i == 0 {
            right_normal(points[0], points[1])
        } else if i == count - 1 {
            right_normal(points[i - 1], points[i])
        } else {
            let normal_in = right_normal(points[i - 1], points[i]);
            let normal_out = right_normal(points[i], points[i + 1]);
            let mitre = (normal_in + normal_out).gd_normalized();
            mitre / f64::from(mitre.dot(normal_in)).max(MIN_MITRE_COSINE) as f32
        };
        result.push(points[i] + normal * offset as f32);
    }
    result
}

/// Removes `start_trim` px from the start and `end_trim` px from the end. If the
/// polyline is too short for both, the trims shrink proportionally.
pub fn trim_polyline(points: &[Vec2], mut start_trim: f64, mut end_trim: f64) -> Vec<Vec2> {
    let length = polyline_length(points);
    let available = (length - MIN_TRIMMED_LENGTH).max(0.0);
    if start_trim + end_trim > available {
        let scale = available / (start_trim + end_trim);
        start_trim *= scale;
        end_trim *= scale;
    }
    sub_polyline(points, start_trim, length - end_trim)
}

/// The part of the polyline between two distances measured along it.
pub fn sub_polyline(points: &[Vec2], from_distance: f64, to_distance: f64) -> Vec<Vec2> {
    let mut result = Vec::new();
    let mut walked = 0.0;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let piece = f64::from(a.gd_distance_to(b));
        let next_walked = walked + piece;
        if next_walked >= from_distance && walked <= to_distance && piece > 0.0 {
            if result.is_empty() {
                result
                    .push(a.gd_lerp(b, ((from_distance - walked) / piece).clamp(0.0, 1.0) as f32));
            }
            if next_walked >= to_distance {
                result.push(a.gd_lerp(b, ((to_distance - walked) / piece).clamp(0.0, 1.0) as f32));
                break;
            }
            result.push(b);
        }
        walked = next_walked;
    }
    result
}

/// Distance along the polyline to the point on it closest to `point`.
pub fn distance_along_polyline(points: &[Vec2], point: Vec2) -> f64 {
    let mut best_gap = f64::INFINITY;
    let mut best_distance = 0.0;
    let mut walked = 0.0;
    for pair in points.windows(2) {
        let closest = closest_point_to_segment(point, pair[0], pair[1]);
        let gap = f64::from(closest.gd_distance_to(point));
        if gap < best_gap {
            best_gap = gap;
            best_distance = walked + f64::from(pair[0].gd_distance_to(closest));
        }
        walked += f64::from(pair[0].gd_distance_to(pair[1]));
    }
    best_distance
}

/// The stretch of polyline `a` (x = from, y = to, as distances along it) that comes
/// within `threshold` of polyline `b`, sampled every `step` px; `None` if none (Godot
/// returns `Vector2(-1, -1)`).
pub fn conflict_zone(a: &[Vec2], b: &[Vec2], threshold: f64, step: f64) -> Option<Vec2> {
    let mut zone: Option<Vec2> = None;
    let mut walked = 0.0;
    for pair in a.windows(2) {
        let piece = f64::from(pair[0].gd_distance_to(pair[1]));
        let samples = ((piece / step).ceil() as i64).max(1);
        for s in 0..=samples {
            let t = s as f64 / samples as f64;
            if distance_to_polyline(pair[0].gd_lerp(pair[1], t as f32), b) < threshold {
                let at = (walked + piece * t) as f32;
                zone = Some(match zone {
                    None => Vec2::new(at, at),
                    Some(zone) => Vec2::new(zone.x, at),
                });
            }
        }
        walked += piece;
    }
    // Sampling may miss up to one step at each edge: widen to stay on the safe side.
    zone.map(|zone| {
        Vec2::new(
            (f64::from(zone.x) - step).max(0.0) as f32,
            (f64::from(zone.y) + step).min(walked) as f32,
        )
    })
}

/// Smallest distance between any two points of two polylines (0 if they cross).
pub fn polyline_distance(a: &[Vec2], b: &[Vec2]) -> f64 {
    let mut best = f64::INFINITY;
    for pa in a.windows(2) {
        for pb in b.windows(2) {
            let (on_a, on_b) = closest_points_between_segments(pa[0], pa[1], pb[0], pb[1]);
            best = best.min(f64::from(on_a.gd_distance_to(on_b)));
        }
    }
    best
}

pub fn polyline_length(points: &[Vec2]) -> f64 {
    points
        .windows(2)
        .map(|pair| f64::from(pair[0].gd_distance_to(pair[1])))
        .sum()
}

pub fn polyline_to_curve(points: &[Vec2]) -> Curve2D {
    Curve2D::from_points(points)
}

pub fn distance_to_polyline(point: Vec2, polyline: &[Vec2]) -> f64 {
    let mut best = f64::INFINITY;
    for pair in polyline.windows(2) {
        let closest = closest_point_to_segment(point, pair[0], pair[1]);
        best = best.min(f64::from(closest.gd_distance_to(point)));
    }
    best
}

/// Unit direction of travel at the start of a lane curve.
pub fn start_direction(curve: &Curve2D) -> Vec2 {
    let points = get_polyline(curve);
    (points[1] - points[0]).gd_normalized()
}

/// Unit direction of travel at the end of a lane curve.
pub fn end_direction(curve: &Curve2D) -> Vec2 {
    let points = get_polyline(curve);
    (points[points.len() - 1] - points[points.len() - 2]).gd_normalized()
}

pub fn has_handles(curve: &Curve2D) -> bool {
    curve
        .points()
        .iter()
        .any(|point| point.point_in != Vec2::ZERO || point.point_out != Vec2::ZERO)
}

/// In y-down screen space, the right-hand side of travel direction d is (-d.y, d.x).
fn right_normal(from: Vec2, to: Vec2) -> Vec2 {
    let direction = (to - from).gd_normalized();
    Vec2::new(-direction.y, direction.x)
}
