//! The door of a parked car (Phase 6 W4b, pivot plan §8 "classic conflicts"). A parked car
//! in a parking strip opens its door into the strip beside it: the bicycle lane, or the
//! edge of the car lane. A rider in the door zone must brake or leave the lane; a buffer
//! strip between the parking and the lane (planting, a median, a sidewalk) puts the door
//! out of reach, and so does a bicycle track that is not beside the parking.
//!
//! Parked cars are not bodies in the run: the level has parking strips, not parked cars.
//! A door is therefore a hazard of the strip, opened by a stream of its own (one per
//! parking strip, from the run seed), so that it opens at the same place and the same tick
//! whatever the traffic does. The chance is per parked car and per second, with the number
//! of cars a strip holds taken from its length.

use glam::Vec2;
use indexmap::IndexMap;

use super::agent::{Agent, chance};
use super::drive::{DT, SAMPLE_STEP, SIDE_MARGIN, World};
use super::mix;
use crate::geom::Curve2D;
use crate::geom::lanes as polylines;
use crate::rng::SimRng;
use crate::street::ids::AgentId;
use crate::street::model::Street;
use crate::street::space::StripUse;

/// A parked car takes this much of a parking strip, metres (a car and the room to get out).
pub const PARKED_CAR_LENGTH_M: f64 = 5.5;
/// Chance per second and per parked car that a door opens (start value: a car parks or
/// leaves every few minutes on a busy street, and a door is opened once per stop).
pub const DOOR_CHANCE_PER_CAR_S: f64 = 1.0 / 900.0;
/// A door stays open this long, seconds (start value: getting in or out with bags).
pub const DOOR_OPEN_S: f64 = 4.0;
/// How far a door reaches into the strip beside the parking, metres (a car door is about
/// 0.9-1.1 m wide).
pub const DOOR_OPEN_M: f64 = 1.0;
/// A person in a parked car looks before opening the door: no door opens with a body this
/// near the place, metres (start value: the mirror check, and no driver has to brake for a
/// door that opens on top of it).
pub const DOOR_LOOK_M: f64 = 12.0;

/// One open door.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Door {
    pub segment: i32,
    /// The parking strip it opens from.
    pub strip: usize,
    /// The middle of the open door, in the strip beside the parking.
    pub pos: Vec2,
    /// The tick it is closed again.
    pub until: u64,
}

/// The doors of a run.
pub struct Doors {
    /// (segment, parking strip) -> the stream that opens its doors.
    streams: IndexMap<(i32, usize), SimRng>,
    open: Vec<Door>,
}

impl Doors {
    /// One stream per parking strip that has a travel strip beside it.
    pub fn new(street: &Street, seed: u64) -> Self {
        let mut streams = IndexMap::new();
        for (id, segment) in &street.segments {
            for (index, strip) in segment.profile.strips.iter().enumerate() {
                if !matches!(strip.use_, StripUse::Parking | StripUse::DeliveryBay) {
                    continue;
                }
                if !beside_travel(&segment.profile, index) {
                    continue;
                }
                let key = mix(*id as u64, index as u64 + 1);
                streams.insert((*id, index), SimRng::new(mix(seed, key)));
            }
        }
        Self {
            streams,
            open: Vec::new(),
        }
    }

    pub fn open(&self) -> &[Door] {
        &self.open
    }

    /// Closes the doors whose time is up, then lets each strip draw whether a door opens.
    /// One draw per strip every tick, and the place only when one opens, so the stream of
    /// a strip does not depend on the traffic.
    pub fn step(&mut self, street: &Street, tick: u64, agents: &IndexMap<AgentId, Agent>) {
        self.open.retain(|door| door.until > tick);
        let mut opened = Vec::new();
        for (&(segment, strip), rng) in self.streams.iter_mut() {
            let Some(found) = street.segments.get(&segment) else {
                continue;
            };
            let length = polylines::polyline_length(&found.centerline)
                - f64::from(found.start_trim)
                - f64::from(found.end_trim);
            if length <= 0.0 {
                continue;
            }
            let cars = (length / PARKED_CAR_LENGTH_M).max(1.0);
            if !chance(rng, DOOR_CHANCE_PER_CAR_S * cars * DT) {
                continue;
            }
            if self
                .open
                .iter()
                .chain(opened.iter())
                .any(|door| door.segment == segment && door.strip == strip)
            {
                continue;
            }
            let place = f64::from(rng.randf_range(0.0, 1.0));
            if let Some(door) = open_door(found, strip, place, tick)
                && !agents
                    .values()
                    .any(|a| a.pos.distance(door.pos) < DOOR_LOOK_M as f32)
            {
                opened.push(door);
            }
        }
        self.open.extend(opened);
    }

