//! How walkers move (pivot plan §5.2): a social-force model with anticipation.
//!
//! Each tick every walker sums, from the snapshot of the previous tick (so the update
//! order does not matter):
//! - a **goal** force toward the first corner of its way, relaxing its velocity to its
//!   desired one in `RELAX_S` (Helbing & Molnár 1995, Physical Review E 51(5));
//! - an **anticipation** force against each neighbour it would pass too close to within
//!   `TTC_HORIZON` if both kept their velocity: it pushes away from where the other
//!   will be at the closest approach, harder the sooner and the closer (after the
//!   anticipation of Karamouzas, Skinner & Guy 2014, Physical Review Letters 113,
//!   238701, and the closest-approach rule of Moussaïd, Helbing & Theraulaz 2011, PNAS
//!   108(17)). It fades out smoothly as the predicted gap opens: a force that switches
//!   off when the predicted paths stop touching made walkers flip from side to side at
//!   every tick. A walker meeting someone head-on also steps to its right
//!   (`KEEP_RIGHT`): in France people pass on the right (Moussaïd et al. 2009, Proc. R.
//!   Soc. B 276), which is what lets opposite flows sort into lanes quickly. Only
//!   polynomials and square roots, so every platform gets the same bits;
//! - a **detour** around a fixed obstacle in the way: the walker aims beside it, on the
//!   side that leaves room for its body (the shorter way round; the right one when the
//!   two are within 0.3 m), and keeps that side until it is past;
//! - short-range **body** and **wall** forces that grow as the gap closes;
//! - for groups, a **cohesion** force toward the group's middle and a pace that waits for
//!   the ones behind.
//!
//! The local **density** slows the desired speed along Weidmann's fundamental diagram.
//! Then velocities are integrated (capped), and positions go through a few passes of
//! constraints that part overlapping bodies and push bodies out of walls and obstacles
//! (as position-based crowd models do). A walker that would still end outside the
//! walkable space stays where it was. Velocities are then what the bodies really moved.

use glam::Vec2;

use super::StreetRun;
use super::detect::Body;
use super::drive::DT;
use super::drive::Seen;
use super::events::{CauseKind, StreetEvent, rounded, rounded_point};
use super::grid::Grid;
use super::kerb::KerbChoice;
use super::walk::{WalkSpace, Wall};
use super::walker::{Walker, WalkerSeen};
use super::{CONFLICT_COOLDOWN_S, GIVE_UP_S};
use crate::geom::polygon::segment_intersection;
use crate::geom::segment::closest_point_to_segment;
use crate::street::ids::AgentId;

