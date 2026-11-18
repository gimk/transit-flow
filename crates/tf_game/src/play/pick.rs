//! Picking in play mode: from a point of the street (metres, y down) to what is under it,
//! and from a click to the `DesignCommand` a tool makes. Pure functions on a `Street`, so
//! they are tested without a window.
//!
//! A segment is found by projecting the point on its centerline: `along` is the distance
//! from the segment's `from` node (the unit `Crossing::at` uses) and `offset` the signed
//! distance to the right of the centerline (the unit of `Profile::strip_span`).

use bevy::prelude::Vec2;
use tf_sim::street::design::{Side, SignalTemplate};
use tf_sim::street::level::{CrossingDef, CrossingKind, DEFAULT_CROSSING_WIDTH, JunctionControl};
use tf_sim::street::model::{Junction, SPEED_LIMITS_KMH};
use tf_sim::street::signals::{
    DEFAULT_BICYCLE_LEAD_S, DEFAULT_GREEN_S, MAX_BICYCLE_LEAD_S, MAX_GREEN_S, MIN_GREEN_S,
};
use tf_sim::street::space::Separator;
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
        let length = tf_sim::geom::lanes::polyline_length(&segment.centerline) as f32;
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

/// The point `along` metres down `line` (clamped to its ends), `offset` metres to its right.
pub fn point_at(line: &[Vec2], along: f32, offset: f32) -> Vec2 {
    let mut walked = 0.0;
    let mut last = (line[0], Vec2::X);
    for pair in line.windows(2) {
        let length = pair[0].distance(pair[1]);
        if length < 1e-6 {
            continue;
        }
        let direction = (pair[1] - pair[0]) / length;
        last = (pair[1], direction);
        if along <= walked + length {
            let right = Vec2::new(-direction.y, direction.x);
            return pair[0] + direction * (along - walked) + right * offset;
        }
        walked += length;
    }
    let (end, direction) = last;
    end + Vec2::new(-direction.y, direction.x) * offset
}

