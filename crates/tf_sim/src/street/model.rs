//! `Street`: the space model of one level with the player's design applied (pivot plan
//! §3). It is built from a `LevelFile`, then changed only through `apply` with a
//! `DesignCommand`, which reports a `StreetChange` (the nodes and segments touched) and
//! rebuilds the derived data: the lanes of the touched segments and the movements of
//! their junctions. Lanes are never saved; level + design rebuild them.

use glam::Vec2;
use indexmap::IndexMap;

use super::design::{Design, DesignCommand, Side, SignalTemplate};
use super::ids::StreetLaneId;
use super::lanes::{self, LaneGroup, StreetLane, Turn};
use super::level::{
    AreaKind, Bounds, Constraint, CrossingDef, CrossingKind, JunctionControl, LevelFile,
    ObstacleKind, Shape, Sign, TurnBan,
};
use super::signals::{
    DEFAULT_ALL_RED_S, DEFAULT_AMBER_S, MAX_BICYCLE_LEAD_S, MAX_GREEN_S, MIN_GREEN_S, SignalPhase,
    SignalPlan, pair_opposite_arms,
};
use super::space::{Access, Crossability, Direction, Profile, Separator, StripUse};
use crate::geom::lanes as polylines;
use crate::geom::{Circle, Polygon};

/// A strip is never made narrower than this by moving a boundary.
pub const MIN_STRIP_WIDTH: f32 = 0.5;
/// The speed limits the player may set (km/h): "zone de rencontre", Paris default, and
/// the old urban limit.
pub const SPEED_LIMITS_KMH: [f32; 3] = [20.0, 30.0, 50.0];

/// The narrowest painted bicycle lane the player may make or leave (metres; start value,
/// the usual one-way lane width in French guidance).
pub const MIN_BIKE_LANE_WIDTH: f32 = 1.5;
/// The narrowest one-way bicycle track (metres; start value: a lane plus room for the
/// separator and for passing).
pub const MIN_BIKE_TRACK_WIDTH: f32 = 2.0;
/// A bicycle strip against the traffic of a one-way street needs a limit of at most this
/// (km/h; French "double-sens cyclable" practice, decree 2008-754: not checked against the
/// text here).
pub const MAX_CONTRAFLOW_LIMIT_KMH: f32 = 30.0;
/// The depth of a bicycle box (*sas vélo*), metres: 3 to 5 m (start values).
pub const MIN_BOX_DEPTH: f32 = 3.0;
pub const MAX_BOX_DEPTH: f32 = 5.0;