/// Seconds to reach the desired velocity (Helbing & Molnár 1995: 0.5 s).
pub const RELAX_S: f32 = 0.5;
/// Neighbours further than this are not anticipated, metres (start value).
pub const ANTICIPATION_RANGE: f32 = 5.0;
/// Meetings further ahead than this are ignored, seconds (Karamouzas et al. 2014
/// found interactions fade after about 3 s).
pub const TTC_HORIZON: f32 = 3.0;
/// Anticipation force scale, m/s² (start value).
pub const ANTICIPATION_STRENGTH: f32 = 1.2;
/// Times to the closest approach below this count as this (keeps the force finite).
const TTC_FLOOR: f32 = 0.25;
/// Room kept around a body when anticipating, metres (start value).
pub const PERSONAL_SPACE: f32 = 0.15;
/// The anticipation fades out over this much predicted gap beyond the bodies and their
/// personal space, metres (start value).
pub const ANTICIPATION_FADE: f32 = 0.4;
/// A detour passes this far from an obstacle, beyond the body, metres (start value).
pub const DETOUR_MARGIN: f32 = 0.2;
/// Group members further than this are not waited for: the group has split, and meets
/// again at the destination (start value).
pub const GROUP_REACH: f32 = 4.0;
/// How much a walker meeting someone head-on steps to its right (start value).
pub const KEEP_RIGHT: f32 = 0.4;
/// Body force: range of the gap and strength at contact, metres and m/s² (start values).
pub const BODY_RANGE: f32 = 0.3;
pub const BODY_STRENGTH: f32 = 4.0;
/// Wall and obstacle force, the same way (start values).
pub const WALL_RANGE: f32 = 0.3;
pub const WALL_STRENGTH: f32 = 5.0;
/// The total force is capped at this, m/s² (start value; a brisk start is about 1.5).
pub const MAX_ACCEL: f32 = 4.0;
/// A walker never goes faster than this times its desired speed.
pub const MAX_SPEED_FACTOR: f32 = 1.3;
/// Group cohesion: a member further than `GROUP_RADIUS` (+ 0.25 m per extra member) from
/// the group's middle is pulled back with this strength per metre (start values).
pub const GROUP_RADIUS: f32 = 0.5;
pub const GROUP_STRENGTH: f32 = 1.5;
/// A member this many metres ahead of the group's middle slows by this fraction per
/// metre (start value); one behind hurries the same way.
pub const GROUP_PACE: f32 = 0.35;
/// Density is counted in a circle of this radius, metres (start value).
pub const DENSITY_RADIUS: f32 = 1.0;
/// The density a walker feels follows the count in about this many seconds.
pub const DENSITY_SMOOTH_S: f64 = 1.0;
/// Above this many people per square metre a walker is in a dense crowd: Fruin's
/// level of service E and worse (less than 0.9 m² each; Fruin 1971, "Pedestrian
/// Planning and Design").
pub const DENSE_CROWD: f64 = 1.1;
/// Passes of the position constraints per tick.
pub const CONSTRAINT_PASSES: usize = 8;
/// A body this far into another one, before the constraints part them, is a bump,
/// metres (start value).
pub const BUMP_DEPTH: f32 = 0.03;
/// A walker this close to its destination line has arrived, metres.
pub const ARRIVE_DISTANCE: f32 = 0.6;
/// Slower than this, a walker counts as stuck (m/s).
pub const STUCK_SPEED: f64 = 0.1;
/// Annoyance: a second in a dense crowd weighs this many seconds of delay, a bump this
/// many seconds (start values).
pub const CROWD_WEIGHT: f64 = 0.5;
pub const BUMP_COST_S: f64 = 2.0;
/// A second of forced detour (the rule way's extra walk) weighs this many seconds of
/// delay (start value; pivot plan §5.6).
pub const DETOUR_WEIGHT: f64 = 1.0;
/// A vehicle slower than this is solid for walkers: they are pushed out of it instead
/// of touching it, m/s.
pub const STANDING_VEHICLE: f64 = 0.5;
/// A walker on the road stops for a moving vehicle body this close in front of it,
/// metres (start value).
pub const CAR_AHEAD: f32 = 1.0;
/// What a walker did wrong stays blamable this long, seconds (start value: a slow
/// crossing of a wide street).
pub const WALKER_CAUSE_S: f64 = 20.0;
/// A walker this close to the kerb in front of it (beyond its body) shows that it is
/// about to cross: drivers yield to it at a zebra, metres (start value).
pub const INTENT_REACH: f32 = 3.0;
/// Walkers further apart than this are not tested for overlap in a tick, metres.
const PAIR_REACH: f32 = 1.0;
/// Grid cells for walkers, metres: a query of the anticipation range reads about 6 × 6
/// cells.
pub const WALKER_CELL: f32 = 2.0;

/// Weidmann's fundamental diagram (1993): walking speed as a share of the free speed by
/// density (people per m²), v/v0 = 1 - exp(-1.913 (1/ρ - 1/5.4)), tabulated so the sim
/// needs no `exp`.
pub const WEIDMANN: &[(f64, f64)] = &[
    (0.0, 1.0),
    (0.5, 0.969),
    (1.0, 0.790),
    (1.5, 0.602),
    (2.0, 0.452),
    (3.0, 0.247),
    (4.0, 0.117),
    (5.4, 0.0),
];

/// The share of its free speed a walker keeps at `density`.
pub fn speed_share(density: f64) -> f64 {
    for pair in WEIDMANN.windows(2) {
        let ((d0, s0), (d1, s1)) = (pair[0], pair[1]);
        if density <= d1 {
            let t = ((density - d0) / (d1 - d0)).clamp(0.0, 1.0);
            return s0 + (s1 - s0) * t;
        }
    }
    0.0
}

/// Measures of how the crowd model behaves, for the tests and the 20 Hz question.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WalkStats {
    /// The deepest two bodies went into each other from the forces alone, before the
    /// constraints, metres.
    pub max_force_overlap: f32,
    /// The deepest overlap left after the constraints, metres.
    pub max_overlap: f32,
    /// Walkers kept in place because the constraints would have left them outside.
    pub held_back: u32,
    /// Ticks where a walker's force turned by more than 90 degrees (both above 1 m/s²),
    /// and all walker ticks with a force above 1 m/s²: their ratio shows oscillation.
    pub force_flips: u32,
    pub force_samples: u32,
    /// Ticks where a walker moving faster than 0.2 m/s reversed (its velocity turned by
    /// more than 120 degrees from the tick before; a sidestep in a crush turns by about
    /// 90), and all walker ticks above that speed.
    pub velocity_flips: u32,
    pub velocity_samples: u32,
}

