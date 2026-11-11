//! Godot's `Vector2` methods, written the way Godot computes them (`f32`, divisions
//! where Godot divides), where glam's equivalents differ in edge cases or rounding:
//! `normalized` of a zero vector stays zero, `lerp` is `a + (b - a) * t`, and so on.
//! Use these in ported code instead of glam's methods of the same meaning.

use glam::Vec2;

/// Godot's `CMP_EPSILON`.
pub const CMP_EPSILON: f32 = 0.00001;

/// Godot's `Math::is_zero_approx` for floats.
pub fn is_zero_approx(value: f32) -> bool {
    value.abs() < CMP_EPSILON
}

/// Godot's `Math::is_equal_approx` for floats.
pub fn is_equal_approx(a: f32, b: f32) -> bool {
    if a == b {
        return true;
    }
    let tolerance = (CMP_EPSILON * a.abs()).max(CMP_EPSILON);
    (a - b).abs() < tolerance
}

/// Godot's `Math::lerp` for floats.
pub fn lerpf(from: f32, to: f32, weight: f32) -> f32 {
    from + (to - from) * weight
}

/// `Vector2` methods as Godot computes them.
pub trait GdVec2 {
    fn gd_normalized(self) -> Vec2;
    fn gd_lerp(self, to: Vec2, weight: f32) -> Vec2;
    fn gd_angle(self) -> f32;
    fn gd_angle_to(self, to: Vec2) -> f32;
    fn gd_cross(self, with: Vec2) -> f32;
    fn gd_rotated(self, angle: f32) -> Vec2;
    fn gd_slerp(self, to: Vec2, weight: f32) -> Vec2;
    fn gd_distance_to(self, to: Vec2) -> f32;
    fn gd_is_equal_approx(self, other: Vec2) -> bool;
    fn gd_bezier_interpolate(self, control_1: Vec2, control_2: Vec2, end: Vec2, t: f32) -> Vec2;
    fn gd_bezier_derivative(self, control_1: Vec2, control_2: Vec2, end: Vec2, t: f32) -> Vec2;
}

impl GdVec2 for Vec2 {
    fn gd_normalized(self) -> Vec2 {
        let l = self.x * self.x + self.y * self.y;
        if l != 0.0 {
            let l = l.sqrt();
            Vec2::new(self.x / l, self.y / l)
        } else {
            self
        }
    }

    fn gd_lerp(self, to: Vec2, weight: f32) -> Vec2 {
        Vec2::new(lerpf(self.x, to.x, weight), lerpf(self.y, to.y, weight))
    }

    fn gd_angle(self) -> f32 {
        self.y.atan2(self.x)
    }

    fn gd_angle_to(self, to: Vec2) -> f32 {
        self.gd_cross(to).atan2(self.dot(to))
    }

    fn gd_cross(self, with: Vec2) -> f32 {
        self.x * with.y - self.y * with.x
    }

    fn gd_rotated(self, angle: f32) -> Vec2 {
        let (sine, cosine) = angle.sin_cos();
        Vec2::new(
            self.x * cosine - self.y * sine,
            self.x * sine + self.y * cosine,
        )
    }

    fn gd_slerp(self, to: Vec2, weight: f32) -> Vec2 {
        let start_length_sq = self.length_squared();
        let end_length_sq = to.length_squared();
        if start_length_sq == 0.0 || end_length_sq == 0.0 {
            // Zero length vectors have no angle, so the best we can do is lerp.
            return self.gd_lerp(to, weight);
        }
        let start_length = start_length_sq.sqrt();
        let result_length = lerpf(start_length, end_length_sq.sqrt(), weight);
        let angle = self.gd_angle_to(to);
        self.gd_rotated(angle * weight) * (result_length / start_length)
    }

    fn gd_distance_to(self, to: Vec2) -> f32 {
        ((self.x - to.x) * (self.x - to.x) + (self.y - to.y) * (self.y - to.y)).sqrt()
    }

    fn gd_is_equal_approx(self, other: Vec2) -> bool {
        is_equal_approx(self.x, other.x) && is_equal_approx(self.y, other.y)
    }

    fn gd_bezier_interpolate(self, control_1: Vec2, control_2: Vec2, end: Vec2, t: f32) -> Vec2 {
        Vec2::new(
            bezier_interpolate(self.x, control_1.x, control_2.x, end.x, t),
            bezier_interpolate(self.y, control_1.y, control_2.y, end.y, t),
        )
    }

    fn gd_bezier_derivative(self, control_1: Vec2, control_2: Vec2, end: Vec2, t: f32) -> Vec2 {
        Vec2::new(
            bezier_derivative(self.x, control_1.x, control_2.x, end.x, t),
            bezier_derivative(self.y, control_1.y, control_2.y, end.y, t),
        )
    }
}

/// Godot's `Math::bezier_interpolate` (float version).
fn bezier_interpolate(start: f32, control_1: f32, control_2: f32, end: f32, t: f32) -> f32 {
    // Formula from Wikipedia article on Bezier curves.
    let omt = 1.0 - t;
    let omt2 = omt * omt;
    let omt3 = omt2 * omt;
    let t2 = t * t;
    let t3 = t2 * t;
    start * omt3 + control_1 * omt2 * t * 3.0 + control_2 * omt * t2 * 3.0 + end * t3
}

/// Godot's `Math::bezier_derivative` (float version).
fn bezier_derivative(start: f32, control_1: f32, control_2: f32, end: f32, t: f32) -> f32 {
    let omt = 1.0 - t;
    let omt2 = omt * omt;
    let t2 = t * t;
    (control_1 - start) * 3.0 * omt2
        + (control_2 - control_1) * 6.0 * omt * t
        + (end - control_2) * 3.0 * t2
}
