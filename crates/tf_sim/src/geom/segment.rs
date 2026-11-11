//! Ports of the `Geometry2D` segment functions the sim uses, with Godot's edge cases
//! (degenerate segments, parallel lines, its epsilon). `godot_geometry2d.json` holds
//! results printed by Godot 4.7.2 for fixed and random segments; the tests compare.

use glam::Vec2;

/// `CMP_EPSILON` in Godot's math code.
const CMP_EPSILON: f32 = 0.00001;

/// Godot's `Math::is_equal_approx(a, b)` for floats.
fn is_equal_approx(a: f32, b: f32) -> bool {
    if a == b {
        return true;
    }
    let tolerance = (CMP_EPSILON * a.abs()).max(CMP_EPSILON);
    (a - b).abs() < tolerance
}

/// `Geometry2D.get_closest_point_to_segment(point, a, b)`.
pub fn closest_point_to_segment(point: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let p = point - a;
    let n = b - a;
    let l2 = n.length_squared();
    if l2 < 1e-20 {
        return a; // Both points are the same, just give any.
    }
    let d = n.dot(p) / l2;
    if d <= 0.0 {
        a
    } else if d >= 1.0 {
        b
    } else {
        a + n * d
    }
}

/// `Geometry2D.segment_intersects_segment(from_a, to_a, from_b, to_b)`: the crossing
/// point, or `None` (Godot's `null`). Parallel and collinear segments never cross.
pub fn segment_intersects_segment(
    from_a: Vec2,
    to_a: Vec2,
    from_b: Vec2,
    to_b: Vec2,
) -> Option<Vec2> {
    let b = to_a - from_a;
    let ab_len = b.dot(b);
    if ab_len <= 0.0 {
        return None;
    }
    // Rotate and scale into the frame where A-B runs from (0, 0) to (1, 0).
    let bn = b / ab_len;
    let to_frame = |p: Vec2| Vec2::new(p.x * bn.x + p.y * bn.y, p.y * bn.x - p.x * bn.y);
    let c = to_frame(from_b - from_a);
    let d = to_frame(to_b - from_a);
    // Fail if C x B and D x B have the same sign (segments don't intersect).
    if (c.y < -CMP_EPSILON && d.y < -CMP_EPSILON) || (c.y > CMP_EPSILON && d.y > CMP_EPSILON) {
        return None;
    }
    // Fail if segments are parallel or collinear.
    if is_equal_approx(c.y, d.y) {
        return None;
    }
    let ab_pos = d.x + (c.x - d.x) * d.y / (d.y - c.y);
    // Fail if segment C-D crosses line A-B outside segment A-B.
    if !(0.0..=1.0).contains(&ab_pos) {
        return None;
    }
    Some(from_a + b * ab_pos)
}

/// `Geometry2D.get_closest_points_between_segments(p1, q1, p2, q2)`: the closest point
/// on the first segment, then on the second.
pub fn closest_points_between_segments(p1: Vec2, q1: Vec2, p2: Vec2, q2: Vec2) -> (Vec2, Vec2) {
    let d1 = q1 - p1; // Direction vector of segment S1.
    let d2 = q2 - p2; // Direction vector of segment S2.
    let r = p1 - p2;
    let a = d1.dot(d1); // Squared length of segment S1, always nonnegative.
    let e = d2.dot(d2); // Squared length of segment S2, always nonnegative.
    let f = d2.dot(r);
    // Check if either or both segments degenerate into points.
    if a <= CMP_EPSILON && e <= CMP_EPSILON {
        return (p1, p2);
    }
    let (s, t);
    if a <= CMP_EPSILON {
        // First segment degenerates into a point.
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= CMP_EPSILON {
            // Second segment degenerates into a point.
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            // The general nondegenerate case.
            let b = d1.dot(d2);
            let denom = a * e - b * b; // Always nonnegative.
            // If the segments aren't parallel, take the point on L1 closest to L2,
            // clamped to S1; else pick s = 0.
            let mut s0 = if denom != 0.0 {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            // The point on L2 closest to S1(s); if outside S2, clamp it and redo s.
            let mut t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t0 = 0.0;
                s0 = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t0 = 1.0;
                s0 = ((b - c) / a).clamp(0.0, 1.0);
            }
            s = s0;
            t = t0;
        }
    }
    (p1 + d1 * s, p2 + d2 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct GodotCase {
        segments: [[f32; 2]; 4],
        intersection: Option<[f32; 2]>,
        closest_points: [[f32; 2]; 2],
        closest_to_segment: [f32; 2],
    }

    fn v(p: [f32; 2]) -> Vec2 {
        Vec2::from_array(p)
    }

    fn assert_near(actual: Vec2, expected: [f32; 2], what: &str, case: usize) {
        assert!(
            (actual - v(expected)).length() < 1e-3,
            "case {case}, {what}: got {actual}, Godot {expected:?}"
        );
    }

    #[test]
    fn matches_godot_geometry2d() {
        let cases: Vec<GodotCase> =
            serde_json::from_str(include_str!("godot_geometry2d.json")).unwrap();
        assert!(cases.len() > 40);
        for (i, case) in cases.iter().enumerate() {
            let [a, b, c, d] = case.segments.map(v);
            let hit = segment_intersects_segment(a, b, c, d);
            match (hit, case.intersection) {
                (None, None) => {}
                (Some(hit), Some(expected)) => assert_near(hit, expected, "intersection", i),
                _ => panic!(
                    "case {i}: intersection {hit:?}, Godot {:?}",
                    case.intersection
                ),
            }
            let (on_a, on_b) = closest_points_between_segments(a, b, c, d);
            assert_near(on_a, case.closest_points[0], "closest on first", i);
            assert_near(on_b, case.closest_points[1], "closest on second", i);
            assert_near(
                closest_point_to_segment(c, a, b),
                case.closest_to_segment,
                "closest to segment",
                i,
            );
        }
    }
}