/// The time of the closest approach of two bodies moving at constant velocities
/// (`rel` = the other's position minus one's own, `closing` = one's velocity minus the
/// other's) and the gap vector then (from one to the other), if they are coming closer
/// and it lies within the horizon.
fn closest_approach(rel: Vec2, closing: Vec2) -> Option<(f32, Vec2)> {
    let speed2 = closing.dot(closing);
    let toward = rel.dot(closing);
    if speed2 < 1e-6 || toward <= 0.0 {
        return None;
    }
    let t = toward / speed2;
    (t < TTC_HORIZON).then(|| (t, rel - closing * t))
}

/// The anticipation push on a walker heading `heading` against a body at `rel`: away
/// from where the other will be at the closest approach, weighted by how soon and how
/// close (both fade to zero, so the force is continuous in the velocities).
fn anticipation(rel: Vec2, closing: Vec2, reach: f32, heading: Vec2) -> Vec2 {
    let Some((t, miss)) = closest_approach(rel, closing) else {
        return Vec2::ZERO;
    };
    let gap = miss.length();
    let near = ((reach + ANTICIPATION_FADE - gap) / ANTICIPATION_FADE).clamp(0.0, 1.0);
    if near == 0.0 {
        return Vec2::ZERO;
    }
    // `miss` is small when the two meet head-on: then the step to the right decides.
    let head_on = (rel.normalize_or_zero().dot(heading)).max(0.0);
    let away = (-miss / reach + heading.perp() * (KEEP_RIGHT * head_on)).normalize_or_zero();
    let soon = (TTC_HORIZON - t) / (TTC_HORIZON * t.max(TTC_FLOOR));
    away * (ANTICIPATION_STRENGTH * soon * near * near)
}

/// What a walker decided this tick.
struct Decided {
    force: Vec2,
    density: f64,
    kerb: Option<KerbChoice>,
    /// The obstacle it goes around and on which side (true = right).
    detour: Option<(usize, bool)>,
}

/// The push of a gap that closes under `range`.
fn gap_force(gap: f32, range: f32, strength: f32) -> f32 {
    if gap >= range {
        return 0.0;
    }
    let t = ((range - gap) / range).min(2.0);
    strength * t * t
}

/// The closest point of a wall and whether the point is beyond its middle part.
fn wall_contact(p: Vec2, a: Vec2, b: Vec2) -> (Vec2, bool) {
    let q = closest_point_to_segment(p, a, b);
    let interior = q != a && q != b;
    (q, interior)
}

/// The point of a vehicle body nearest to `p` (`p` itself when inside).
fn nearest_on_body(body: &Body, p: Vec2) -> Vec2 {
    let local = p - body.center;
    let side = body.dir.perp();
    let along = local
        .dot(body.dir)
        .clamp(-body.half_length, body.half_length);
    let across = local.dot(side).clamp(-body.half_width, body.half_width);
    body.center + body.dir * along + side * across
}

/// Pushes a body out of a vehicle body it went into: to the side nearest to where it
/// was at the start of the tick.
fn push_out_of_vehicle(body: &Body, p: Vec2, before: Vec2, r: f32) -> Vec2 {
    let q = nearest_on_body(body, p);
    let d = p - q;
    let dist = d.length();
    if dist >= r {
        return p;
    }
    if dist > 1e-6 && !circle_inside(body, p) {
        return q + d / dist * r;
    }
    // The centre went inside: back out through the side it came from, or, when it was
    // inside already, through the nearest side.
    let q = nearest_on_body(body, before);
    let d = before - q;
    if d.length() > 1e-6 {
        return q + d.normalize() * r;
    }
    let local = p - body.center;
    let side = body.dir.perp();
    let (along, across) = (local.dot(body.dir), local.dot(side));
    let to_end = body.half_length - along.abs();
    let to_side = body.half_width - across.abs();
    if to_end < to_side {
        p + body.dir * (along.signum() * (to_end + r))
    } else {
        p + side * (across.signum() * (to_side + r))
    }
}

fn circle_inside(body: &Body, p: Vec2) -> bool {
    let local = p - body.center;
    local.dot(body.dir).abs() < body.half_length
        && local.dot(body.dir.perp()).abs() < body.half_width
}

/// The walls a walker may not cross: every wall, and the kerbs of the road areas not in
/// `open` (`WalkSpace::open_roads`).
fn barriers<'a>(space: &'a WalkSpace, open: &'a [usize]) -> impl Iterator<Item = Wall> + 'a {
    space.walls.iter().copied().chain(
        space
            .kerbs
            .iter()
            .filter_map(move |kerb| space.barrier_for(kerb, open)),
    )
}