/// Whether `p` is inside the polygon (even-odd rule).
pub fn polygon_contains(points: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// The node of the junction whose area holds `p`.
pub fn junction_at(street: &Street, p: Vec2) -> Option<i32> {
    street
        .junctions
        .values()
        .find(|junction| polygon_contains(&junction.polygon.points, p))
        .map(|junction| junction.node)
}

/// A drag shorter than this is a click, metres.
pub const MIN_DRAG_M: f32 = 1.0;
/// How near a row's kerb line a click must be to pick the row, metres.
pub const BOLLARD_HIT_M: f32 = 0.8;
/// The shortest row of bollards the tool places, metres.
pub const MIN_ROW_M: f32 = 1.0;

/// The id of the bollard row under `p`, if any.
pub fn bollard_row_at(street: &Street, p: Vec2) -> Option<i32> {
    let hit = segment_at(street, p)?;
    let (left, right) = carriageway(&street.segments[&hit.segment].profile)?;
    street
        .bollard_rows
        .values()
        .find(|row| {
            let kerb = match row.side {
                Side::Left => left,
                Side::Right => right,
            };
            row.segment == hit.segment
                && hit.along >= row.from - 0.5
                && hit.along <= row.to + 0.5
                && (hit.offset - kerb).abs() <= BOLLARD_HIT_M
        })
        .map(|row| row.id)
}

/// The bollards tool's drag from `a` to `b` along a sidewalk edge: a row on the side
/// whose kerb is nearer the pointer, from and to snapped to 0.5 m and held inside the
/// segment's free length. `None` if the drag is not on one segment or is too short.
pub fn bollards_command(street: &Street, a: Vec2, b: Vec2) -> Option<DesignCommand> {
    let (first, second) = (segment_at(street, a)?, segment_at(street, b)?);
    if first.segment != second.segment {
        return None;
    }
    let segment = &street.segments[&first.segment];
    let (left, right) = carriageway(&segment.profile)?;
    let middle = (first.offset + second.offset) * 0.5;
    let side = if (middle - left).abs() <= (middle - right).abs() {
        Side::Left
    } else {
        Side::Right
    };
    let length = tf_sim::geom::lanes::polyline_length(&segment.centerline) as f32;
    let snap = |v: f32| (v / CROSSING_SNAP_M).round() * CROSSING_SNAP_M;
    let lowest = (segment.start_trim / CROSSING_SNAP_M).ceil() * CROSSING_SNAP_M;
    let highest = ((length - segment.end_trim) / CROSSING_SNAP_M).floor() * CROSSING_SNAP_M;
    let from = snap(first.along.min(second.along)).max(lowest);
    let to = snap(first.along.max(second.along)).min(highest);
    (to - from >= MIN_ROW_M).then_some(DesignCommand::AddBollards {
        segment: first.segment,
        side,
        from,
        to,
    })
}

/// The bollards tool when the button comes up: a short drag is a click (a row under it
/// goes), a long one places a row.
pub fn bollards_release(street: &Street, start: Vec2, end: Vec2) -> Option<DesignCommand> {
    if start.distance(end) < MIN_DRAG_M {
        bollard_row_at(street, start).map(|id| DesignCommand::RemoveBollards { id })
    } else {
        bollards_command(street, start, end)
    }
}

/// The green time (and walk time) of a junction's plan, read back from the plan the
/// design made: a last phase with nobody on green is the walk phase.
pub fn template_of(junction: &Junction) -> Option<SignalTemplate> {
    let plan = junction.signal_plan.as_ref()?;
    let green_s = plan.phases.first()?.green_s;
    match plan.phases.last() {
        Some(last) if plan.phases.len() > 1 && last.green.is_empty() => {
            Some(SignalTemplate::TwoPhaseWithWalk {
                green_s,
                walk_s: last.green_s,
            })
        }
        _ => Some(SignalTemplate::TwoPhase { green_s }),
    }
}

/// Walk time a junction gets when the tool first adds a walk phase, seconds.
pub const DEFAULT_WALK_S: f32 = 10.0;
/// One step of the wheel or +/- on the green time, seconds.
pub const GREEN_STEP_S: f32 = 5.0;

/// The signals tool's click: priority to the right, then two phases, then two phases with
/// a walk phase, then back.
pub fn signals_click_command(street: &Street, p: Vec2) -> Option<DesignCommand> {
    let node = junction_at(street, p)?;
    let junction = &street.junctions[&node];
    let (control, template) = match (junction.control, template_of(junction)) {
        (JunctionControl::PriorityToRight, _) => (
            JunctionControl::Signals,
            Some(SignalTemplate::TwoPhase {
                green_s: DEFAULT_GREEN_S,
            }),
        ),
        (JunctionControl::Signals, Some(SignalTemplate::TwoPhaseWithWalk { .. })) => {
            (JunctionControl::PriorityToRight, None)
        }
        (JunctionControl::Signals, Some(SignalTemplate::TwoPhase { green_s })) => (
            JunctionControl::Signals,
            Some(SignalTemplate::TwoPhaseWithWalk {
                green_s,
                walk_s: DEFAULT_WALK_S,
            }),
        ),
        // The level's own signals without a stored plan run the default plan.
        (JunctionControl::Signals, None) => (
            JunctionControl::Signals,
            Some(SignalTemplate::TwoPhaseWithWalk {
                green_s: DEFAULT_GREEN_S,
                walk_s: DEFAULT_WALK_S,
            }),
        ),
    };
    Some(DesignCommand::SetJunctionControl {
        node,
        control,
        template,
    })
}

/// The node of the junction under `p` if it is under signals.
pub fn signalled_junction_at(street: &Street, p: Vec2) -> Option<i32> {
    let node = junction_at(street, p)?;
    (street.junctions[&node].control == JunctionControl::Signals).then_some(node)
}

/// Wheel or +/- over a signalled junction: its green time moves `steps` times 5 s, held to
/// 10..60 s. `None` if nothing changes.
pub fn green_command(street: &Street, p: Vec2, steps: i32) -> Option<DesignCommand> {
    let node = signalled_junction_at(street, p)?;
    let template = template_of(&street.junctions[&node]).unwrap_or(SignalTemplate::TwoPhase {
        green_s: DEFAULT_GREEN_S,
    });
    let step = |green: f32| (green + steps as f32 * GREEN_STEP_S).clamp(MIN_GREEN_S, MAX_GREEN_S);
    let new = match template {
        SignalTemplate::TwoPhase { green_s } => SignalTemplate::TwoPhase {
            green_s: step(green_s),
        },
        SignalTemplate::TwoPhaseWithWalk { green_s, walk_s } => SignalTemplate::TwoPhaseWithWalk {
            green_s: step(green_s),
            walk_s,
        },
    };
    (new != template).then_some(DesignCommand::SetJunctionControl {
        node,
        control: JunctionControl::Signals,
        template: Some(new),
    })
}

/// How near a boundary line a click must be to pick it, metres.
pub const BOUNDARY_HIT_M: f32 = 0.5;

/// A boundary between two strips of a segment, found under a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundaryHit {
    pub segment: i32,
    /// The boundary between strips `boundary - 1` and `boundary`.
    pub boundary: usize,
    /// Its offset to the right of the centerline, metres.
    pub offset: f32,
    /// Metres along the centerline.
    pub along: f32,
}