    /// The nearest open door in the path of `agent` within `lookahead` metres: (gap from
    /// its front, the door's place). The body is taken at its side position.
    pub fn ahead(&self, agent: &Agent, world: &World, lookahead: f64) -> Option<(f64, Vec2)> {
        if self.open.is_empty() {
            return None;
        }
        let route = &world.routes[agent.route];
        let reach = (agent.width * 0.5 + DOOR_OPEN_M * 0.5 + SIDE_MARGIN) as f32;
        let count = (lookahead / SAMPLE_STEP).ceil() as usize + 1;
        let mut best: Option<(f64, Vec2)> = None;
        for door in &self.open {
            if door.pos.distance(agent.pos) > (lookahead + 10.0) as f32 {
                continue;
            }
            for k in 0..=count {
                let d = agent.d + k as f64 * SAMPLE_STEP;
                let mut point = route.point(world.street, d);
                if agent.lateral != 0.0 {
                    let tangent = (route.point(world.street, d + 0.5)
                        - route.point(world.street, d - 0.5))
                    .normalize_or_zero();
                    point += tangent.perp() * agent.lateral as f32;
                }
                if point.distance_squared(door.pos) < reach * reach {
                    let gap = k as f64 * SAMPLE_STEP;
                    if best.is_none_or(|(g, _)| gap < g) {
                        best = Some((gap, door.pos));
                    }
                    break;
                }
            }
        }
        best
    }
}

/// Is a travel strip beside strip `index` of this profile?
fn beside_travel(profile: &crate::street::space::Profile, index: usize) -> bool {
    [index.checked_sub(1), Some(index + 1)]
        .into_iter()
        .flatten()
        .any(|i| profile.strips.get(i).is_some_and(|s| s.use_.is_travel()))
}

/// The door of the parked car at `place` (0 to 1 along the strip), opening into the travel
/// strip beside the parking.
fn open_door(
    segment: &crate::street::model::StreetSegment,
    strip: usize,
    place: f64,
    tick: u64,
) -> Option<Door> {
    let profile = &segment.profile;
    let side = [strip.checked_sub(1), Some(strip + 1)]
        .into_iter()
        .flatten()
        .find(|i| profile.strips.get(*i).is_some_and(|s| s.use_.is_travel()))?;
    let length = polylines::polyline_length(&segment.centerline)
        - f64::from(segment.start_trim)
        - f64::from(segment.end_trim);
    let at = f64::from(segment.start_trim) + place * length;
    let curve = Curve2D::from_points(&segment.centerline);
    let point = curve.sample_baked(at as f32);
    let tangent = (curve.sample_baked((at + 0.5) as f32)
        - curve.sample_baked((at - 0.5).max(0.0) as f32))
    .normalize_or_zero();
    // The edge of the parking strip on the side of the travel strip, then half a door into
    // that strip.
    let (left, right) = profile.strip_span(strip);
    let edge = if side < strip { left } else { right };
    let into = if side < strip {
        -DOOR_OPEN_M * 0.5
    } else {
        DOOR_OPEN_M * 0.5
    };
    Some(Door {
        segment: segment.id,
        strip,
        pos: point + tangent.perp() * (f64::from(edge) + into) as f32,
        until: tick + (DOOR_OPEN_S / DT).round() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::street::design::Design;
    use crate::street::level::LevelFile;

    fn street() -> Street {
        let level = LevelFile::load("carrefour-ecole").expect("the level loads");
        Street::from_level_and_design(&level, &Design::new(&level.id)).expect("builds")
    }

    /// Level 1 has a parking strip on each arm of the school street, beside the car lane.
    #[test]
    fn a_parking_strip_beside_a_car_lane_gets_a_stream() {
        let street = street();
        let doors = Doors::new(&street, 1);
        assert!(doors.streams.contains_key(&(1, 3)), "segment 1, strip 3");
        assert!(doors.streams.contains_key(&(3, 3)), "segment 3, strip 3");
    }

    /// A door opens in the strip beside the parking, and closes again.
    #[test]
    fn a_door_opens_in_the_strip_beside_the_parking_and_closes() {
        let street = street();
        let mut doors = Doors::new(&street, 1);
        let mut opened = None;
        let nobody = IndexMap::new();
        for tick in 0..20_000 {
            doors.step(&street, tick, &nobody);
            if let Some(door) = doors.open().first() {
                opened = Some((tick, *door));
                break;
            }
        }
        let (tick, door) = opened.expect("a door opens within 1000 s");
        assert_eq!(door.strip, 3, "the parking strip of the school street");
        // It stands in the car lane beside the parking (strip 2), half a door from its edge.
        let segment = &street.segments[&door.segment];
        let lateral = polylines::distance_to_polyline(door.pos, &segment.centerline);
        assert!(
            lateral > 0.5 && lateral < 3.5,
            "{lateral} m from the middle"
        );
        // It closes after `DOOR_OPEN_S`.
        let closed = tick + (DOOR_OPEN_S / DT).round() as u64;
        doors.step(&street, closed, &nobody);
        assert!(doors.open().is_empty());
    }
}