/// Pushes a body out of walls and obstacles. A wall the body went through during this
/// tick (from `before`, where it was at the start of the tick, on the wall's walkable
/// side) is undone; a body merely near the back of a wall (a thin barrier) stays on its
/// own side.
fn push_out(space: &WalkSpace, mut p: Vec2, before: Vec2, r: f32, open: &[usize]) -> Vec2 {
    for wall in barriers(space, open) {
        let came_through = (before - wall.a).dot(wall.normal) >= 0.0
            && segment_intersection(before, p, wall.a, wall.b).is_some();
        let q = closest_point_to_segment(p, wall.a, wall.b);
        let d = p - q;
        let dist = d.length();
        if came_through {
            p = q + wall.normal * r;
        } else if dist < r {
            let out = if dist > 1e-6 { d / dist } else { wall.normal };
            p = q + out * r;
        }
    }
    for round in &space.circles {
        let d = p - round.center;
        let dist = d.length();
        let reach = round.radius + r;
        if dist < reach {
            let out = if dist > 1e-6 { d / dist } else { Vec2::X };
            p = round.center + out * reach;
        }
    }
    p
}

impl StreetRun {
    /// The force a walker asks for, from the previous tick's snapshot, and the density
    /// around it.
    fn walker_force(
        &self,
        walker: &Walker,
        seen: &[WalkerSeen],
        grid: &Grid,
        vehicles: &[Seen],
    ) -> Decided {
        let kerb = self.kerb_choice(walker);
        let path = &self.walk_paths[walker.path];
        let going = matches!(kerb, Some(KerbChoice::Go { .. }));
        let open = self.walk_space.open_roads(
            path,
            walker.area_index,
            going || walker.cleared_kerb == Some(walker.area_index),
        );
        let (target, _) = Self::walk_target(path, walker.area_index, walker.pos);
        let (target, detour) = self.detour(walker, target);
        let to_target = (target - walker.pos).normalize_or_zero();
        let heading = if walker.vel.length_squared() > 0.01 {
            walker.vel.normalize()
        } else {
            to_target
        };
        let (p, v, r) = (walker.pos, walker.vel, walker.radius);
        let mut force = Vec2::ZERO;
        let mut near = 0u32;
        let mut found = Vec::new();
        grid.query(p, ANTICIPATION_RANGE, &mut found);
        for &index in &found {
            let other = &seen[index as usize];
            if other.id == walker.id {
                continue;
            }
            let rel = other.pos - p;
            let dist = rel.length();
            if dist > ANTICIPATION_RANGE {
                continue;
            }
            if dist < DENSITY_RADIUS {
                near += 1;
            }
            force += anticipation(
                rel,
                v - other.vel,
                r + other.radius + PERSONAL_SPACE,
                heading,
            );
            let gap = dist - r - other.radius;
            if dist > 1e-6 {
                force -= rel / dist * gap_force(gap, BODY_RANGE, BODY_STRENGTH);
            }
        }
        let space = &self.walk_space;
        // Vehicle bodies: people keep off them like off a wall, and one on the road stops
        // for a moving one right in front of it (it does not walk into its side).
        let mut car_ahead = false;
        for vehicle in vehicles {
            if vehicle.pos.distance(p) > (vehicle.length * 0.5) as f32 + 2.0 {
                continue;
            }
            let body = Body {
                center: vehicle.pos,
                dir: vehicle.dir,
                half_length: (vehicle.length * 0.5) as f32,
                half_width: (vehicle.width * 0.5) as f32,
            };
            let d = p - nearest_on_body(&body, p);
            let dist = d.length();
            if dist > 1e-6 {
                force += d / dist * gap_force(dist - r, WALL_RANGE, WALL_STRENGTH);
                if vehicle.speed > STANDING_VEHICLE
                    && dist - r < CAR_AHEAD
                    && (-d).dot(to_target) > 0.0
                {
                    car_ahead = true;
                }
            }
        }
        for round in &space.circles {
            let rel = p - round.center;
            let dist = rel.length();
            if dist > 1e-6 {
                force += rel / dist * gap_force(dist - round.radius - r, WALL_RANGE, WALL_STRENGTH);
            }
        }
        for wall in barriers(space, &open) {
            let (q, _) = wall_contact(p, wall.a, wall.b);
            let d = p - q;
            let dist = d.length();
            if dist - r < WALL_RANGE {
                let out = if dist > 1e-6 && d.dot(wall.normal) >= 0.0 {
                    d / dist
                } else {
                    wall.normal
                };
                force += out * gap_force(dist - r, WALL_RANGE, WALL_STRENGTH);
            }
        }
        let area = std::f64::consts::PI * f64::from(DENSITY_RADIUS * DENSITY_RADIUS);
        // The others only: a walker alone walks at its free speed.
        let density = f64::from(near) / area;
        let mut pace = speed_share(walker.density) as f32;
        if walker.members.len() > 1 {
            let mut middle = Vec2::ZERO;
            let mut count = 0;
            for id in &walker.members {
                if let Ok(index) = seen.binary_search_by_key(id, |s| s.id)
                    && seen[index].pos.distance(p) <= GROUP_REACH
                {
                    middle += seen[index].pos;
                    count += 1;
                }
            }
            if count > 1 {
                middle /= count as f32;
                let radius = GROUP_RADIUS + 0.25 * (count - 1) as f32;
                let off = middle - p;
                let dist = off.length();
                if dist > radius {
                    force += off / dist * (GROUP_STRENGTH * (dist - radius));
                }
                let ahead = (p - middle).dot(to_target);
                pace *= (1.0 - GROUP_PACE * ahead).clamp(0.6, 1.25);
            }
        }
        if kerb == Some(KerbChoice::Wait) || (car_ahead && !open.is_empty()) {
            pace = 0.0;
        }
        let desired = to_target * (walker.desired_speed as f32 * pace);
        force += (desired - v) / RELAX_S;
        Decided {
            force: force.clamp_length_max(MAX_ACCEL),
            density,
            kerb,
            detour,
        }
    }

