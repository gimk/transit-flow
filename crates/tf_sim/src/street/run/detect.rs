//! Do two bodies touch? (pivot plan §5.5.) Bodies are oriented rectangles; the test is
//! made at several moments of the tick (a car moves 0.7 m per tick at 50 km/h), which
//! sweeps them well enough for bodies 1.8 m wide.

use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub center: Vec2,
    /// Unit heading.
    pub dir: Vec2,
    pub half_length: f32,
    pub half_width: f32,
}

impl Body {
    /// The body part of the way from pose `from` to pose `to` (same size).
    pub fn between(from: &Body, to: &Body, t: f32) -> Body {
        let dir = from.dir.lerp(to.dir, t).normalize_or_zero();
        Body {
            center: from.center.lerp(to.center, t),
            dir: if dir == Vec2::ZERO { to.dir } else { dir },
            ..*to
        }
    }

    /// How far the body reaches from its centre along `axis` (a unit vector).
    fn reach(&self, axis: Vec2) -> f32 {
        self.half_length * self.dir.dot(axis).abs()
            + self.half_width * self.dir.perp().dot(axis).abs()
    }
}

/// Separating axis test: two rectangles are apart if, along one of their four side
/// directions, their shadows do not meet.
pub fn overlap(a: &Body, b: &Body) -> bool {
    let between = b.center - a.center;
    [a.dir, a.dir.perp(), b.dir, b.dir.perp()]
        .into_iter()
        .all(|axis| between.dot(axis).abs() < a.reach(axis) + b.reach(axis))
}

/// The moments of a tick at which bodies are tested, as fractions of the step.
pub const SWEEP: [f32; 3] = [1.0 / 3.0, 2.0 / 3.0, 1.0];

/// Do the bodies touch at some moment of the tick? `*_from` is the pose before the
/// step, `*_to` after it.
pub fn swept_overlap(a_from: &Body, a_to: &Body, b_from: &Body, b_to: &Body) -> bool {
    SWEEP.into_iter().any(|t| {
        overlap(
            &Body::between(a_from, a_to, t),
            &Body::between(b_from, b_to, t),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(x: f32, y: f32, dir: Vec2) -> Body {
        Body {
            center: Vec2::new(x, y),
            dir,
            half_length: 2.15,
            half_width: 0.9,
        }
    }

    #[test]
    fn cars_side_by_side_in_two_lanes_do_not_touch() {
        assert!(!overlap(&car(0.0, 0.0, Vec2::X), &car(0.0, 3.0, -Vec2::X)));
        assert!(overlap(&car(0.0, 0.0, Vec2::X), &car(0.0, 1.7, -Vec2::X)));
    }

    #[test]
    fn a_crossing_car_touches_only_when_it_reaches_the_side() {
        let across = |y: f32| car(0.0, y, Vec2::Y);
        assert!(!overlap(&car(0.0, 0.0, Vec2::X), &across(3.2)));
        assert!(overlap(&car(0.0, 0.0, Vec2::X), &across(2.9)));
        // Diagonal: the corners are what counts, not the circles around the bodies.
        let diagonal = car(3.7, 2.6, Vec2::new(1.0, 1.0).normalize());
        assert!(!overlap(&car(0.0, 0.0, Vec2::X), &diagonal));
    }

    #[test]
    fn a_body_that_arrives_during_the_tick_touches() {
        // 30 m/s: 1.5 m per tick, across a parked car's nose.
        let parked = car(0.0, 0.0, Vec2::X);
        let from = car(2.6, -3.2, Vec2::Y);
        let to = car(2.6, -1.7, Vec2::Y);
        assert!(!overlap(&parked, &from));
        assert!(swept_overlap(&parked, &parked, &from, &to));
    }
}
