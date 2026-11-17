//! Where pedestrians may walk, and how they find their way (pivot plan §3.3, §5.2).
//!
//! The walkable space is a set of convex areas. **Foot areas** are the sidewalk and
//! terrace strips (as `Street::walkable_polygons` gives them) and the level's plazas.
//! **Road areas** cover the carriageway between two sidewalks: a **crossing** area for
//! each crossing placed on the street, and **carriageway** pieces for the rest of it.
//! Obstacles are taken out of the space. Walls are the parts of the area outlines that do
//! not open onto another area, plus the outlines of the obstacles.
//!
//! **Kerbs** are the edges between a foot area and a road area. They are walls for every
//! walker except the one that has decided to cross there (`open_roads`): a walker on the
//! sidewalk never drifts into the street. A walker steps over a kerb only after waiting
//! for its gap (`kerb`), on a crossing by the rules or anywhere else (jaywalking).
//!
//! The walk graph: the points are the entries of the gates and points along the portals
//! (the shared edges between two areas; long ones, like a kerb, get a point every
//! `PORTAL_STEP`), and two points are joined when one area holds both. A* (the kept
//! `routing::astar`) finds the cheapest chain, which gives the corridor of areas a walker
//! goes through. Two ways are found for every trip: the **rule** way, which crosses
//! streets only on crossings (and on carriageways where the law allows it, see
//! `walker`), and the **free** way, which may cross a carriageway anywhere and costs
//! `ROAD_COST` times its length there (people walk on the sidewalk, not down the
//! street). Inside the corridor the walker steers to the first corner of the shortest
//! path through the remaining portals (the "simple stupid funnel" of M. Mononen, 2010),
//! so it cuts corners like a person and never walks into a wall corner. Areas are convex
//! as long as their segment is straight; a curved street would need its strips cut in
//! pieces (not needed by the levels so far).

use glam::Vec2;

use crate::geom::Polygon;
use crate::geom::lanes as lane_builder;
use crate::geom::segment::closest_point_to_segment;
use crate::street::astar::AStar;
use crate::street::level::{Gate, JunctionControl, Shape};
use crate::street::model::Street;
use crate::street::space::StripUse;

/// A crossing whose near edge is this close to a junction under signals gets the
/// junction's pedestrian lights, metres (start value).
pub const SIGNAL_REACH: f32 = 10.0;
/// Long portals get a graph point every this many metres, so a way that crosses a kerb
/// can do it where it wants to (start value).
pub const PORTAL_STEP: f32 = 4.0;
/// Walking a metre down a carriageway costs this many metres on the sidewalk, in the
/// free way (start value).
pub const ROAD_COST: f32 = 1.5;
/// Carriageway pieces shorter than this (between two crossings) are left out.
const MIN_PIECE: f32 = 0.3;
/// A walker steers this far from the ends of a portal, so it does not graze the corner
/// of a wall. Start value: a body radius and a little more.
pub const CORNER_CLEARANCE: f32 = 0.35;
/// A gate this close to an end node of its segment is a street end: walkers enter on
/// every sidewalk of that end. Other gates are doors on the nearest walkable edge.
pub const END_GATE_RADIUS: f32 = 1.0;
/// A door gate opens this wide along the building line, metres (start value).
pub const DOOR_WIDTH: f32 = 2.0;
/// A door gate further than this from every walkable edge has no entry.
const DOOR_REACH: f32 = 1.0;
/// Shared edges shorter than this are not portals (areas that only touch at a corner).
const MIN_PORTAL: f32 = 0.3;
/// Outlines are tested for walls in pieces this long.
const WALL_PIECE: f32 = 0.25;
/// How far beside an outline the wall test looks for another area.
const PROBE: f32 = 0.02;
/// Points this close to an edge's line count as on it (strips meet with float noise).
const ON_LINE: f32 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaKind {
    Sidewalk {
        segment: i32,
        strip: usize,
    },
    Plaza,
    /// A crossing on `segment` (its id in `Street::crossings`); `signal` is the junction
    /// whose pedestrian lights it follows.
    Crossing {
        id: i32,
        segment: i32,
        signal: Option<i32>,
    },
    /// The rest of a carriageway: walkers cross here out of a crossing.
    Carriageway {
        segment: i32,
    },
}