    /// Where to aim instead of `target` when a fixed obstacle stands in the way: beside
    /// it, on a side with room for the body. The side chosen before is kept while the
    /// same obstacle is in the way.
    fn detour(&self, walker: &Walker, target: Vec2) -> (Vec2, Option<(usize, bool)>) {
        let space = &self.walk_space;
        let (p, r) = (walker.pos, walker.radius);
        let to = target - p;
        let length = to.length();
        if length < 1e-3 {
            return (target, None);
        }
        let along = to / length;
        let clearance = r + DETOUR_MARGIN;
        let mut blocking: Option<(usize, (f32, f32, f32, f32))> = None;
        for index in 0..space.obstacle_count() {
            let extent = space.obstacle_extent(index, p, along);
            let (near, far, left, right) = extent;
            if far <= 0.0 || near >= length || right <= -r || left >= r {
                continue;
            }
            if blocking.is_none_or(|(_, e)| near < e.0) {
                blocking = Some((index, extent));
            }
        }
        let Some((index, (near, far, left, right))) = blocking else {
            return (target, None);
        };
        // Aim beside its near end while coming up to it (a line to its middle could cut
        // the corner), beside its far end once alongside.
        let ahead = if near > 0.0 { near } else { far };
        let side_point = |right_side: bool| {
            let offset = if right_side {
                right + clearance
            } else {
                left - clearance
            };
            (p + along * ahead + along.perp() * offset, offset.abs())
        };
        let free = |q: Vec2| {
            space.contains(q) && space.wall_distance(q) >= r && space.obstacle_depth(q, r) <= 0.0
        };
        if let Some((kept, right_side)) = walker.detour
            && kept == index
        {
            let (q, _) = side_point(right_side);
            if free(q) {
                return (q, walker.detour);
            }
        }
        let (rq, rd) = side_point(true);
        let (lq, ld) = side_point(false);
        match (free(rq), free(lq)) {
            (true, true) if ld + 0.3 < rd => (lq, Some((index, false))),
            (true, _) => (rq, Some((index, true))),
            (false, true) => (lq, Some((index, false))),
            (false, false) => (target, None),
        }
    }