/// A bicycle box on one approach of a signalled junction. Cyclists stop at the crossing
/// (the end of the road lane); cars stop `depth` metres before it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BicycleBox {
    pub node: i32,
    pub segment: i32,
    pub depth: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreetNode {
    pub id: i32,
    pub position: Vec2,
    /// The segments that end here, in level order.
    pub segment_ids: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreetSegment {
    pub id: i32,
    pub name: String,
    pub from: i32,
    pub to: i32,
    /// From the `from` node to the `to` node.
    pub centerline: Vec<Vec2>,
    pub profile: Profile,
    pub speed_limit_kmh: f32,
    /// Distances cut from each end, up to the edge of the junction there.
    pub start_trim: f32,
    pub end_trim: f32,
    /// One per travel strip, in strip order.
    pub lane_ids: Vec<StreetLaneId>,
}

impl StreetSegment {
    pub fn strip_polygon(&self, index: usize) -> Vec<Vec2> {
        lanes::strip_polygon(
            &self.centerline,
            &self.profile,
            index,
            self.start_trim,
            self.end_trim,
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Junction {
    pub node: i32,
    pub polygon: Polygon,
    pub control: JunctionControl,
    /// Per approach segment.
    pub signs: IndexMap<i32, Sign>,
    /// The level's bans, then the player's.
    pub banned_turns: Vec<TurnBan>,
    pub signal_plan: Option<SignalPlan>,
    pub movement_ids: Vec<StreetLaneId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Obstacle {
    pub id: i32,
    pub kind: ObstacleKind,
    pub shape: Shape,
    /// Fixed by the level: the player cannot remove it.
    pub fixed: bool,
}

/// A crossing placed on a segment (pivot plan §3.3: crossings come from the player, and
/// from the level).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crossing {
    pub id: i32,
    pub kind: CrossingKind,
    pub segment: i32,
    /// Centre, metres along the centerline from the segment's `from` node.
    pub at: f32,
    pub width: f32,
}

/// A row of bollards the player placed along one kerb. `obstacle_ids` are its bollards in
/// `Street::obstacles`, in order along the segment.
#[derive(Clone, Debug, PartialEq)]
pub struct BollardRow {
    pub id: i32,
    pub segment: i32,
    pub side: Side,
    pub from: f32,
    pub to: f32,
    pub obstacle_ids: Vec<i32>,
}

/// Spacing of bollards in a row, metres: the Paris "potelet" (start value; the real spacing
/// is a little less so that the row ends on `to`).
pub const BOLLARD_SPACING: f32 = 1.5;
/// Radius of one bollard, metres (start value: a 20 cm post).
pub const BOLLARD_RADIUS: f32 = 0.1;

/// A crossing is never narrower than this, metres.
pub const MIN_CROSSING_WIDTH: f32 = 2.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    pub id: i32,
    pub kind: AreaKind,
    pub polygon: Polygon,
}

/// What an edit touched, so views redraw only that. Movements of the listed nodes and
/// lanes of the listed segments got new ids.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreetChange {
    pub node_ids: Vec<i32>,
    pub segment_ids: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    UnknownSegment(i32),
    UnknownStrip { segment: i32, strip: usize },
    UnknownJunction(i32),
    Locked { segment: i32, strip: usize },
    Invalid(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::UnknownSegment(id) => write!(f, "unknown segment {id}"),
            EditError::UnknownStrip { segment, strip } => {
                write!(f, "segment {segment} has no strip {strip}")
            }
            EditError::UnknownJunction(id) => write!(f, "no junction at node {id}"),
            EditError::Locked { segment, strip } => {
                write!(
                    f,
                    "strip {strip} of segment {segment} is locked by the level"
                )
            }
            EditError::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for EditError {}

#[derive(Clone, Debug, PartialEq)]
pub struct Street {
    pub level_id: String,
    pub bounds: Bounds,
    pub blocks: Vec<Polygon>,
    pub nodes: IndexMap<i32, StreetNode>,
    pub segments: IndexMap<i32, StreetSegment>,
    /// Keyed by node id.
    pub junctions: IndexMap<i32, Junction>,
    /// Road lanes and movements.
    pub lanes: IndexMap<StreetLaneId, StreetLane>,
    /// The level's walkable areas besides sidewalks (corners, plazas, islands).
    pub plazas: Vec<Polygon>,
    pub obstacles: IndexMap<i32, Obstacle>,
    pub crossings: IndexMap<i32, Crossing>,
    pub bollard_rows: IndexMap<i32, BollardRow>,
    /// Keyed by (junction node, approach segment).
    pub bicycle_boxes: IndexMap<(i32, i32), BicycleBox>,
    pub areas: IndexMap<i32, Area>,
    pub constraints: Vec<Constraint>,
    /// Bumped by every successful edit.
    pub version: u64,
    next_lane_id: i32,
    next_crossing_id: i32,
    next_obstacle_id: i32,
    next_bollard_row_id: i32,
}

impl Street {
    pub fn from_level(level: &LevelFile) -> Result<Street, EditError> {
        let mut street = Street {
            level_id: level.id.clone(),
            bounds: level.bounds,
            blocks: level.blocks.clone(),
            nodes: IndexMap::new(),
            segments: IndexMap::new(),
            junctions: IndexMap::new(),
            lanes: IndexMap::new(),
            plazas: level.plazas.clone(),
            obstacles: IndexMap::new(),
            crossings: IndexMap::new(),
            bollard_rows: IndexMap::new(),
            bicycle_boxes: IndexMap::new(),
            areas: IndexMap::new(),
            constraints: level.constraints.clone(),
            version: 0,
            next_lane_id: 1,
            next_crossing_id: 1,
            next_obstacle_id: 1,
            next_bollard_row_id: 1,
        };
        for node in &level.nodes {
            street.nodes.insert(
                node.id,
                StreetNode {
                    id: node.id,
                    position: node.position,
                    segment_ids: Vec::new(),
                },
            );
        }
        for def in &level.segments {
            let mut centerline = vec![street.nodes[&def.from].position];
            centerline.extend(&def.points);
            centerline.push(street.nodes[&def.to].position);
            street.nodes[&def.from].segment_ids.push(def.id);
            street.nodes[&def.to].segment_ids.push(def.id);
            street.segments.insert(
                def.id,
                StreetSegment {
                    id: def.id,
                    name: def.name.clone(),
                    from: def.from,
                    to: def.to,
                    centerline,
                    profile: def.profile.clone(),
                    speed_limit_kmh: def.speed_limit_kmh,
                    start_trim: 0.0,
                    end_trim: 0.0,
                    lane_ids: Vec::new(),
                },
            );
        }
        for def in &level.junctions {
            street.junctions.insert(
                def.node,
                Junction {
                    node: def.node,
                    polygon: def.polygon.clone(),
                    control: def.control,
                    signs: def.signs.iter().copied().collect(),
                    banned_turns: def.banned_turns.clone(),
                    signal_plan: def.signal_plan.clone(),
                    movement_ids: Vec::new(),
                },
            );
        }
        for node in street.nodes.values() {
            if node.segment_ids.len() >= 2 && !street.junctions.contains_key(&node.id) {
                return Err(EditError::Invalid(format!(
                    "node {} joins several segments but has no junction",
                    node.id
                )));
            }
        }
        for (i, def) in level.obstacles.iter().enumerate() {
            let id = i as i32 + 1;
            street.obstacles.insert(
                id,
                Obstacle {
                    id,
                    kind: def.kind,
                    shape: def.shape.clone(),
                    fixed: true,
                },
            );
            street.next_obstacle_id = id + 1;
        }
        for (i, def) in level.areas.iter().enumerate() {
            let id = i as i32 + 1;
            street.areas.insert(
                id,
                Area {
                    id,
                    kind: def.kind,
                    polygon: def.polygon.clone(),
                },
            );
        }
        let segment_ids: Vec<i32> = street.segments.keys().copied().collect();
        for id in &segment_ids {
            street.rebuild_segment_lanes(*id);
        }
        let junction_ids: Vec<i32> = street.junctions.keys().copied().collect();
        for node in junction_ids {
            street.rebuild_movements(node);
        }
        for def in &level.crossings {
            street.add_crossing(def)?;
        }
        Ok(street)
    }

    /// The level with the design's commands replayed in order.
    pub fn from_level_and_design(level: &LevelFile, design: &Design) -> Result<Street, EditError> {
        if design.level != level.id {
            return Err(EditError::Invalid(format!(
                "the design is for level '{}', not '{}'",
                design.level, level.id
            )));
        }
        let mut street = Street::from_level(level)?;
        for command in &design.commands {
            street.apply(command)?;
        }
        Ok(street)
    }

    /// Applies one edit. On an error nothing changed.
    pub fn apply(&mut self, command: &DesignCommand) -> Result<StreetChange, EditError> {
        let change = match *command {
            DesignCommand::SetStripUse {
                segment,
                strip,
                use_,
                direction,
            } => {
                if use_.is_travel() == (direction == Direction::None) {
                    return Err(EditError::Invalid(
                        "a travel strip needs a direction, other strips none".into(),
                    ));
                }
                let backup = self.edit_backup(segment);
                let target = self.unlocked_strip(segment, strip)?;
                target.use_ = use_;
                target.direction = direction;
                target.access = None;
                self.default_bicycle_separators(segment, strip);
                self.finish_profile_edit(segment, backup)?
            }
            DesignCommand::SetStripDirection {
                segment,
                strip,
                direction,
            } => {
                let backup = self.edit_backup(segment);
                let target = self.unlocked_strip(segment, strip)?;
                if !target.use_.is_travel() || direction == Direction::None {
                    return Err(EditError::Invalid(
                        "only travel strips have a direction".into(),
                    ));
                }
                target.direction = direction;
                self.finish_profile_edit(segment, backup)?
            }
            DesignCommand::MoveBoundary {
                segment,
                boundary,
                delta,
            } => {
                if boundary == 0 {
                    return Err(EditError::Invalid("the building line cannot move".into()));
                }
                let left = self.unlocked_strip(segment, boundary - 1)?.width + delta;
                let right = self.unlocked_strip(segment, boundary)?.width - delta;
                if left < MIN_STRIP_WIDTH || right < MIN_STRIP_WIDTH {
                    return Err(EditError::Invalid(format!(
                        "a strip cannot be narrower than {MIN_STRIP_WIDTH} m"
                    )));
                }
                let backup = self.edit_backup(segment);
                let strips = &mut self.segments[&segment].profile.strips;
                strips[boundary - 1].width = left;
                strips[boundary].width = right;
                self.finish_profile_edit(segment, backup)?
            }
            DesignCommand::SetSeparator {
                segment,
                boundary,
                separator,
            } => {
                let count = self
                    .segments
                    .get(&segment)
                    .ok_or(EditError::UnknownSegment(segment))?
                    .profile
                    .strips
                    .len();
                if boundary == 0 || boundary >= count {
                    return Err(EditError::Invalid(
                        "a separator goes between two strips, not on a building line".into(),
                    ));
                }
                self.unlocked_strip(segment, boundary - 1)?;
                self.unlocked_strip(segment, boundary)?;
                let backup = self.edit_backup(segment);
                let strips = &mut self.segments[&segment].profile.strips;
                strips[boundary - 1].right = separator;
                strips[boundary].left = separator;
                self.finish_profile_edit(segment, backup)?
            }
            DesignCommand::SetSpeedLimit { segment, kmh } => {
                if !SPEED_LIMITS_KMH.contains(&kmh) {
                    return Err(EditError::Invalid(format!("no {kmh} km/h speed limit")));
                }
                let backup = self.edit_backup(segment);
                self.segments
                    .get_mut(&segment)
                    .ok_or(EditError::UnknownSegment(segment))?
                    .speed_limit_kmh = kmh;
                self.finish_profile_edit(segment, backup)?
            }
            DesignCommand::BanTurn { node, ban } => {
                self.check_arms(node, &ban)?;
                let junction = &mut self.junctions[&node];
                if junction.banned_turns.contains(&ban) {
                    return Err(EditError::Invalid("this turn is already banned".into()));
                }
                junction.banned_turns.push(ban);
                self.junction_changed(node)
            }
            DesignCommand::UnbanTurn { node, ban } => {
                self.check_arms(node, &ban)?;
                let junction = &mut self.junctions[&node];
                let Some(index) = junction.banned_turns.iter().position(|b| *b == ban) else {
                    return Err(EditError::Invalid("this turn is not banned".into()));
                };
                junction.banned_turns.remove(index);
                self.junction_changed(node)
            }
            DesignCommand::AddCrossing { crossing } => {
                let segment = crossing.segment;
                self.add_crossing(&crossing)?;
                StreetChange {
                    node_ids: Vec::new(),
                    segment_ids: vec![segment],
                }
            }
            DesignCommand::RemoveCrossing { id } => {
                let crossing = self
                    .crossings
                    .shift_remove(&id)
                    .ok_or_else(|| EditError::Invalid(format!("no crossing {id}")))?;
                StreetChange {
                    node_ids: Vec::new(),
                    segment_ids: vec![crossing.segment],
                }
            }
            DesignCommand::AddBollards {
                segment,
                side,
                from,
                to,
            } => {
                self.add_bollards(segment, side, from, to)?;
                StreetChange {
                    node_ids: Vec::new(),
                    segment_ids: vec![segment],
                }
            }
            DesignCommand::RemoveBollards { id } => {
                let row = self
                    .bollard_rows
                    .shift_remove(&id)
                    .ok_or_else(|| EditError::Invalid(format!("no bollard row {id}")))?;
                for obstacle in &row.obstacle_ids {
                    self.obstacles.shift_remove(obstacle);
                }
                StreetChange {
                    node_ids: Vec::new(),
                    segment_ids: vec![row.segment],
                }
            }
            DesignCommand::SetJunctionControl {
                node,
                control,
                template,
            } => {
                if !self.junctions.contains_key(&node) {
                    return Err(EditError::UnknownJunction(node));
                }
                if control == JunctionControl::PriorityToRight
                    && self.bicycle_boxes.keys().any(|(n, _)| *n == node)
                {
                    return Err(EditError::Invalid(
                        "a bicycle box needs signals: remove the box first".into(),
                    ));
                }
                let plan = match (control, template) {
                    (JunctionControl::PriorityToRight, Some(_)) => {
                        return Err(EditError::Invalid(
                            "a junction without signals has no signal plan".into(),
                        ));
                    }
                    (JunctionControl::PriorityToRight, None) | (JunctionControl::Signals, None) => {
                        None
                    }
                    (JunctionControl::Signals, Some(template)) => {
                        Some(self.template_plan(node, template)?)
                    }
                };
                let junction = &mut self.junctions[&node];
                junction.control = control;
                junction.signal_plan = plan;
                self.junction_changed(node)
            }
            DesignCommand::AddBicycleBox {
                node,
                segment,
                depth,
            } => {
                self.add_bicycle_box(node, segment, depth)?;
                StreetChange {
                    node_ids: vec![node],
                    segment_ids: vec![segment],
                }
            }
            DesignCommand::RemoveBicycleBox { node, segment } => {
                self.bicycle_boxes
                    .shift_remove(&(node, segment))
                    .ok_or_else(|| {
                        EditError::Invalid(format!(
                            "no bicycle box on segment {segment} at junction {node}"
                        ))
                    })?;
                StreetChange {
                    node_ids: vec![node],
                    segment_ids: vec![segment],
                }
            }
            DesignCommand::SetBicycleLead { node, lead_s } => {
                if !self.junctions.contains_key(&node) {
                    return Err(EditError::UnknownJunction(node));
                }
                let Some(plan) = self.junctions[&node].signal_plan.as_mut() else {
                    return Err(EditError::Invalid(
                        "a leading bicycle green needs a signal plan: pick a signal template \
                         first"
                            .into(),
                    ));
                };
                let shortest = plan
                    .phases
                    .iter()
                    .map(|phase| phase.green_s)
                    .fold(f32::INFINITY, f32::min);
                if !lead_s.is_finite()
                    || !(0.0..=MAX_BICYCLE_LEAD_S).contains(&lead_s)
                    || lead_s >= shortest
                {
                    return Err(EditError::Invalid(format!(
                        "a leading bicycle green lasts 0 to {MAX_BICYCLE_LEAD_S} s and less \
                         than every green of the plan, not {lead_s} s"
                    )));
                }
                plan.bicycle_lead_s = lead_s;
                StreetChange {
                    node_ids: vec![node],
                    segment_ids: Vec::new(),
                }
            }
            DesignCommand::SetSign {
                node,
                segment,
                sign,
            } => {
                self.check_arm(node, segment)?;
                self.junctions[&node].signs.insert(segment, sign);
                StreetChange {
                    node_ids: vec![node],
                    segment_ids: Vec::new(),
                }
            }
        };
        self.version += 1;
        Ok(change)
    }

    /// Places a crossing: it must lie on the segment between the junction edges and
    /// overlap no other crossing. On an error nothing changed.
    fn add_crossing(&mut self, def: &CrossingDef) -> Result<i32, EditError> {
        let segment = self
            .segments
            .get(&def.segment)
            .ok_or(EditError::UnknownSegment(def.segment))?;
        let length = polylines::polyline_length(&segment.centerline) as f32;
        let (low, high) = (def.at - def.width * 0.5, def.at + def.width * 0.5);
        if !def.at.is_finite()
            || !def.width.is_finite()
            || def.width < MIN_CROSSING_WIDTH
            || low < segment.start_trim - 1e-3
            || high > length - segment.end_trim + 1e-3
        {
            return Err(EditError::Invalid(format!(
                "a crossing {} m wide at {} m does not fit on segment {}",
                def.width, def.at, def.segment
            )));
        }
        let overlaps = self.crossings.values().any(|other| {
            other.segment == def.segment
                && (other.at - def.at).abs() < (other.width + def.width) * 0.5
        });
        if overlaps {
            return Err(EditError::Invalid(
                "the crossing overlaps another one".into(),
            ));
        }
        let id = self.next_crossing_id;
        self.next_crossing_id += 1;
        self.crossings.insert(
            id,
            Crossing {
                id,
                kind: def.kind,
                segment: def.segment,
                at: def.at,
                width: def.width,
            },
        );
        Ok(id)
    }

    /// Places a row of bollards on the kerb line (the carriageway edge of the sidewalk
    /// strip nearest the carriageway on that side), so that each bollard is half on the
    /// sidewalk: it stops cars and walkers go round it. The row lies between the junction
    /// edges and overlaps no other row on the same side. On an error nothing changed.
    fn add_bollards(
        &mut self,
        segment_id: i32,
        side: Side,
        from: f32,
        to: f32,
    ) -> Result<i32, EditError> {
        let segment = self
            .segments
            .get(&segment_id)
            .ok_or(EditError::UnknownSegment(segment_id))?;
        let length = polylines::polyline_length(&segment.centerline) as f32;
        if !from.is_finite()
            || !to.is_finite()
            || from >= to
            || from < segment.start_trim - 1e-3
            || to > length - segment.end_trim + 1e-3
        {
            return Err(EditError::Invalid(format!(
                "bollards from {from} m to {to} m do not fit on segment {segment_id}"
            )));
        }
        let strips = &segment.profile.strips;
        let first_travel = strips.iter().position(|s| s.use_.is_travel());
        let last_travel = strips.iter().rposition(|s| s.use_.is_travel());
        let (Some(first_travel), Some(last_travel)) = (first_travel, last_travel) else {
            return Err(EditError::Invalid("the segment has no carriageway".into()));
        };
        let is_sidewalk = |i: &usize| strips[*i].use_ == StripUse::Sidewalk;
        let (strip, edge_at_right) = match side {
            Side::Left => ((0..first_travel).rev().find(is_sidewalk), true),
            Side::Right => (((last_travel + 1)..strips.len()).find(is_sidewalk), false),
        };
        let Some(strip) = strip else {
            return Err(EditError::Invalid(format!(
                "no sidewalk on that side of segment {segment_id}"
            )));
        };
        let (left, right) = segment.profile.strip_span(strip);
        let offset = if edge_at_right { right } else { left };
        let overlaps = self.bollard_rows.values().any(|row| {
            row.segment == segment_id && row.side == side && from < row.to && row.from < to
        });
        if overlaps {
            return Err(EditError::Invalid(
                "the bollards overlap another row".into(),
            ));
        }
        let line = polylines::offset_polyline(&segment.centerline, f64::from(offset));
        let gaps = ((to - from) / BOLLARD_SPACING).floor().max(1.0);
        let count = gaps as usize + 1;
        let step = (to - from) / gaps;
        let mut obstacle_ids = Vec::with_capacity(count);
        for k in 0..count {
            let d = from + step * k as f32;
            let center = polylines::sub_polyline(&line, f64::from(d), f64::from(d) + 0.01)
                .first()
                .copied()
                .expect("a segment centerline has length");
            let id = self.next_obstacle_id;
            self.next_obstacle_id += 1;
            self.obstacles.insert(
                id,
                Obstacle {
                    id,
                    kind: ObstacleKind::Bollard,
                    shape: Shape::Circle(Circle {
                        center,
                        radius: BOLLARD_RADIUS,
                    }),
                    fixed: false,
                },
            );
            obstacle_ids.push(id);
        }
        let id = self.next_bollard_row_id;
        self.next_bollard_row_id += 1;
        self.bollard_rows.insert(
            id,
            BollardRow {
                id,
                segment: segment_id,
                side,
                from,
                to,
                obstacle_ids,
            },
        );
        Ok(id)
    }

    /// The plan a template gives the junction at `node`: one phase per group of opposite
    /// arms (the grouping of the default plan), then for `TwoPhaseWithWalk` a phase with
    /// no green approach, in which pedestrians walk.
    fn template_plan(&self, node: i32, template: SignalTemplate) -> Result<SignalPlan, EditError> {
        let (green_s, walk_s) = match template {
            SignalTemplate::TwoPhase { green_s } => (green_s, None),
            SignalTemplate::TwoPhaseWithWalk { green_s, walk_s } => (green_s, Some(walk_s)),
        };
        for seconds in std::iter::once(green_s).chain(walk_s) {
            if !(MIN_GREEN_S..=MAX_GREEN_S).contains(&seconds) {
                return Err(EditError::Invalid(format!(
                    "a signal step must last {MIN_GREEN_S} to {MAX_GREEN_S} s, not {seconds}"
                )));
            }
        }
        // Arms with a movement, in the order of the junction's movements, each with the
        // heading of its feeder lane at the junction.
        let mut arms: Vec<(i32, Vec2)> = Vec::new();
        for movement in self.movements(node) {
            let feeder = &self.lanes[&movement.feeder.expect("a movement has a feeder")];
            let segment = feeder.segment_id.expect("a feeder is a road lane");
            if arms.iter().any(|(s, _)| *s == segment) {
                continue;
            }
            let line = feeder.polyline();
            let heading = (line[line.len() - 1] - line[line.len() - 2]).normalize_or_zero();
            arms.push((segment, heading));
        }
        let ids: Vec<i32> = arms.iter().map(|(s, _)| *s).collect();
        let heading_of = |segment: i32| {
            arms.iter()
                .find(|(s, _)| *s == segment)
                .map_or(Vec2::ZERO, |(_, h)| *h)
        };
        let mut phases: Vec<SignalPhase> = pair_opposite_arms(&ids, heading_of)
            .into_iter()
            .map(|green| SignalPhase { green, green_s })
            .collect();
        if let Some(walk_s) = walk_s {
            phases.push(SignalPhase {
                green: Vec::new(),
                green_s: walk_s,
            });
        }
        Ok(SignalPlan {
            phases,
            amber_s: DEFAULT_AMBER_S,
            all_red_s: DEFAULT_ALL_RED_S,
            bicycle_lead_s: 0.0,
        })
    }

    /// Road lanes of a segment, in strip order.
    pub fn segment_lanes(&self, segment: i32) -> impl Iterator<Item = &StreetLane> {
        self.segments[&segment]
            .lane_ids
            .iter()
            .map(|id| &self.lanes[id])
    }

    /// The movements of the junction at `node`.
    pub fn movements(&self, node: i32) -> impl Iterator<Item = &StreetLane> {
        self.junctions[&node]
            .movement_ids
            .iter()
            .map(|id| &self.lanes[id])
    }

    /// Where pedestrians may walk freely: sidewalk and terrace strips, then the plazas.
    pub fn walkable_polygons(&self) -> Vec<Polygon> {
        let mut polygons = Vec::new();
        for segment in self.segments.values() {
            for (index, strip) in segment.profile.strips.iter().enumerate() {
                if matches!(strip.use_, StripUse::Sidewalk | StripUse::Terrace) {
                    polygons.push(Polygon::new(segment.strip_polygon(index)));
                }
            }
        }
        polygons.extend(self.plazas.iter().cloned());
        polygons
    }

    fn unlocked_strip(
        &mut self,
        segment: i32,
        strip: usize,
    ) -> Result<&mut super::space::Strip, EditError> {
        let target = self
            .segments
            .get_mut(&segment)
            .ok_or(EditError::UnknownSegment(segment))?
            .profile
            .strips
            .get_mut(strip)
            .ok_or(EditError::UnknownStrip { segment, strip })?;
        if target.locked {
            return Err(EditError::Locked { segment, strip });
        }
        Ok(target)
    }

    fn check_arm(&self, node: i32, segment: i32) -> Result<(), EditError> {
        if !self.junctions.contains_key(&node) {
            return Err(EditError::UnknownJunction(node));
        }
        if !self.nodes[&node].segment_ids.contains(&segment) {
            return Err(EditError::Invalid(format!(
                "segment {segment} is not an arm of junction {node}"
            )));
        }
        Ok(())
    }

    fn check_arms(&self, node: i32, ban: &TurnBan) -> Result<(), EditError> {
        self.check_arm(node, ban.from_segment)?;
        self.check_arm(node, ban.to_segment)
    }

    fn segment_changed(&mut self, segment: i32) -> StreetChange {
        self.rebuild_segment_lanes(segment);
        let (from, to) = (self.segments[&segment].from, self.segments[&segment].to);
        let mut node_ids = vec![from, to];
        node_ids.retain(|node| self.junctions.contains_key(node));
        for &node in &node_ids {
            self.rebuild_movements(node);
        }
        StreetChange {
            node_ids,
            segment_ids: vec![segment],
        }
    }

    fn junction_changed(&mut self, node: i32) -> StreetChange {
        self.rebuild_movements(node);
        StreetChange {
            node_ids: vec![node],
            segment_ids: Vec::new(),
        }
    }

    fn trim_at(&self, node: i32, from_node_side: &[Vec2]) -> f32 {
        self.junctions
            .get(&node)
            .and_then(|junction| junction.polygon.first_exit_along(from_node_side))
            .unwrap_or(0.0)
    }

    fn rebuild_segment_lanes(&mut self, segment_id: i32) {
        let segment = &self.segments[&segment_id];
        let mut reversed = segment.centerline.clone();
        reversed.reverse();
        let start_trim = self.trim_at(segment.from, &segment.centerline);
        let end_trim = self.trim_at(segment.to, &reversed);
        let specs = lanes::strip_lanes(&segment.centerline, &segment.profile, start_trim, end_trim);
        let speed_limit = f64::from(segment.speed_limit_kmh) / 3.6;
        let (from, to) = (segment.from, segment.to);
        for id in std::mem::take(&mut self.segments[&segment_id].lane_ids) {
            self.lanes.shift_remove(&id);
        }
        let mut lane_ids = Vec::new();
        for spec in specs {
            let id = StreetLaneId(self.next_lane_id);
            self.next_lane_id += 1;
            let curve = lanes::curve_from_points(&spec.points);
            let (from_node, to_node) = if spec.forward { (from, to) } else { (to, from) };
            self.lanes.insert(
                id,
                StreetLane {
                    id,
                    segment_id: Some(segment_id),
                    strip: Some(spec.strip),
                    from_node,
                    to_node,
                    length: polylines::polyline_length(&spec.points),
                    curve,
                    width: spec.width,
                    access: spec.access,
                    lateral: spec.lateral,
                    speed_limit,
                    turn: Turn::None,
                    feeder: None,
                    outgoing: Vec::new(),
                    conflict_zones: IndexMap::new(),
                },
            );
            lane_ids.push(id);
        }
        let segment = &mut self.segments[&segment_id];
        segment.start_trim = start_trim;
        segment.end_trim = end_trim;
        segment.lane_ids = lane_ids;
    }

    fn lane_group(&self, lane: &StreetLane) -> LaneGroup {
        let segment = &self.segments[&lane.segment_id.expect("road lane")];
        let use_ = segment.profile.strips[lane.strip.expect("road lane")].use_;
        LaneGroup::of(use_, lane.access)
    }

    /// The lanes of `segment` that leave (or reach) `node`, in `group`, left to right.
    fn approach_lanes(
        &self,
        segment: i32,
        node: i32,
        leaving: bool,
        group: LaneGroup,
    ) -> Vec<StreetLaneId> {
        let mut ids: Vec<StreetLaneId> = self
            .segment_lanes(segment)
            .filter(|lane| {
                (if leaving {
                    lane.from_node
                } else {
                    lane.to_node
                }) == node
            })
            .filter(|lane| self.lane_group(lane) == group)
            .map(|lane| lane.id)
            .collect();
        ids.sort_by(|a, b| self.lanes[a].lateral.total_cmp(&self.lanes[b].lateral));
        ids
    }

    /// Regenerates the movements of a junction from the rules (pivot plan §3.2), then
    /// takes the banned turns away, then finds the conflicts.
    fn rebuild_movements(&mut self, node: i32) {
        for id in std::mem::take(&mut self.junctions[&node].movement_ids) {
            self.lanes.shift_remove(&id);
        }
        let arms = self.nodes[&node].segment_ids.clone();
        let mut movement_ids = Vec::new();
        for &from_segment in &arms {
            for in_id in self.segments[&from_segment].lane_ids.clone() {
                if self.lanes[&in_id].to_node != node {
                    continue;
                }
                self.lanes[&in_id].outgoing.clear();
                let group = self.lane_group(&self.lanes[&in_id]);
                let own = self.approach_lanes(from_segment, node, false, group);
                let index = own.iter().position(|id| *id == in_id).expect("own lane");
                for &to_segment in &arms {
                    if to_segment == from_segment {
                        continue;
                    }
                    let Some((target_group, targets)) = group.targets().iter().find_map(|g| {
                        let lanes = self.approach_lanes(to_segment, node, true, *g);
                        (!lanes.is_empty()).then_some((*g, lanes))
                    }) else {
                        continue;
                    };
                    let (in_lane, first_target) = (&self.lanes[&in_id], &self.lanes[&targets[0]]);
                    let turn = lanes::classify_turn(
                        polylines::end_direction(&in_lane.curve),
                        polylines::start_direction(&first_target.curve),
                    );
                    let Some(target) = lanes::target_index(
                        turn,
                        index,
                        own.len(),
                        targets.len(),
                        target_group == group,
                    ) else {
                        continue;
                    };
                    let out_lane = &self.lanes[&targets[target]];
                    let mut access = in_lane.access & out_lane.access;
                    for ban in &self.junctions[&node].banned_turns {
                        if ban.from_segment == from_segment && ban.to_segment == to_segment {
                            access = access.without(ban.access);
                        }
                    }
                    if access.is_empty() {
                        continue;
                    }
                    let id = StreetLaneId(self.next_lane_id);
                    self.next_lane_id += 1;
                    let curve = lanes::connector_curve(&in_lane.curve, &out_lane.curve);
                    let movement = StreetLane {
                        id,
                        segment_id: None,
                        strip: None,
                        from_node: node,
                        to_node: node,
                        length: f64::from(curve.baked_length()),
                        curve,
                        width: in_lane.width.min(out_lane.width),
                        access,
                        lateral: 0.0,
                        speed_limit: in_lane.speed_limit.min(out_lane.speed_limit),
                        turn,
                        feeder: Some(in_id),
                        outgoing: vec![out_lane.id],
                        conflict_zones: IndexMap::new(),
                    };
                    self.lanes.insert(id, movement);
                    self.lanes[&in_id].outgoing.push(id);
                    movement_ids.push(id);
                }
            }
        }
        self.junctions[&node].movement_ids = movement_ids;
        self.compute_conflicts(node);
    }

    /// Movements conflict where their paths come closer than their half widths (crossing
    /// or merging). Movements from the same lane don't: their users already follow each
    /// other.
    fn compute_conflicts(&mut self, node: i32) {
        let ids = self.junctions[&node].movement_ids.clone();
        let polylines: Vec<Vec<Vec2>> = ids.iter().map(|id| self.lanes[id].polyline()).collect();
        for i in 0..ids.len() {
            for j in i + 1..ids.len() {
                let (a, b) = (&self.lanes[&ids[i]], &self.lanes[&ids[j]]);
                if a.feeder == b.feeder {
                    continue;
                }
                let Some((zone_a, zone_b)) = lanes::conflict_zones(
                    &polylines[i],
                    a.width,
                    a.length,
                    &polylines[j],
                    b.width,
                    b.length,
                    a.outgoing[0] == b.outgoing[0],
                ) else {
                    continue;
                };
                self.lanes[&ids[i]].conflict_zones.insert(ids[j], zone_a);
                self.lanes[&ids[j]].conflict_zones.insert(ids[i], zone_b);
            }
        }
    }

    /// The classes allowed on each movement of a junction, by (from segment, to segment).
    pub fn movement_access(&self, node: i32, from_segment: i32, to_segment: i32) -> Access {
        self.movements(node)
            .filter(|m| {
                let feeder = &self.lanes[&m.feeder.expect("movement")];
                let out = &self.lanes[&m.outgoing[0]];
                feeder.segment_id == Some(from_segment) && out.segment_id == Some(to_segment)
            })
            .fold(Access::NONE, |all, m| all | m.access)
    }
}

/// Why `profile` is not a valid street with its bicycle strips, if it is not. `before` is
/// the profile before the edit: a strip that was already too narrow is tolerated as long
/// as the edit does not make it narrower.
///
/// The rules (start values and French practice, see the constants):
/// - a painted lane is at least `MIN_BIKE_LANE_WIDTH` wide, a track `MIN_BIKE_TRACK_WIDTH`;
/// - cyclists keep to the right of their own direction: a bicycle strip lies on the right
///   of the car and bus lanes that run the other way (so in a two-way street it is on the
///   side of the traffic it rides with, and a contraflow lane lies on the left of the
///   one-way carriageway);
/// - a strip against the traffic of a one-way street needs a limit of at most
///   `MAX_CONTRAFLOW_LIMIT_KMH`;
/// - a track next to a car or bus lane has a kerb, bollards or a planter between them.
fn check_bicycle_strips(
    profile: &Profile,
    speed_limit_kmh: f32,
    before: Option<&Profile>,
) -> Result<(), String> {
    let strips = &profile.strips;
    let motor: Vec<(usize, Direction)> = strips
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            matches!(s.use_, StripUse::CarLane | StripUse::BusLane)
                && s.direction != Direction::None
        })
        .map(|(i, s)| (i, s.direction))
        .collect();
    let one_way = motor
        .first()
        .map(|(_, d)| *d)
        .filter(|d| motor.iter().all(|(_, other)| other == d));
    for (i, strip) in strips.iter().enumerate() {
        let (name, minimum) = match strip.use_ {
            StripUse::BikeLane => ("bicycle lane", MIN_BIKE_LANE_WIDTH),
            StripUse::BikeTrack => ("bicycle track", MIN_BIKE_TRACK_WIDTH),
            _ => continue,
        };
        let tolerated = before.is_some_and(|b| {
            b.strips
                .get(i)
                .is_some_and(|old| old.use_ == strip.use_ && old.width <= strip.width + 1e-4)
        });
        if strip.width < minimum - 1e-4 && !tolerated {
            return Err(format!(
                "a {name} needs at least {minimum} m, this strip is {} m",
                strip.width
            ));
        }
        for &(j, d) in &motor {
            let wrong_side = match strip.direction {
                Direction::Backward => d == Direction::Forward && i > j,
                Direction::Forward => d == Direction::Backward && i < j,
                Direction::None => false,
            };
            if wrong_side {
                return Err(format!(
                    "a {name} must lie on the right of the car lanes that run the other way, \
                     so that cyclists keep to the right of their own direction"
                ));
            }
        }
        if let Some(flow) = one_way
            && flow != strip.direction
            && speed_limit_kmh > MAX_CONTRAFLOW_LIMIT_KMH
        {
            return Err(format!(
                "a {name} against the traffic is only allowed on a one-way street limited to \
                 {MAX_CONTRAFLOW_LIMIT_KMH} km/h or less (the limit here is {speed_limit_kmh} km/h)"
            ));
        }
        if strip.use_ == StripUse::BikeTrack {
            for (neighbour, own, before_us) in [
                (i.checked_sub(1), strip.left, true),
                (Some(i + 1), strip.right, false),
            ] {
                let Some(other) = neighbour.and_then(|n| strips.get(n)) else {
                    continue;
                };
                if !matches!(other.use_, StripUse::CarLane | StripUse::BusLane) {
                    continue;
                }
                let theirs = if before_us { other.right } else { other.left };
                if !own.is_physical() && !theirs.is_physical() {
                    return Err(
                        "a bicycle track next to a car lane needs a kerb, bollards or a \
                         planter between them"
                            .into(),
                    );
                }
            }
        }
    }
    Ok(())
}

impl Street {
    fn edit_backup(&self, segment: i32) -> Option<(Profile, f32)> {
        self.segments
            .get(&segment)
            .map(|s| (s.profile.clone(), s.speed_limit_kmh))
    }

    /// Ends a profile or speed edit: checks the bicycle rules on the result and, if they
    /// fail, puts the old profile and limit back, so that an error changes nothing.
    fn finish_profile_edit(
        &mut self,
        segment: i32,
        backup: Option<(Profile, f32)>,
    ) -> Result<StreetChange, EditError> {
        let target = &self.segments[&segment];
        let before = backup.as_ref().map(|(profile, _)| profile);
        if let Err(message) = check_bicycle_strips(&target.profile, target.speed_limit_kmh, before)
        {
            if let Some((profile, limit)) = backup {
                let target = &mut self.segments[&segment];
                target.profile = profile;
                target.speed_limit_kmh = limit;
            }
            return Err(EditError::Invalid(message));
        }
        Ok(self.segment_changed(segment))
    }

    /// Gives a new bicycle strip the separators its use needs next to car lanes: paint
    /// for a lane (next to parking too), a kerb for a track. A separator that is already
    /// there stays (a track keeps any physical one); a locked neighbour is left alone.
    fn default_bicycle_separators(&mut self, segment: i32, strip: usize) {
        let strips = &mut self.segments[&segment].profile.strips;
        let track = match strips[strip].use_ {
            StripUse::BikeLane => false,
            StripUse::BikeTrack => true,
            _ => return,
        };
        for neighbour in [strip.checked_sub(1), Some(strip + 1)]
            .into_iter()
            .flatten()
        {
            let Some(other) = strips.get(neighbour) else {
                continue;
            };
            if other.locked {
                continue;
            }
            let want = match (other.use_, track) {
                (StripUse::CarLane | StripUse::BusLane, true) => Separator::Kerb,
                (StripUse::CarLane | StripUse::BusLane, false)
                | (StripUse::Parking | StripUse::DeliveryBay, false) => Separator::Paint,
                _ => continue,
            };
            let (own_side, other_side) = if neighbour < strip {
                (strips[strip].left, strips[neighbour].right)
            } else {
                (strips[strip].right, strips[neighbour].left)
            };
            let keep = if track {
                own_side.is_physical() || other_side.is_physical()
            } else {
                own_side != Separator::None || other_side != Separator::None
            };
            if keep {
                continue;
            }
            if neighbour < strip {
                strips[strip].left = want;
                strips[neighbour].right = want;
            } else {
                strips[strip].right = want;
                strips[neighbour].left = want;
            }
        }
    }

    /// Places a bicycle box on an approach of a signalled junction. On an error nothing
    /// changed.
    fn add_bicycle_box(&mut self, node: i32, segment: i32, depth: f32) -> Result<(), EditError> {
        self.check_arm(node, segment)?;
        if !depth.is_finite() || !(MIN_BOX_DEPTH..=MAX_BOX_DEPTH).contains(&depth) {
            return Err(EditError::Invalid(format!(
                "a bicycle box is {MIN_BOX_DEPTH} to {MAX_BOX_DEPTH} m deep, not {depth} m"
            )));
        }
        let junction = &self.junctions[&node];
        if junction.control != JunctionControl::Signals {
            return Err(EditError::Invalid(
                "a bicycle box goes on a signalled approach: put signals on the junction first"
                    .into(),
            ));
        }
        if let Some(plan) = &junction.signal_plan
            && !plan.phases.iter().any(|p| p.green.contains(&segment))
        {
            return Err(EditError::Invalid(format!(
                "segment {segment} has no green in the signal plan of junction {node}"
            )));
        }
        if self.bicycle_boxes.contains_key(&(node, segment)) {
            return Err(EditError::Invalid(
                "this approach already has a bicycle box".into(),
            ));
        }
        let incoming: Vec<&StreetLane> = self
            .segment_lanes(segment)
            .filter(|lane| lane.to_node == node)
            .collect();
        let bicycles = incoming
            .iter()
            .any(|l| l.access.intersects(Access::BICYCLE));
        let motor = incoming.iter().any(|l| l.access.intersects(Access::MOTOR));
        if !bicycles || !motor {
            return Err(EditError::Invalid(
                "a bicycle box needs lanes for bicycles and for cars on the approach".into(),
            ));
        }
        let shortest = incoming
            .iter()
            .map(|l| l.length)
            .fold(f64::INFINITY, f64::min);
        if f64::from(depth) + 1.0 > shortest {
            return Err(EditError::Invalid(
                "the approach is too short for this bicycle box".into(),
            ));
        }
        self.bicycle_boxes.insert(
            (node, segment),
            BicycleBox {
                node,
                segment,
                depth,
            },
        );
        Ok(())
    }

    /// The bicycle box on approach `segment` of junction `node`, if any.
    pub fn bicycle_box(&self, node: i32, segment: i32) -> Option<&BicycleBox> {
        self.bicycle_boxes.get(&(node, segment))
    }

    /// Where cars stop on a road lane, as a distance from its start. This is the end of
    /// the lane (the crossing line), less the depth of the bicycle box of its approach,
    /// for lanes that motor vehicles use. Movements and unknown lanes give their length.
    pub fn car_stop_distance(&self, lane: StreetLaneId) -> f64 {
        let Some(lane) = self.lanes.get(&lane) else {
            return 0.0;
        };
        let depth = lane
            .segment_id
            .filter(|_| lane.access.intersects(Access::MOTOR))
            .and_then(|segment| self.bicycle_box(lane.to_node, segment))
            .map_or(0.0, |b| f64::from(b.depth));
        (lane.length - depth).max(0.0)
    }

    /// Where cyclists stop on a road lane: always the crossing line, the end of the lane,
    /// so that with a bicycle box they wait in front of the cars.
    pub fn bicycle_stop_distance(&self, lane: StreetLaneId) -> f64 {
        self.lanes.get(&lane).map_or(0.0, |lane| lane.length)
    }

    /// How hard it is for `mode` to cross boundary `boundary` of `segment` (between strips
    /// `boundary - 1` and `boundary`): the hardest of the two separators on that line, see
    /// `Separator::crossability`. The building lines (0 and the last) and unknown
    /// boundaries are `Blocked`. The drive code reads this: `Paint` is what a careless
    /// driver may cross to park, stop or pass; `Blocked` nobody crosses.
    pub fn boundary_crossability(
        &self,
        segment: i32,
        boundary: usize,
        mode: Access,
    ) -> Crossability {
        let Some(segment) = self.segments.get(&segment) else {
            return Crossability::Blocked;
        };
        let strips = &segment.profile.strips;
        if boundary == 0 || boundary >= strips.len() {
            return Crossability::Blocked;
        }
        strips[boundary - 1]
            .right
            .crossability(mode)
            .max(strips[boundary].left.crossability(mode))
    }

    /// Can `mode` physically cross boundary `boundary` of `segment`? (`Free` or `Paint`;
    /// whether it is allowed to is `boundary_crossability`.)
    pub fn crossable_by(&self, segment: i32, boundary: usize, mode: Access) -> bool {
        self.boundary_crossability(segment, boundary, mode) != Crossability::Blocked
    }

    /// Can `mode` get from strip `from_strip` to strip `to_strip` of `segment` without a
    /// barrier: no `Blocked` boundary in between, and for wheeled classes no strip in
    /// between that blocks wheels (planting, median, sidewalk, terrace: the buffer strips).
    pub fn crossable_between(
        &self,
        segment: i32,
        from_strip: usize,
        to_strip: usize,
        mode: Access,
    ) -> bool {
        let Some(found) = self.segments.get(&segment) else {
            return false;
        };
        let strips = &found.profile.strips;
        let (low, high) = (from_strip.min(to_strip), from_strip.max(to_strip));
        if high >= strips.len() {
            return false;
        }
        let wheeled = mode.intersects(Access::BICYCLE | Access::MOTOR);
        (low + 1..=high).all(|b| self.crossable_by(segment, b, mode))
            && (low + 1..high).all(|i| !(wheeled && strips[i].use_.blocks_wheels()))
    }
}
