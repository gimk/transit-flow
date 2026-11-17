//! Picking in play mode: from a point of the street (metres, y down) to what is under it,
//! and from a click to the `DesignCommand` a tool makes. Pure functions on a `Street`, so
//! they are tested without a window.
//!
//! A segment is found by projecting the point on its centerline: `along` is the distance
//! from the segment's `from` node (the unit `Crossing::at` uses) and `offset` the signed
//! distance to the right of the centerline (the unit of `Profile::strip_span`).

use bevy::prelude::Vec2;
use tf_sim::street::level::{CrossingDef, CrossingKind, DEFAULT_CROSSING_WIDTH};
use tf_sim::street::model::SPEED_LIMITS_KMH;
use tf_sim::street::{DesignCommand, Street};

use crate::views::street::carriageway;

/// A crossing's centre is placed on this grid, metres.
pub const CROSSING_SNAP_M: f32 = 0.5;

/// Where a point lies on a segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentHit {
    pub segment: i32,
    /// Metres along the centerline from the `from` node.
    pub along: f32,
    /// Metres to the right of the centerline (negative: left).
    pub offset: f32,
}

/// Projects `p` on a polyline: the distance along it, the signed distance to its right,
/// and the distance to the closest point. `None` if `p` lies beyond either end (the
/// closest point is an end and `p` is past it).
fn project(line: &[Vec2], p: Vec2) -> Option<(f32, f32, f32)> {
    let mut best: Option<(f32, f32, f32)> = None;
    let mut walked = 0.0;
    let last = line.len().saturating_sub(2);
    for (i, pair) in line.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let length = a.distance(b);
        if length < 1e-6 {
            continue;
        }
        let direction = (b - a) / length;
        let t = (p - a).dot(direction);
        // Past the start of the first piece or the end of the last one: not on the segment.
        let beyond = (i == 0 && t < 0.0) || (i == last && t > length);
        if !beyond {
            let t = t.clamp(0.0, length);
            let closest = a + direction * t;
            let right = Vec2::new(-direction.y, direction.x);
            let distance = p.distance(closest);
            if best.is_none_or(|(_, _, d)| distance < d) {
                best = Some((walked + t, (p - closest).dot(right), distance));
            }
        }
        walked += length;
    }
    best
}

/// The segment whose road surface (its whole profile, from building line to building
/// line, between the junction edges) holds `p`.
pub fn segment_at(street: &Street, p: Vec2) -> Option<SegmentHit> {
    street.segments.values().find_map(|segment| {
        let (along, offset, _) = project(&segment.centerline, p)?;
        let length = tf_sim::roads::lane_builder::polyline_length(&segment.centerline) as f32;
        let half = segment.profile.total_width() * 0.5;
        let inside = along >= segment.start_trim
            && along <= length - segment.end_trim
            && offset.abs() <= half;
        inside.then_some(SegmentHit {
            segment: segment.id,
            along,
            offset,
        })
    })
}

/// A hit on a carriageway (the strips between the sidewalks), where a crossing goes.
pub fn carriageway_at(street: &Street, p: Vec2) -> Option<SegmentHit> {
    let hit = segment_at(street, p)?;
    let (left, right) = carriageway(&street.segments[&hit.segment].profile)?;
    (hit.offset >= left && hit.offset <= right).then_some(hit)
}

/// The id of the crossing under `p`, if any.
pub fn crossing_at(street: &Street, p: Vec2) -> Option<i32> {
    let hit = carriageway_at(street, p)?;
    street
        .crossings
        .values()
        .find(|crossing| {
            crossing.segment == hit.segment
                && (hit.along - crossing.at).abs() <= crossing.width * 0.5
        })
        .map(|crossing| crossing.id)
}

/// The zebra tool's click: a crossing under the point is removed, else a new zebra goes
/// on the carriageway at the point. `None` when the click hit neither.
pub fn zebra_command(street: &Street, p: Vec2) -> Option<DesignCommand> {
    if let Some(id) = crossing_at(street, p) {
        return Some(DesignCommand::RemoveCrossing { id });
    }
    let hit = carriageway_at(street, p)?;
    Some(DesignCommand::AddCrossing {
        crossing: CrossingDef {
            kind: CrossingKind::Zebra,
            segment: hit.segment,
            at: (hit.along / CROSSING_SNAP_M).round() * CROSSING_SNAP_M,
            width: DEFAULT_CROSSING_WIDTH,
        },
    })
}

/// The limit after `kmh` in `SPEED_LIMITS_KMH`, wrapping; the first one for a value that
/// is not in the list.
pub fn next_speed_limit(kmh: f32) -> f32 {
    let next = SPEED_LIMITS_KMH
        .iter()
        .position(|limit| (*limit - kmh).abs() < 0.01)
        .map_or(0, |i| (i + 1) % SPEED_LIMITS_KMH.len());
    SPEED_LIMITS_KMH[next]
}