    /// Walkers decide from the previous tick and move; then the constraints.
    pub(super) fn walk_stage(&mut self) {
        if self.walkers.is_empty() {
            return;
        }
        let snapshot = self.history.back().expect("there is always a snapshot");
        let decided: Vec<Decided> = self
            .walkers
            .values()
            .map(|w| self.walker_force(w, &snapshot.walkers, &snapshot.walker_grid, &snapshot.seen))
            .collect();
        let mut stats = self.walk_stats;
        let tick = self.tick;
        let starts: Vec<StreetEvent> = self
            .walkers
            .values()
            .zip(&decided)
            .filter_map(|(walker, d)| match d.kerb {
                Some(KerbChoice::Go { gap_s, on_red, .. }) => {
                    let (segment, crossing) = self.crossing_ahead(walker);
                    Some(StreetEvent::CrossingStarted {
                        agent: walker.id,
                        segment,
                        crossing,
                        position: rounded_point(walker.pos),
                        on_red,
                        gap_s: gap_s.map(|g| rounded(g, 100.0)),
                        waited_s: rounded(
                            walker
                                .kerb_since
                                .map_or(0.0, |since| (tick - since) as f64 * DT),
                            100.0,
                        ),
                    })
                }
                _ => None,
            })
            .collect();
        for (
            walker,
            Decided {
                force,
                density,
                kerb,
                detour,
            },
        ) in self.walkers.values_mut().zip(decided)
        {
            walker.detour = detour;
            walker
                .recent
                .retain(|(t, _)| (tick - t) as f64 * DT <= WALKER_CAUSE_S);
            if walker.hit() {
                // It stays where it fell.
                walker.vel = Vec2::ZERO;
                walker.vel_before = Vec2::ZERO;
                walker.accel = Vec2::ZERO;
                walker.prev_pos = walker.pos;
                walker.waiting = false;
                continue;
            }
            walker.waiting = kerb == Some(KerbChoice::Wait);
            match kerb {
                Some(KerbChoice::Wait) => {
                    walker.kerb_since.get_or_insert(tick);
                    walker.kerb_wait_ticks += 1;
                    walker.frustration =
                        (walker.frustration + DT / walker.traits.patience_s).min(1.0);
                }
                Some(KerbChoice::Go {
                    gap_s,
                    on_red,
                    short_gap,
                }) => {
                    walker.cleared_kerb = Some(walker.area_index);
                    walker.kerb_since = None;
                    if on_red {
                        walker.recent.push((tick, CauseKind::WalkedOnRed));
                    }
                    if short_gap {
                        let gap_s = rounded(gap_s.unwrap_or(0.0), 100.0);
                        walker
                            .recent
                            .push((tick, CauseKind::AcceptedShortGap { gap_s }));
                    }
                }
                None => walker.frustration = (walker.frustration - DT / 60.0).max(0.0),
            }
            let (old, new) = (walker.accel, force);
            if old.length() > 1.0 && new.length() > 1.0 {
                stats.force_samples += 1;
                if old.dot(new) < 0.0 {
                    stats.force_flips += 1;
                }
            }
            walker.accel = force;
            walker.vel_before = walker.vel;
            walker.density += (density - walker.density) * (DT / DENSITY_SMOOTH_S).min(1.0);
            let cap = (walker.desired_speed as f32 * MAX_SPEED_FACTOR).max(0.1);
            walker.vel = (walker.vel + force * DT as f32).clamp_length_max(cap);
            walker.prev_pos = walker.pos;
            walker.pos += walker.vel * DT as f32;
        }
        // Bodies the forces let into each other: bumps, and the measure of the forces.
        let mut bumps: Vec<(AgentId, AgentId, Vec2)> = Vec::new();
        let pairs = self.close_pairs();
        let overlaps = self.overlaps(&pairs);
        for &(a, b, depth, at) in &overlaps {
            stats.max_force_overlap = stats.max_force_overlap.max(depth);
            if depth > BUMP_DEPTH {
                bumps.push((a, b, at));
            }
        }
        // Standing vehicles are solid for walkers (a moving one is a contact instead).
        let standing: Vec<Body> = self
            .agents
            .values()
            .filter(|car| car.speed < STANDING_VEHICLE)
            .map(|car| Body {
                center: car.pos,
                dir: car.dir,
                half_length: (car.length * 0.5) as f32,
                half_width: (car.width * 0.5) as f32,
            })
            .collect();
        for _ in 0..CONSTRAINT_PASSES {
            self.part_bodies(&pairs);
            for walker in self.walkers.values_mut() {
                if walker.hit() {
                    continue;
                }
                let path = &self.walk_paths[walker.path];
                let cleared = walker.cleared_kerb == Some(walker.area_index);
                let open = self.walk_space.open_roads(path, walker.area_index, cleared);
                walker.pos = push_out(
                    &self.walk_space,
                    walker.pos,
                    walker.prev_pos,
                    walker.radius,
                    &open,
                );
                for body in &standing {
                    if body.center.distance(walker.pos) < body.half_length + 2.0 {
                        walker.pos =
                            push_out_of_vehicle(body, walker.pos, walker.prev_pos, walker.radius);
                    }
                }
            }
        }
        for walker in self.walkers.values_mut() {
            let path = &self.walk_paths[walker.path];
            let cleared = walker.cleared_kerb == Some(walker.area_index);
            let open = self.walk_space.open_roads(path, walker.area_index, cleared);
            if !self.walk_space.contains_for(walker.pos, &open) {
                walker.pos = walker.prev_pos;
                stats.held_back += 1;
            }
            let before = walker.vel_before;
            walker.vel = (walker.pos - walker.prev_pos) / DT as f32;
            if before.length() > 0.2 && walker.vel.length() > 0.2 {
                stats.velocity_samples += 1;
                if before.dot(walker.vel) < -0.5 * before.length() * walker.vel.length() {
                    stats.velocity_flips += 1;
                }
            }
            let path = &self.walk_paths[walker.path];
            let last = path.areas.len() - 1;
            for k in walker.area_index + 1..=(walker.area_index + 3).min(last) {
                if self.walk_space.areas[path.areas[k]]
                    .polygon
                    .contains(walker.pos)
                {
                    walker.area_index = k;
                }
            }
            if walker.density > DENSE_CROWD {
                walker.crowded_ticks += 1;
            }
            if walker.waiting || walker.speed() < STUCK_SPEED {
                walker.stuck_ticks += 1;
            } else {
                walker.stuck_ticks = 0;
            }
        }
        for (_, _, depth, _) in self.overlaps(&pairs) {
            stats.max_overlap = stats.max_overlap.max(depth);
        }
        self.walk_stats = stats;
        for event in starts {
            if let StreetEvent::CrossingStarted {
                agent,
                crossing: None,
                ..
            } = event
                && let Some(walker) = self.walkers.get_mut(&agent)
            {
                walker.recent.push((tick, CauseKind::CrossedOutOfCrossing));
            }
            self.log(event);
        }
        let cooldown = (CONFLICT_COOLDOWN_S / DT).round() as u64;
        self.bump_last.retain(|_, last| tick - *last < cooldown);
        for (a, b, at) in bumps {
            if self
                .bump_last
                .get(&(a, b))
                .is_some_and(|last| tick - last < cooldown)
            {
                continue;
            }
            self.bump_last.insert((a, b), tick);
            for id in [a, b] {
                if let Some(walker) = self.walkers.get_mut(&id) {
                    walker.bumps += 1;
                }
            }
            self.log(StreetEvent::Bump {
                a,
                b,
                position: rounded_point(at),
            });
        }
    }