/// The strip boundary (not a building line) nearest to `p`, within `BOUNDARY_HIT_M`.
pub fn boundary_at(street: &Street, p: Vec2) -> Option<BoundaryHit> {
    let hit = segment_at(street, p)?;
    let profile = &street.segments[&hit.segment].profile;
    (1..profile.strips.len())
        .map(|boundary| (boundary, profile.strip_span(boundary).0))
        .filter(|(_, offset)| (hit.offset - offset).abs() <= BOUNDARY_HIT_M)
        .min_by(|a, b| {
            (hit.offset - a.1)
                .abs()
                .total_cmp(&(hit.offset - b.1).abs())
        })
        .map(|(boundary, offset)| BoundaryHit {
            segment: hit.segment,
            boundary,
            offset,
            along: hit.along,
        })
}

/// The separator after `separator` when a click cycles: none, paint, kerb, bollards,
/// planter, none.
pub fn next_separator(separator: Separator) -> Separator {
    match separator {
        Separator::None => Separator::Paint,
        Separator::Paint => Separator::Kerb,
        Separator::Kerb => Separator::Bollards,
        Separator::Bollards => Separator::Planter,
        Separator::Planter => Separator::None,
    }
}

/// The separator on a boundary now (the right edge of the strip before it).
pub fn separator_on(street: &Street, hit: &BoundaryHit) -> Separator {
    street.segments[&hit.segment].profile.strips[hit.boundary - 1].right
}

/// The separator tool's click: the boundary under the point gets the next separator.
pub fn separator_command(street: &Street, p: Vec2) -> Option<DesignCommand> {
    let hit = boundary_at(street, p)?;
    Some(DesignCommand::SetSeparator {
        segment: hit.segment,
        boundary: hit.boundary,
        separator: next_separator(separator_on(street, &hit)),
    })
}

/// Depth of a bicycle box the tool places, metres (inside 3 to 5 m).
pub const DEFAULT_BOX_DEPTH_M: f32 = 4.0;

/// The approach a point belongs to: the segment under it and its end node that is nearer.
/// Inside a junction area: the nearest approach of that junction.
pub fn approach_at(street: &Street, p: Vec2) -> Option<(i32, i32)> {
    if let Some(hit) = segment_at(street, p) {
        let segment = &street.segments[&hit.segment];
        let length = tf_sim::geom::lanes::polyline_length(&segment.centerline) as f32;
        let node = if hit.along * 2.0 < length {
            segment.from
        } else {
            segment.to
        };
        return Some((node, hit.segment));
    }
    let node = junction_at(street, p)?;
    street.nodes[&node]
        .segment_ids
        .iter()
        .copied()
        .min_by(|a, b| {
            let d = |id: &i32| {
                let segment = &street.segments[id];
                let end = if segment.to == node {
                    segment.centerline.last()
                } else {
                    segment.centerline.first()
                };
                end.map_or(f32::MAX, |end| end.distance(p))
            };
            d(a).total_cmp(&d(b))
        })
        .map(|segment| (node, segment))
}

