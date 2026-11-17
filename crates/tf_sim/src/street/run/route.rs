//! The path of a trip: the lanes and movements from one gate to another, as one line
//! with a distance along it. Routes are fixed per origin, destination and class for a
//! run (pivot plan §2.2), so they are found once and shared.

use glam::Vec2;
use indexmap::IndexMap;

use crate::street::ids::StreetLaneId;
use crate::street::level::Gate;
use crate::street::model::Street;
use crate::street::space::Access;

#[derive(Clone, Debug, PartialEq)]
pub struct RoutePath {
    pub lanes: Vec<StreetLaneId>,
    /// The distance along the route at which each lane starts.
    pub offsets: Vec<f64>,
    pub length: f64,
    start: Vec2,
    start_dir: Vec2,
    end: Vec2,
    end_dir: Vec2,
}

impl RoutePath {
    pub fn new(street: &Street, lanes: Vec<StreetLaneId>) -> Self {
        let mut offsets = Vec::with_capacity(lanes.len());
        let mut length = 0.0;
        for id in &lanes {
            offsets.push(length);
            length += street.lanes[id].length;
        }
        let first = street.lanes[&lanes[0]].polyline();
        let last = street.lanes[lanes.last().expect("a route has lanes")].polyline();
        let n = last.len();
        Self {
            lanes,
            offsets,
            length,
            start: first[0],
            start_dir: (first[1] - first[0]).normalize_or_zero(),
            end: last[n - 1],
            end_dir: (last[n - 1] - last[n - 2]).normalize_or_zero(),
        }
    }

    /// The index of the lane that holds distance `d` (clamped to the route).
    pub fn lane_index(&self, d: f64) -> usize {
        self.offsets
            .iter()
            .rposition(|offset| *offset <= d)
            .unwrap_or(0)
    }

    /// Where lane `index` ends along the route.
    pub fn lane_end(&self, index: usize) -> f64 {
        self.offsets.get(index + 1).copied().unwrap_or(self.length)
    }

    /// The point at distance `d`. Before the start and after the end the route goes on
    /// straight, so a body can enter and leave the level.
    pub fn point(&self, street: &Street, d: f64) -> Vec2 {
        if d <= 0.0 {
            return self.start + self.start_dir * d as f32;
        }
        if d >= self.length {
            return self.end + self.end_dir * (d - self.length) as f32;
        }
        let index = self.lane_index(d);
        street.lanes[&self.lanes[index]]
            .curve
            .sample_baked((d - self.offsets[index]) as f32)
    }
}

/// The node at the end of the gate's segment where the gate stands.
pub fn gate_node(street: &Street, gate: &Gate) -> i32 {
    let segment = &street.segments[&gate.segment];
    let from = street.nodes[&segment.from].position;
    let to = street.nodes[&segment.to].position;
    if gate.position.distance_squared(from) <= gate.position.distance_squared(to) {
        segment.from
    } else {
        segment.to
    }
}

/// The shortest chain of lanes open to `class` from gate `from` to gate `to`, or `None`
/// when the design leaves no way.
pub fn find_route(
    street: &Street,
    from: &Gate,
    to: &Gate,
    class: Access,
) -> Option<Vec<StreetLaneId>> {
    let from_node = gate_node(street, from);
    let to_node = gate_node(street, to);
    // lane -> (distance to its end, the lane before it).
    let mut best: IndexMap<StreetLaneId, (f64, Option<StreetLaneId>)> = IndexMap::new();
    let mut done: Vec<StreetLaneId> = Vec::new();
    for lane in street.segment_lanes(from.segment) {
        if lane.from_node == from_node && lane.access.contains(class) {
            best.insert(lane.id, (lane.length, None));
        }
    }
    loop {
        // The closest lane not yet settled; the first found wins a tie.
        let mut next: Option<(StreetLaneId, f64)> = None;
        for (id, (distance, _)) in &best {
            if !done.contains(id) && next.is_none_or(|(_, d)| *distance < d) {
                next = Some((*id, *distance));
            }
        }
        let (id, distance) = next?;
        done.push(id);
        let lane = &street.lanes[&id];
        if lane.segment_id == Some(to.segment) && lane.to_node == to_node {
            let mut lanes = vec![id];
            let mut at = id;
            while let Some(previous) = best[&at].1 {
                lanes.push(previous);
                at = previous;
            }
            lanes.reverse();
            return Some(lanes);
        }
        for out_id in &lane.outgoing {
            let out = &street.lanes[out_id];
            if !out.access.contains(class) {
                continue;
            }
            let through = distance + out.length;
            if best.get(out_id).is_none_or(|(d, _)| through < *d) {
                best.insert(*out_id, (through, Some(id)));
            }
        }
    }
}