    /// Pairs of walkers (indices in id order, lower first) close enough to touch during
    /// the constraint passes, found once per tick: the passes move bodies by far less
    /// than `PAIR_REACH` less two radii.
    fn close_pairs(&self) -> Vec<(usize, usize)> {
        let mut grid = Grid::with_cell(self.street.bounds, WALKER_CELL);
        for (index, walker) in self.walkers.values().enumerate() {
            grid.insert(index as u32, walker.pos);
        }
        let positions: Vec<Vec2> = self.walkers.values().map(|w| w.pos).collect();
        let mut found = Vec::new();
        let mut pairs = Vec::new();
        for (i, p) in positions.iter().enumerate() {
            grid.query(*p, PAIR_REACH, &mut found);
            for &j in &found {
                let j = j as usize;
                if j > i && positions[j].distance(*p) < PAIR_REACH {
                    pairs.push((i, j));
                }
            }
        }
        pairs
    }

    /// The pairs whose bodies overlap now: (lower id, higher id, depth, the point between).
    fn overlaps(&self, pairs: &[(usize, usize)]) -> Vec<(AgentId, AgentId, f32, Vec2)> {
        let walkers: Vec<&Walker> = self.walkers.values().collect();
        pairs
            .iter()
            .filter_map(|&(i, j)| {
                let (a, b) = (walkers[i], walkers[j]);
                let depth = a.radius + b.radius - a.pos.distance(b.pos);
                (depth > 0.0).then(|| (a.id, b.id, depth, (a.pos + b.pos) * 0.5))
            })
            .collect()
    }

    /// One pass that moves every overlapping pair apart by half the overlap each, all
    /// from the same positions (so the order does not matter).
    fn part_bodies(&mut self, pairs: &[(usize, usize)]) {
        let bodies: Vec<(Vec2, f32)> = self.walkers.values().map(|w| (w.pos, w.radius)).collect();
        let mut moves = vec![Vec2::ZERO; bodies.len()];
        for &(i, j) in pairs {
            let ((p, r), (q, s)) = (bodies[i], bodies[j]);
            let d = p - q;
            let dist = d.length();
            let overlap = r + s - dist;
            if overlap <= 0.0 {
                continue;
            }
            let out = if dist > 1e-6 { d / dist } else { -Vec2::X };
            moves[i] += out * (overlap * 0.5);
            moves[j] -= out * (overlap * 0.5);
        }
        for (walker, step) in self.walkers.values_mut().zip(moves) {
            if !walker.hit() {
                walker.pos += step;
            }
        }
    }

    /// Walkers who arrived or gave up leave.
    pub(super) fn walk_removal_stage(&mut self) {
        let tick = self.tick;
        let ids: Vec<AgentId> = self.walkers.keys().copied().collect();
        for id in ids {
            let walker = &self.walkers[&id];
            let path = &self.walk_paths[walker.path];
            let goal = closest_point_to_segment(walker.pos, path.goal.0, path.goal.1);
            let event = if let Some(until) = walker.hit_until {
                if until > tick {
                    continue;
                }
                StreetEvent::Cleared { agent: id }
            } else if walker.pos.distance(goal) <= ARRIVE_DISTANCE {
                let time_s = (tick - walker.spawn_tick) as f64 * DT;
                let delay_s = (time_s - walker.free_time_s).max(0.0);
                let crowded_s = f64::from(walker.crowded_ticks) * DT;
                let annoyance = (delay_s
                    + CROWD_WEIGHT * crowded_s
                    + BUMP_COST_S * f64::from(walker.bumps)
                    + DETOUR_WEIGHT * walker.detour_s)
                    / walker.traits.patience_s;
                StreetEvent::WalkEnded {
                    agent: id,
                    time_s: rounded(time_s, 100.0),
                    delay_s: rounded(delay_s, 100.0),
                    crowded_s: rounded(crowded_s, 100.0),
                    bumps: walker.bumps,
                    kerb_wait_s: rounded(f64::from(walker.kerb_wait_ticks) * DT, 100.0),
                    detour_s: rounded(walker.detour_s, 100.0),
                    annoyance: rounded(annoyance, 100.0),
                }
            } else if f64::from(walker.stuck_ticks) * DT > GIVE_UP_S {
                StreetEvent::GaveUp {
                    agent: id,
                    waited_s: rounded(f64::from(walker.stuck_ticks) * DT, 100.0),
                }
            } else {
                continue;
            };
            self.walkers.shift_remove(&id);
            self.log(event);
        }
    }