impl AreaKind {
    /// A carriageway or a crossing: a walker enters it only after the kerb.
    pub fn is_road(self) -> bool {
        matches!(
            self,
            AreaKind::Crossing { .. } | AreaKind::Carriageway { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WalkArea {
    pub kind: AreaKind,
    pub polygon: Polygon,
}

/// The edge two areas share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Portal {
    pub a: Vec2,
    pub b: Vec2,
    pub areas: (usize, usize),
}

/// A piece of outline that bodies may not cross. `normal` points into the walkable side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wall {
    pub a: Vec2,
    pub b: Vec2,
    pub normal: Vec2,
}

/// An edge of a road area: to a foot area (a kerb proper; `inside` is the foot area) or
/// to another road area. `normal` points into `inside`. It stops a walker that may be
/// on one side and not on the other (`WalkSpace::barrier_for`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kerb {
    pub a: Vec2,
    pub b: Vec2,
    pub normal: Vec2,
    pub inside: usize,
    pub outside: usize,
}

/// Where walkers of a gate come in and go out: a line inside one area, `a` to `b`, with
/// `inward` pointing away from the gate into the area.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub gate: String,
    pub area: usize,
    pub a: Vec2,
    pub b: Vec2,
    pub inward: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Round {
    pub center: Vec2,
    pub radius: f32,
}

#[derive(Clone, Debug, Default)]
pub struct WalkSpace {
    pub areas: Vec<WalkArea>,
    pub portals: Vec<Portal>,
    pub walls: Vec<Wall>,
    pub kerbs: Vec<Kerb>,
    /// Round obstacles (trees, bollards).
    pub circles: Vec<Round>,
    /// The other obstacles; their outlines are also in `walls`.
    pub obstacle_polygons: Vec<Polygon>,
    pub entries: Vec<Entry>,
}

/// A walker's way from an entry to an entry of its destination gate.
#[derive(Clone, Debug, PartialEq)]
pub struct WalkPath {
    /// The corridor: the areas walked through, in order.
    pub areas: Vec<usize>,
    /// `portals[i]` leads from `areas[i]` to `areas[i + 1]`, as (left, right) seen when
    /// walking, already shortened by `CORNER_CLEARANCE` at both ends.
    pub portals: Vec<(Vec2, Vec2)>,
    /// `kerbs[i]`: portal `i` steps from a foot area onto a road area; the walker waits
    /// there for its gap.
    pub kerbs: Vec<bool>,
    /// The way crosses a carriageway out of a crossing.
    pub off_crossing: bool,
    /// The length of the way through the portal points, metres.
    pub length: f32,
    pub to_entry: usize,
    /// The destination line, shortened at both ends like the portals and moved
    /// `CORNER_CLEARANCE` into the area (a walker heads for the door, not the façade).
    pub goal: (Vec2, Vec2),
    /// Where a zebra on the way may be cut at its corner (see `CutWay`).
    pub cuts: Vec<CutWay>,
}

/// How far from a zebra's edge a walker may step onto the carriageway to cut its corner,
/// metres (start value: about a car's door width plus a stride). A walker is never
/// further off the zebra than this, so it always counts as crossing at the zebra.
pub const CUT_M: f32 = 2.5;

/// A zebra on a path whose corner a walker may cut: it steps over the kerb up to `CUT_M`
/// beside the zebra, on the carriageway piece next to it (on the side of its goal), and
/// crosses on a slight diagonal. The widened portals replace the zebra's two (kerb and
/// far kerb) in the steering only.
#[derive(Clone, Debug, PartialEq)]
pub struct CutWay {
    /// The index of the kerb portal onto the zebra (the far one is `kerb + 1`).
    pub kerb: usize,
    /// The carriageway area next to the zebra that the walker may also use.
    pub piece: usize,
    pub near: (Vec2, Vec2),
    pub far: (Vec2, Vec2),
}

/// A portal along the kerb line made wider by `CUT_M` toward the piece next to the
/// zebra: `base` is the zebra's portal (raw), `extra` the piece's one on the same kerb.
fn widened(base: &Portal, extra: &Portal, across: Vec2) -> (Vec2, Vec2) {
    let ends = [base.a, base.b];
    let others = [extra.a, extra.b];
    let mut best = (f32::INFINITY, 0, 0);
    for (i, e) in ends.iter().enumerate() {
        for (j, o) in others.iter().enumerate() {
            let d = e.distance(*o);
            if d < best.0 {
                best = (d, i, j);
            }
        }
    }
    let (shared, rest) = (ends[best.1], ends[1 - best.1]);
    let far = others[1 - best.2];
    let reach = shared.distance(far).min(CUT_M);
    let tip = shared + (far - shared).normalize_or_zero() * reach;
    let (a, b) = shorten(rest, tip, CORNER_CLEARANCE);
    if across.perp_dot(b - a) < 0.0 {
        (a, b)
    } else {
        (b, a)
    }
}

fn shorten(a: Vec2, b: Vec2, by: f32) -> (Vec2, Vec2) {
    let length = a.distance(b);
    if length <= 2.0 * by {
        let mid = (a + b) * 0.5;
        return (mid, mid);
    }
    let along = (b - a) / length;
    (a + along * by, b - along * by)
}

/// Twice the signed area of the triangle, with the sign of Mononen's `triarea2`.
fn triarea2(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    let (ax, ay) = (b.x - a.x, b.y - a.y);
    let (bx, by) = (c.x - a.x, c.y - a.y);
    bx * ay - ax * by
}

/// The shortest path from `start` to `end` through the portals (each as (left, right)),
/// corners only: the "simple stupid funnel" (Mononen 2010). The first point is `start`,
/// the last `end`.
pub fn string_pull(start: Vec2, portals: &[(Vec2, Vec2)], end: Vec2) -> Vec<Vec2> {
    let mut all = Vec::with_capacity(portals.len() + 2);
    all.push((start, start));
    all.extend_from_slice(portals);
    all.push((end, end));
    let mut points = vec![start];
    let (mut apex, mut left, mut right) = (start, start, start);
    let (mut left_index, mut right_index) = (0, 0);
    let mut i = 1;
    while i < all.len() {
        let (l, r) = all[i];
        if triarea2(apex, right, r) <= 0.0 {
            if apex == right || triarea2(apex, left, r) > 0.0 {
                right = r;
                right_index = i;
            } else {
                points.push(left);
                apex = left;
                (right, right_index) = (apex, left_index);
                i = left_index + 1;
                continue;
            }
        }
        if triarea2(apex, left, l) >= 0.0 {
            if apex == left || triarea2(apex, right, l) < 0.0 {
                left = l;
                left_index = i;
            } else {
                points.push(right);
                apex = right;
                (left, left_index) = (apex, right_index);
                i = right_index + 1;
                continue;
            }
        }
        i += 1;
    }
    if points.last() != Some(&end) {
        points.push(end);
    }
    points
}

pub fn path_length(points: &[Vec2]) -> f32 {
    points
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum()
}

/// The part of edge `a`-`b` that lies on edge `c`-`d` (same line, overlapping), if it is
/// long enough to walk through.
fn shared_edge(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<(Vec2, Vec2)> {
    let length = a.distance(b);
    if length < 1e-6 {
        return None;
    }
    let along = (b - a) / length;
    let off_line = |p: Vec2| along.perp_dot(p - a).abs();
    if off_line(c) > ON_LINE || off_line(d) > ON_LINE {
        return None;
    }
    let (tc, td) = (along.dot(c - a), along.dot(d - a));
    let low = tc.min(td).max(0.0);
    let high = tc.max(td).min(length);
    (high - low >= MIN_PORTAL).then(|| (a + along * low, a + along * high))
}

impl WalkSpace {
    pub fn new(street: &Street, gates: &[Gate]) -> WalkSpace {
        let mut space = WalkSpace::default();
        for segment in street.segments.values() {
            for (strip, s) in segment.profile.strips.iter().enumerate() {
                if matches!(s.use_, StripUse::Sidewalk | StripUse::Terrace) {
                    space.areas.push(WalkArea {
                        kind: AreaKind::Sidewalk {
                            segment: segment.id,
                            strip,
                        },
                        polygon: Polygon::new(segment.strip_polygon(strip)),
                    });
                }
            }
        }
        for plaza in &street.plazas {
            space.areas.push(WalkArea {
                kind: AreaKind::Plaza,
                polygon: plaza.clone(),
            });
        }
        space.add_road_areas(street);
        for obstacle in street.obstacles.values() {
            match &obstacle.shape {
                Shape::Circle(circle) => space.circles.push(Round {
                    center: circle.center,
                    radius: circle.radius,
                }),
                Shape::Polygon(polygon) => {
                    let inside = |p: Vec2| polygon.contains(p);
                    for (a, b) in polygon.edges() {
                        let normal = (b - a).perp().normalize_or_zero();
                        let mid = (a + b) * 0.5;
                        let normal = if inside(mid + normal * PROBE) {
                            -normal
                        } else {
                            normal
                        };
                        space.walls.push(Wall { a, b, normal });
                    }
                    space.obstacle_polygons.push(polygon.clone());
                }
            }
        }
        space.find_portals();
        space.find_walls();
        space.find_kerbs();
        for gate in gates {
            space.add_entries(street, gate);
        }
        space
    }

    /// Road areas of each run of non-walkable strips between two walkable ones: one per
    /// crossing on the segment, and carriageway pieces between them.
    fn add_road_areas(&mut self, street: &Street) {
        for segment in street.segments.values() {
            let walkable: Vec<bool> = segment
                .profile
                .strips
                .iter()
                .map(|s| matches!(s.use_, StripUse::Sidewalk | StripUse::Terrace))
                .collect();
            let length = lane_builder::polyline_length(&segment.centerline) as f32;
            let (from, to) = (segment.start_trim, length - segment.end_trim);
            let mut crossings: Vec<_> = street
                .crossings
                .values()
                .filter(|c| c.segment == segment.id)
                .collect();
            crossings.sort_by(|a, b| a.at.total_cmp(&b.at));
            let signal_at = |node: i32| {
                street
                    .junctions
                    .get(&node)
                    .is_some_and(|j| j.control == JunctionControl::Signals)
                    .then_some(node)
            };
            let mut i = 0;
            while i < walkable.len() {
                if walkable[i] {
                    i += 1;
                    continue;
                }
                let first = i;
                while i < walkable.len() && !walkable[i] {
                    i += 1;
                }
                if first == 0 || i == walkable.len() {
                    continue;
                }
                let left = segment.profile.strip_span(first).0;
                let right = segment.profile.strip_span(i - 1).1;
                let cut = |d0: f32, d1: f32| {
                    let side = |offset: f32| {
                        let line =
                            lane_builder::offset_polyline(&segment.centerline, f64::from(offset));
                        lane_builder::sub_polyline(&line, f64::from(d0), f64::from(d1))
                    };
                    let mut points = side(left);
                    let mut other = side(right);
                    other.reverse();
                    points.extend(other);
                    Polygon::new(points)
                };
                let mut d = from;
                for crossing in &crossings {
                    let (low, high) = (
                        crossing.at - crossing.width * 0.5,
                        crossing.at + crossing.width * 0.5,
                    );
                    if low - d >= MIN_PIECE {
                        self.areas.push(WalkArea {
                            kind: AreaKind::Carriageway {
                                segment: segment.id,
                            },
                            polygon: cut(d, low),
                        });
                    }
                    let signal = if low - from <= SIGNAL_REACH {
                        signal_at(segment.from)
                    } else if to - high <= SIGNAL_REACH {
                        signal_at(segment.to)
                    } else {
                        None
                    };
                    self.areas.push(WalkArea {
                        kind: AreaKind::Crossing {
                            id: crossing.id,
                            segment: segment.id,
                            signal,
                        },
                        polygon: cut(low, high),
                    });
                    d = high;
                }
                if to - d >= MIN_PIECE {
                    self.areas.push(WalkArea {
                        kind: AreaKind::Carriageway {
                            segment: segment.id,
                        },
                        polygon: cut(d, to),
                    });
                }
            }
        }
    }

    fn find_portals(&mut self) {
        for i in 0..self.areas.len() {
            for j in i + 1..self.areas.len() {
                for (a, b) in self.areas[i].polygon.edges() {
                    for (c, d) in self.areas[j].polygon.edges() {
                        if let Some((p, q)) = shared_edge(a, b, c, d) {
                            self.portals.push(Portal {
                                a: p,
                                b: q,
                                areas: (i, j),
                            });
                        }
                    }
                }
            }
        }
    }

    fn find_walls(&mut self) {
        let mut walls = Vec::new();
        for (index, area) in self.areas.iter().enumerate() {
            for (a, b) in area.polygon.edges() {
                let length = a.distance(b);
                if length < 1e-6 {
                    continue;
                }
                let normal = (b - a).perp() / length;
                let pieces = (length / WALL_PIECE).ceil().max(1.0) as usize;
                let mut open_from: Option<(Vec2, Vec2)> = None;
                for k in 0..pieces {
                    let p0 = a.lerp(b, k as f32 / pieces as f32);
                    let p1 = a.lerp(b, (k + 1) as f32 / pieces as f32);
                    let mid = (p0 + p1) * 0.5;
                    let inward = if area.polygon.contains(mid + normal * PROBE) {
                        normal
                    } else {
                        -normal
                    };
                    let outside = mid - inward * PROBE;
                    let is_wall = !self
                        .areas
                        .iter()
                        .enumerate()
                        .any(|(other, o)| other != index && o.polygon.contains(outside));
                    match (&mut open_from, is_wall) {
                        (Some((_, end)), true) => *end = p1,
                        (None, true) => open_from = Some((p0, p1)),
                        (Some(_), false) => {
                            let (s, e) = open_from.take().expect("a wall run is open");
                            walls.push(Wall {
                                a: s,
                                b: e,
                                normal: inward,
                            });
                        }
                        (None, false) => {}
                    }
                    if k + 1 == pieces
                        && let Some((s, e)) = open_from.take()
                    {
                        walls.push(Wall {
                            a: s,
                            b: e,
                            normal: inward,
                        });
                    }
                }
            }
        }
        self.walls.extend(walls);
    }

    fn find_kerbs(&mut self) {
        for portal in &self.portals {
            let (i, j) = portal.areas;
            let (inside, outside) =
                match (self.areas[i].kind.is_road(), self.areas[j].kind.is_road()) {
                    (false, true) => (i, j),
                    (true, false) => (j, i),
                    (true, true) => (i, j),
                    (false, false) => continue,
                };
            let along = (portal.b - portal.a).normalize_or_zero();
            let mid = (portal.a + portal.b) * 0.5;
            let normal = if self.areas[inside]
                .polygon
                .contains(mid + along.perp() * PROBE)
            {
                along.perp()
            } else {
                -along.perp()
            };
            self.kerbs.push(Kerb {
                a: portal.a,
                b: portal.b,
                normal,
                inside,
                outside,
            });
        }
    }

    /// A kerb as a wall for a walker that may enter the road areas `open`: when it may be
    /// on one side only, with the normal toward that side.
    pub fn barrier_for(&self, kerb: &Kerb, open: &[usize]) -> Option<Wall> {
        let allowed = |area: usize| !self.areas[area].kind.is_road() || open.contains(&area);
        let normal = match (allowed(kerb.inside), allowed(kerb.outside)) {
            (true, false) => kerb.normal,
            (false, true) => -kerb.normal,
            _ => return None,
        };
        Some(Wall {
            a: kerb.a,
            b: kerb.b,
            normal,
        })
    }

    /// Inside a foot area, or a road area in `open`, and inside no obstacle: where a
    /// walker that may enter `open` may be.
    pub fn contains_for(&self, point: Vec2, open: &[usize]) -> bool {
        let inside = self.areas.iter().enumerate().any(|(index, area)| {
            (!area.kind.is_road() || open.contains(&index)) && area.polygon.contains(point)
        });
        inside && !self.in_obstacle(point)
    }

    fn in_obstacle(&self, point: Vec2) -> bool {
        self.circles
            .iter()
            .any(|round| round.center.distance(point) < round.radius)
            || self.obstacle_polygons.iter().any(|p| p.contains(point))
    }

    fn add_entries(&mut self, street: &Street, gate: &Gate) {
        let Some(segment) = street.segments.get(&gate.segment) else {
            return;
        };
        let at_start = gate.position.distance(street.nodes[&segment.from].position);
        let at_end = gate.position.distance(street.nodes[&segment.to].position);
        if at_start.min(at_end) <= END_GATE_RADIUS {
            let start = at_start <= at_end;
            for (index, area) in self.areas.iter().enumerate() {
                let AreaKind::Sidewalk { segment: s, strip } = area.kind else {
                    continue;
                };
                if s != segment.id {
                    continue;
                }
                let (left, right) = segment.profile.strip_span(strip);
                let side = |offset: f32| {
                    lane_builder::offset_polyline(&segment.centerline, f64::from(offset))
                };
                let (l, r) = (side(left), side(right));
                let (a, b, inward) = if start {
                    (l[0], r[0], (l[1] - l[0]).normalize_or_zero())
                } else {
                    let n = l.len();
                    (
                        l[n - 1],
                        r[n - 1],
                        (l[n - 2] - l[n - 1]).normalize_or_zero(),
                    )
                };
                self.entries.push(Entry {
                    gate: gate.id.clone(),
                    area: index,
                    a,
                    b,
                    inward,
                });
            }
            return;
        }
        // A door: on the walkable edge nearest to the gate.
        let mut best: Option<(f32, usize, Vec2, Vec2)> = None;
        for (index, area) in self.areas.iter().enumerate() {
            for (a, b) in area.polygon.edges() {
                let q = closest_point_to_segment(gate.position, a, b);
                let d = q.distance(gate.position);
                if d <= DOOR_REACH && best.is_none_or(|(bd, ..)| d < bd) {
                    best = Some((d, index, a, b));
                }
            }
        }
        let Some((_, index, a, b)) = best else {
            return;
        };
        let along = (b - a).normalize_or_zero();
        let q = closest_point_to_segment(gate.position, a, b);
        let ta = (along.dot(q - a) - DOOR_WIDTH * 0.5).max(0.0);
        let tb = (along.dot(q - a) + DOOR_WIDTH * 0.5).min(a.distance(b));
        let normal = along.perp();
        let polygon = &self.areas[index].polygon;
        let inward = if polygon.contains(q + normal * PROBE) {
            normal
        } else {
            -normal
        };
        self.entries.push(Entry {
            gate: gate.id.clone(),
            area: index,
            a: a + along * ta,
            b: a + along * tb,
            inward,
        });
    }

    /// The entries of a gate, as indices into `entries`.
    pub fn entries_of<'a>(&'a self, gate: &'a str) -> impl Iterator<Item = usize> + 'a {
        self.entries
            .iter()
            .enumerate()
            .filter(move |(_, entry)| entry.gate == gate)
            .map(|(index, _)| index)
    }

    /// Inside some area (road areas too) and inside no obstacle: nowhere a walker could
    /// ever be is outside this.
    pub fn contains(&self, point: Vec2) -> bool {
        self.areas.iter().any(|area| area.polygon.contains(point)) && !self.in_obstacle(point)
    }

    /// The distance from `point` to the nearest wall (obstacle outlines included).
    pub fn wall_distance(&self, point: Vec2) -> f32 {
        self.walls
            .iter()
            .map(|wall| closest_point_to_segment(point, wall.a, wall.b).distance(point))
            .fold(f32::INFINITY, f32::min)
    }

    /// Obstacles in one list: the circles first, then the polygons.
    pub fn obstacle_count(&self) -> usize {
        self.circles.len() + self.obstacle_polygons.len()
    }

    /// The extent of obstacle `index` seen from `from` looking along `along` (a unit
    /// vector): (nearest and furthest distance ahead, leftmost and rightmost offset, with
    /// `along.perp()` as right).
    pub fn obstacle_extent(&self, index: usize, from: Vec2, along: Vec2) -> (f32, f32, f32, f32) {
        let side = along.perp();
        if let Some(round) = self.circles.get(index) {
            let (a, l) = (
                along.dot(round.center - from),
                side.dot(round.center - from),
            );
            return (
                a - round.radius,
                a + round.radius,
                l - round.radius,
                l + round.radius,
            );
        }
        let polygon = &self.obstacle_polygons[index - self.circles.len()];
        let mut extent = (
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
        );
        for &p in &polygon.points {
            let (a, l) = (along.dot(p - from), side.dot(p - from));
            extent = (
                extent.0.min(a),
                extent.1.max(a),
                extent.2.min(l),
                extent.3.max(l),
            );
        }
        extent
    }

    /// How deep a body of `radius` at `point` is inside an obstacle (0 when clear).
    pub fn obstacle_depth(&self, point: Vec2, radius: f32) -> f32 {
        let mut depth: f32 = 0.0;
        for round in &self.circles {
            depth = depth.max(round.radius + radius - round.center.distance(point));
        }
        for polygon in &self.obstacle_polygons {
            let edge = polygon.distance_to_edge(point);
            let d = if polygon.contains(point) {
                edge + radius
            } else {
                radius - edge
            };
            depth = depth.max(d);
        }
        depth
    }

    /// The cheapest way from entry `from` to any entry of gate `to`. Without `free`, the
    /// way crosses streets only on crossings.
    pub fn find_path(&self, from: usize, to: &str, free: bool) -> Option<WalkPath> {
        let mut best: Option<(f32, WalkPath)> = None;
        for to_entry in self.entries_of(to) {
            if to_entry == from {
                continue;
            }
            if let Some((cost, path)) = self.path_between(from, to_entry, free)
                && best.as_ref().is_none_or(|(c, _)| cost < *c)
            {
                best = Some((cost, path));
            }
        }
        best.map(|(_, path)| path)
    }

    fn path_between(&self, from: usize, to: usize, free: bool) -> Option<(f32, WalkPath)> {
        let (start, end) = (&self.entries[from], &self.entries[to]);
        let allowed =
            |area: usize| free || !matches!(self.areas[area].kind, AreaKind::Carriageway { .. });
        // Points: 0 the start, 1 the end, then points along the portals.
        let mut graph = AStar::new();
        let mut areas_of: Vec<Vec<usize>> = Vec::new();
        let mut portal_of: Vec<usize> = Vec::new();
        graph.add_point((start.a + start.b) * 0.5);
        areas_of.push(vec![start.area]);
        portal_of.push(usize::MAX);
        graph.add_point((end.a + end.b) * 0.5);
        areas_of.push(vec![end.area]);
        portal_of.push(usize::MAX);
        for (index, portal) in self.portals.iter().enumerate() {
            if !allowed(portal.areas.0) || !allowed(portal.areas.1) {
                continue;
            }
            let pieces = (portal.a.distance(portal.b) / PORTAL_STEP).ceil().max(1.0) as usize;
            for k in 0..pieces {
                let t = (k as f32 + 0.5) / pieces as f32;
                graph.add_point(portal.a.lerp(portal.b, t));
                areas_of.push(vec![portal.areas.0, portal.areas.1]);
                portal_of.push(index);
            }
        }
        // The area an edge between two points goes through (the first they share).
        let shared = |i: usize, j: usize| {
            areas_of[i]
                .iter()
                .copied()
                .find(|a| areas_of[j].contains(a))
        };
        for i in 0..areas_of.len() {
            for j in i + 1..areas_of.len() {
                if shared(i, j).is_some() {
                    graph.connect_points(i, j);
                }
            }
        }
        let ids = graph.id_path(
            0,
            1,
            |positions, a, b| {
                let distance = positions[a].distance(positions[b]);
                let road = shared(a, b).is_some_and(|area| {
                    matches!(self.areas[area].kind, AreaKind::Carriageway { .. })
                });
                if road { distance * ROAD_COST } else { distance }
            },
            |a, b| a.distance(b),
        );
        if ids.is_empty() {
            return None;
        }
        let mut cost = 0.0;
        let mut length = 0.0;
        let mut areas = vec![start.area];
        let mut crossed: Vec<usize> = Vec::new();
        for (k, pair) in ids.windows(2).enumerate() {
            let here = *areas.last().expect("a corridor starts somewhere");
            let common: Vec<usize> = areas_of[pair[0]]
                .iter()
                .copied()
                .filter(|a| areas_of[pair[1]].contains(a))
                .collect();
            let area = if common.contains(&here) {
                here
            } else {
                common[0]
            };
            if area != here {
                // The point between the previous edge and this one is the portal crossed.
                let portal = portal_of[ids[k]];
                if portal == usize::MAX {
                    return None;
                }
                let (p, q) = self.portals[portal].areas;
                if !((p == here && q == area) || (q == here && p == area)) {
                    // The chain of portals does not join up: no corridor.
                    return None;
                }
                crossed.push(portal);
                areas.push(area);
            }
            let distance = graph.position(pair[0]).distance(graph.position(pair[1]));
            length += distance;
            cost += if matches!(self.areas[area].kind, AreaKind::Carriageway { .. }) {
                distance * ROAD_COST
            } else {
                distance
            };
        }
        if *areas.last().expect("not empty") != end.area {
            return None;
        }
        let mut portals = Vec::new();
        let mut kerbs = Vec::new();
        for (k, &portal) in crossed.iter().enumerate() {
            let (here, next) = (areas[k], areas[k + 1]);
            let portal = &self.portals[portal];
            // Left and right as seen walking from `here` into `next`.
            let across = self.areas[next].polygon.centroid() - self.areas[here].polygon.centroid();
            let (a, b) = shorten(portal.a, portal.b, CORNER_CLEARANCE);
            let (left, right) = if across.perp_dot(b - a) < 0.0 {
                (a, b)
            } else {
                (b, a)
            };
            portals.push((left, right));
            kerbs.push(!self.areas[here].kind.is_road() && self.areas[next].kind.is_road());
        }
        let off_crossing = areas
            .iter()
            .any(|a| matches!(self.areas[*a].kind, AreaKind::Carriageway { .. }));
        let (a, b) = shorten(end.a, end.b, CORNER_CLEARANCE);
        let goal = (
            a + end.inward * CORNER_CLEARANCE,
            b + end.inward * CORNER_CLEARANCE,
        );
        let cuts = self.cut_ways(&areas, &crossed, (goal.0 + goal.1) * 0.5);
        Some((
            cost,
            WalkPath {
                areas,
                portals,
                kerbs,
                off_crossing,
                length,
                to_entry: to,
                goal,
                cuts,
            },
        ))
    }

    /// The zebras of a corridor (`crossed[k]` leads from `areas[k]` to `areas[k + 1]`)
    /// that may be cut at a corner: foot area, zebra, foot area, with a carriageway piece
    /// beside the zebra that touches both kerbs. The piece on the goal's side is used.
    fn cut_ways(&self, areas: &[usize], crossed: &[usize], goal: Vec2) -> Vec<CutWay> {
        let portal_between = |p: usize, q: usize| {
            self.portals
                .iter()
                .find(|x| x.areas == (p, q) || x.areas == (q, p))
        };
        let mut cuts = Vec::new();
        for k in 0..crossed.len().saturating_sub(1) {
            let (foot, zebra, land) = (areas[k], areas[k + 1], areas[k + 2]);
            if !matches!(self.areas[zebra].kind, AreaKind::Crossing { .. })
                || self.areas[foot].kind.is_road()
                || self.areas[land].kind.is_road()
            {
                continue;
            }
            let mut best: Option<(f32, usize)> = None;
            for piece in 0..self.areas.len() {
                if !matches!(self.areas[piece].kind, AreaKind::Carriageway { .. }) {
                    continue;
                }
                let (Some(beside), Some(_), Some(_)) = (
                    portal_between(zebra, piece),
                    portal_between(foot, piece),
                    portal_between(land, piece),
                ) else {
                    continue;
                };
                let d = ((beside.a + beside.b) * 0.5).distance(goal);
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, piece));
                }
            }
            let Some((_, piece)) = best else {
                continue;
            };
            let across = |from: usize, to: usize| {
                self.areas[to].polygon.centroid() - self.areas[from].polygon.centroid()
            };
            let near = widened(
                &self.portals[crossed[k]],
                portal_between(foot, piece).expect("checked"),
                across(foot, zebra),
            );
            let far = widened(
                &self.portals[crossed[k + 1]],
                portal_between(land, piece).expect("checked"),
                across(zebra, land),
            );
            cuts.push(CutWay {
                kerb: k,
                piece,
                near,
                far,
            });
        }
        cuts
    }

    /// The road areas a walker on `path`, in area `index` of its corridor, may be in:
    /// the ones it is on or has just left, and the next one once it has decided to cross
    /// (`cleared`: the kerb in front of it is passed).
    pub fn open_roads(&self, path: &WalkPath, index: usize, cleared: bool) -> Vec<usize> {
        let mut open = Vec::new();
        let on_road = |i: usize| self.areas[path.areas[i]].kind.is_road();
        if index > 0 && on_road(index - 1) {
            open.push(path.areas[index - 1]);
        }
        if on_road(index) {
            open.push(path.areas[index]);
        }
        if index + 1 < path.areas.len() && on_road(index + 1) && (cleared || on_road(index)) {
            open.push(path.areas[index + 1]);
        }
        open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An L corridor: east along y in [0, 2], then south through x in [8, 10].
    fn corner_portals() -> Vec<(Vec2, Vec2)> {
        // Walking east into the corner square, then south out of it.
        let east = (Vec2::new(8.0, 0.0), Vec2::new(8.0, 2.0));
        let south = (Vec2::new(10.0, 2.0), Vec2::new(8.0, 2.0));
        let orient = |(a, b): (Vec2, Vec2), across: Vec2| {
            if across.perp_dot(b - a) < 0.0 {
                (a, b)
            } else {
                (b, a)
            }
        };
        vec![orient(east, Vec2::X), orient(south, Vec2::Y)]
    }

    #[test]
    fn the_funnel_cuts_the_inner_corner_of_an_l() {
        let path = string_pull(Vec2::new(1.0, 1.0), &corner_portals(), Vec2::new(9.0, 10.0));
        assert_eq!(path.len(), 3, "{path:?}");
        assert_eq!(path[1], Vec2::new(8.0, 2.0), "the inner corner: {path:?}");
    }

    #[test]
    fn a_straight_corridor_has_no_corner() {
        let portals = corner_portals();
        let path = string_pull(Vec2::new(1.0, 1.0), &portals[..1], Vec2::new(9.0, 1.5));
        assert_eq!(path, vec![Vec2::new(1.0, 1.0), Vec2::new(9.0, 1.5)]);
    }

    #[test]
    fn shared_edges_are_found_and_corners_are_not() {
        let a = (Vec2::new(0.0, 2.0), Vec2::new(10.0, 2.0));
        let b = (Vec2::new(12.0, 2.0), Vec2::new(7.0, 2.0));
        assert_eq!(
            shared_edge(a.0, a.1, b.0, b.1),
            Some((Vec2::new(7.0, 2.0), Vec2::new(10.0, 2.0)))
        );
        let touching = (Vec2::new(10.0, 2.0), Vec2::new(14.0, 2.0));
        assert_eq!(shared_edge(a.0, a.1, touching.0, touching.1), None);
    }
}