/// The bicycle box tool's click: a box on the approach is removed, else one is added.
pub fn box_command(street: &Street, p: Vec2) -> Option<DesignCommand> {
    let (node, segment) = approach_at(street, p)?;
    Some(if street.bicycle_box(node, segment).is_some() {
        DesignCommand::RemoveBicycleBox { node, segment }
    } else {
        DesignCommand::AddBicycleBox {
            node,
            segment,
            depth: DEFAULT_BOX_DEPTH_M,
        }
    })
}

/// One step of the wheel or +/- on the leading bicycle green, seconds.
pub const LEAD_STEP_S: f32 = 1.0;

/// The leading bicycle green of a junction's plan now, seconds (0 without a plan).
pub fn lead_of(junction: &Junction) -> f32 {
    junction
        .signal_plan
        .as_ref()
        .map_or(0.0, |plan| plan.bicycle_lead_s)
}

/// The lead tool's click on a junction: no lead becomes `DEFAULT_BICYCLE_LEAD_S`, any
/// lead becomes none. A junction without signals is refused by the street, with its reason.
pub fn lead_click_command(street: &Street, p: Vec2) -> Option<DesignCommand> {
    let node = junction_at(street, p)?;
    let lead_s = if lead_of(&street.junctions[&node]) > 0.0 {
        0.0
    } else {
        DEFAULT_BICYCLE_LEAD_S
    };
    Some(DesignCommand::SetBicycleLead { node, lead_s })
}

/// Wheel or +/- over a signalled junction with the lead tool: the lead moves `steps`
/// seconds, held to 0..6 s. `None` if nothing changes.
pub fn lead_step_command(street: &Street, p: Vec2, steps: i32) -> Option<DesignCommand> {
    let node = signalled_junction_at(street, p)?;
    let now = lead_of(&street.junctions[&node]);
    let lead_s = (now + steps as f32 * LEAD_STEP_S).clamp(0.0, MAX_BICYCLE_LEAD_S);
    ((lead_s - now).abs() > 1e-3).then_some(DesignCommand::SetBicycleLead { node, lead_s })
}

/// Where the signal mark of one approach of a signalled junction goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ApproachMark {
    pub node: i32,
    /// The approach (a segment id), as `ApproachLight::segment`.
    pub segment: i32,
    /// Street metres.
    pub position: Vec2,
    /// The leading bicycle green of the junction's plan, seconds (0: none). The mark shows
    /// a second, small light for the bicycles when it is above 0.
    pub bicycle_lead_s: f32,
}

/// A mark per approach of every junction under signals: just before the junction edge, on
/// the kerb of the side the approaching traffic keeps to.
pub fn approach_marks(street: &Street) -> Vec<ApproachMark> {
    let mut marks = Vec::new();
    for junction in street.junctions.values() {
        if junction.control != JunctionControl::Signals {
            continue;
        }
        let Some(node) = street.nodes.get(&junction.node) else {
            continue;
        };
        for id in &node.segment_ids {
            let segment = &street.segments[id];
            let Some((left, right)) = carriageway(&segment.profile) else {
                continue;
            };
            let length = tf_sim::geom::lanes::polyline_length(&segment.centerline) as f32;
            // Traffic that arrives at the `to` end keeps right; at the `from` end, left.
            let (along, offset) = if segment.to == junction.node {
                (length - segment.end_trim - 1.0, right + 0.5)
            } else {
                (segment.start_trim + 1.0, left - 0.5)
            };
            marks.push(ApproachMark {
                node: junction.node,
                segment: *id,
                position: point_at(&segment.centerline, along, offset),
                bicycle_lead_s: lead_of(junction),
            });
        }
    }
    marks
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::level::LoadedLevel;
    use tf_sim::geom::lanes::polyline_length;

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