    /// What walkers will see of each other next tick.
    pub(super) fn walker_snapshot(&self) -> (Vec<WalkerSeen>, Grid) {
        let mut grid = Grid::with_cell(self.street.bounds, WALKER_CELL);
        let seen = self
            .walkers
            .values()
            .enumerate()
            .map(|(index, walker)| {
                grid.insert(index as u32, walker.pos);
                let path = &self.walk_paths[walker.path];
                let k = walker.area_index;
                let next = path.areas.get(k + 1).copied();
                let about_to_step = path.kerbs.get(k).copied().unwrap_or(false) && {
                    let (l, r) = path.portals[k];
                    walker.kerb_since.is_some()
                        || walker.cleared_kerb == Some(k)
                        || closest_point_to_segment(walker.pos, l, r).distance(walker.pos)
                            < walker.radius + INTENT_REACH
                };
                WalkerSeen {
                    id: walker.id,
                    pos: walker.pos,
                    vel: walker.vel,
                    radius: walker.radius,
                    group: walker.group,
                    area: Some(path.areas[k]),
                    intent: if about_to_step { next } else { None },
                    hit: walker.hit(),
                }
            })
            .collect();
        (seen, grid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_walker_found_inside_a_standing_vehicle_is_pushed_out_of_it() {
        // Stage C review, finding 3: both where it is and where it was are inside.
        let body = Body {
            center: Vec2::ZERO,
            dir: Vec2::X,
            half_length: 2.15,
            half_width: 0.9,
        };
        let out = push_out_of_vehicle(&body, Vec2::new(0.5, 0.6), Vec2::new(0.4, 0.5), 0.25);
        assert!(!circle_inside(&body, out), "{out:?}");
        assert!(
            (nearest_on_body(&body, out) - out).length() >= 0.25 - 1e-4,
            "{out:?}"
        );
    }

    #[test]
    fn the_fundamental_diagram_slows_walkers_as_density_grows() {
        assert_eq!(speed_share(0.0), 1.0);
        assert!((speed_share(1.0) - 0.79).abs() < 1e-9);
        assert!(speed_share(2.5) < speed_share(2.0));
        assert_eq!(speed_share(6.0), 0.0);
    }

    #[test]
    fn two_walkers_on_a_collision_course_feel_it_before_touching() {
        // Head-on, 4 m apart, 2.6 m/s closing: the closest approach is in about 1.5 s.
        let (t, miss) =
            closest_approach(Vec2::new(4.0, 0.0), Vec2::new(2.6, 0.0)).expect("they meet");
        assert!((t - 4.0 / 2.6).abs() < 1e-5 && miss.length() < 1e-5);
        // Walking apart, or passing wide: nothing.
        assert!(closest_approach(Vec2::new(4.0, 0.0), Vec2::new(-1.0, 0.0)).is_none());
        let wide = anticipation(Vec2::new(4.0, 2.0), Vec2::new(2.6, 0.0), 0.65, Vec2::X);
        assert_eq!(wide, Vec2::ZERO);
        // Head-on, the push has a part to the walker's right (+y when heading +x, y down).
        let push = anticipation(Vec2::new(4.0, 0.0), Vec2::new(2.6, 0.0), 0.65, Vec2::X);
        assert!(push.y > 0.0, "{push:?}");
    }

    #[test]
    fn the_anticipation_fades_smoothly_as_the_paths_part() {
        let push = |offset: f32| {
            anticipation(Vec2::new(4.0, offset), Vec2::new(2.6, 0.0), 0.65, Vec2::X).length()
        };
        let mut before = push(0.3);
        for step in 1..=20 {
            let now = push(0.3 + step as f32 * 0.05);
            assert!(now <= before + 1e-6, "it only weakens as the gap opens");
            assert!(before - now < 0.4, "no jump at {step}: {before} -> {now}");
            before = now;
        }
        assert_eq!(push(0.65 + ANTICIPATION_FADE + 0.01), 0.0);
    }
}