/// The speed tool's click: the segment under the point gets its next limit.
pub fn speed_command(street: &Street, p: Vec2) -> Option<DesignCommand> {
    let hit = segment_at(street, p)?;
    let kmh = street.segments[&hit.segment].speed_limit_kmh;
    Some(DesignCommand::SetSpeedLimit {
        segment: hit.segment,
        kmh: next_speed_limit(kmh),
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::level::LoadedLevel;
    use tf_sim::roads::lane_builder::polyline_length;

    /// The point `along` metres down `line`, `offset` metres to its right.
    pub fn point_on(line: &[Vec2], along: f32, offset: f32) -> Vec2 {
        let mut walked = 0.0;
        for pair in line.windows(2) {
            let length = pair[0].distance(pair[1]);
            if along <= walked + length || std::ptr::eq(&pair[1], line.last().expect("a line")) {
                let direction = (pair[1] - pair[0]) / length;
                let right = Vec2::new(-direction.y, direction.x);
                return pair[0] + direction * (along - walked) + right * offset;
            }
            walked += length;
        }
        unreachable!("a line has pieces")
    }

    /// A free spot of the first segment where a zebra can go, and the segment's id.
    pub fn free_spot(street: &Street) -> (i32, f32, Vec2) {
        for segment in street.segments.values() {
            let length = polyline_length(&segment.centerline) as f32;
            let mut along = segment.start_trim + 4.0;
            while along < length - segment.end_trim - 4.0 {
                let p = point_on(&segment.centerline, along, 0.0);
                let mut trial = street.clone();
                if let Some(command) = zebra_command(&trial, p)
                    && matches!(command, DesignCommand::AddCrossing { .. })
                    && trial.apply(&command).is_ok()
                {
                    return (segment.id, along, p);
                }
                along += 3.0;
            }
        }
        panic!("no free spot for a zebra");
    }

    #[test]
    fn a_point_on_a_carriageway_gives_a_zebra_snapped_to_half_a_metre() {
        let street = LoadedLevel::load("carrefour-ecole").expect("loads");
        let (segment, along, p) = free_spot(&street);
        let Some(DesignCommand::AddCrossing { crossing }) = zebra_command(&street, p) else {
            panic!("a zebra")
        };
        assert_eq!(crossing.segment, segment);
        assert!((crossing.at - along).abs() <= CROSSING_SNAP_M * 0.5 + 1e-3);
        assert_eq!((crossing.at * 2.0).fract(), 0.0, "on the 0.5 m grid");
        assert_eq!(crossing.width, DEFAULT_CROSSING_WIDTH);
    }

    #[test]
    fn a_click_on_a_crossing_removes_it_and_off_the_road_does_nothing() {
        let mut street = LoadedLevel::load("carrefour-ecole").expect("loads");
        let (_, _, p) = free_spot(&street);
        let add = zebra_command(&street, p).expect("a zebra");
        street.apply(&add).expect("placed");
        let id = crossing_at(&street, p).expect("the new crossing is under the point");
        assert_eq!(
            zebra_command(&street, p),
            Some(DesignCommand::RemoveCrossing { id })
        );
        // Far outside the level: nothing under it.
        assert_eq!(segment_at(&street, Vec2::new(-500.0, -500.0)), None);
        assert_eq!(zebra_command(&street, Vec2::new(-500.0, -500.0)), None);
    }

    #[test]
    fn a_sidewalk_is_a_segment_but_not_a_carriageway() {
        let street = LoadedLevel::load("carrefour-ecole").expect("loads");
        let (segment, along, _) = free_spot(&street);
        let segment = &street.segments[&segment];
        let half = segment.profile.total_width() * 0.5;
        let p = point_on(&segment.centerline, along, half - 0.2);
        assert!(
            segment_at(&street, p).is_some(),
            "the outer strip is part of the segment"
        );
        assert_eq!(
            carriageway_at(&street, p),
            None,
            "but no crossing goes there"
        );
        assert!(matches!(
            speed_command(&street, p),
            Some(DesignCommand::SetSpeedLimit { .. })
        ));
    }

    #[test]
    fn the_speed_limit_cycles_through_the_allowed_values() {
        assert_eq!(next_speed_limit(20.0), 30.0);
        assert_eq!(next_speed_limit(30.0), 50.0);
        assert_eq!(next_speed_limit(50.0), 20.0);
        assert_eq!(
            next_speed_limit(37.0),
            20.0,
            "an unknown value starts the cycle"
        );
    }
}
