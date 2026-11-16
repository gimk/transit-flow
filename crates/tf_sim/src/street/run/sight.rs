//! Lines of sight (pivot plan §5.1, "a van hides a child"). A sight line is cut by an
//! obstacle that blocks sight (`ObstacleKind::blocks_sight`: trees, pillars, metro exits,
//! bus shelters, parked vans) or by a large vehicle: vans and buses hide what is behind
//! them, cars do not (a driver sees over and through a car, a child behind it is seen
//! enough to be braked for; start value). Sight between two vehicles is not tested
//! (Phase 2 limit): this is about seeing people.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::drive::Seen;
use crate::geom::Polygon;
use crate::geom::polygon::segment_intersection;
use crate::street::ids::AgentId;
use crate::street::level::Shape;
use crate::street::model::Street;

/// What cut a sight line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Hider {
    /// An obstacle of the street, by its id in `Street::obstacles`.
    Obstacle(i32),
    Vehicle(AgentId),
}

/// The obstacles that block sight, with their ids.
#[derive(Clone, Debug, Default)]
pub struct SightBlockers {
    circles: Vec<(i32, Vec2, f32)>,
    polygons: Vec<(i32, Polygon)>,
}

/// Does segment `p`-`q` pass through the disc?
fn cuts_circle(p: Vec2, q: Vec2, center: Vec2, radius: f32) -> bool {
    let closest = crate::geom::segment::closest_point_to_segment(center, p, q);
    closest.distance(center) < radius
}

fn cuts_polygon(p: Vec2, q: Vec2, polygon: &Polygon) -> bool {
    polygon.contains(p)
        || polygon.contains(q)
        || polygon
            .edges()
            .any(|(a, b)| segment_intersection(p, q, a, b).is_some())
}

/// The corners of a vehicle body.
fn corners(seen: &Seen) -> [Vec2; 4] {
    let along = seen.dir * (seen.length as f32 * 0.5);
    let side = seen.dir.perp() * (seen.width as f32 * 0.5);
    [
        seen.pos + along + side,
        seen.pos + along - side,
        seen.pos - along - side,
        seen.pos - along + side,
    ]
}

impl SightBlockers {
    pub fn new(street: &Street) -> SightBlockers {
        let mut blockers = SightBlockers::default();
        for obstacle in street.obstacles.values() {
            if !obstacle.kind.blocks_sight() {
                continue;
            }
            match &obstacle.shape {
                Shape::Circle(c) => blockers.circles.push((obstacle.id, c.center, c.radius)),
                Shape::Polygon(p) => blockers.polygons.push((obstacle.id, p.clone())),
            }
        }
        blockers
    }

    /// What cuts the sight line from `eye` to `target`, if anything: an obstacle, or a
    /// large vehicle of `vehicles` other than `skip` (the one looking).
    pub fn hider(
        &self,
        eye: Vec2,
        target: Vec2,
        vehicles: &[Seen],
        skip: Option<AgentId>,
    ) -> Option<Hider> {
        for (id, center, radius) in &self.circles {
            if cuts_circle(eye, target, *center, *radius) {
                return Some(Hider::Obstacle(*id));
            }
        }
        for (id, polygon) in &self.polygons {
            if cuts_polygon(eye, target, polygon) {
                return Some(Hider::Obstacle(*id));
            }
        }
        for seen in vehicles {
            if !seen.hides || Some(seen.id) == skip {
                continue;
            }
            // A quick reject: the body is further from the line than its half diagonal.
            let reach = (seen.length.hypot(seen.width) * 0.5) as f32;
            if !cuts_circle(eye, target, seen.pos, reach) {
                continue;
            }
            let body = Polygon::new(corners(seen).to_vec());
            if cuts_polygon(eye, target, &body) {
                return Some(Hider::Vehicle(seen.id));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wall_between_cuts_the_line_and_one_beside_it_does_not() {
        let mut blockers = SightBlockers::default();
        blockers
            .polygons
            .push((7, Polygon::rect(Vec2::new(4.0, -1.0), Vec2::new(5.0, 1.0))));
        blockers.circles.push((8, Vec2::new(5.0, 10.0), 0.5));
        let eye = Vec2::ZERO;
        assert_eq!(
            blockers.hider(eye, Vec2::new(10.0, 0.0), &[], None),
            Some(Hider::Obstacle(7))
        );
        assert_eq!(blockers.hider(eye, Vec2::new(10.0, 5.0), &[], None), None);
        assert_eq!(
            blockers.hider(eye, Vec2::new(10.0, 20.0), &[], None),
            Some(Hider::Obstacle(8))
        );
    }
}
