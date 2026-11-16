//! The level file (pivot plan §3.4): one RON file per level in `data/levels/`, embedded
//! with `include_str!`. A level is authored data: its nodes and segments never split, so
//! their ids are stable and a design can refer to them. Metres, y down.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::signals::SignalPlan;
use super::space::{Access, Profile};
use crate::geom::{Circle, Polygon};

/// Every level shipped with the game: (id, RON text).
const LEVELS: &[(&str, &str)] = &[(
    "carrefour-ecole",
    include_str!("../../data/levels/carrefour_ecole.ron"),
)];

pub fn level_ids() -> impl Iterator<Item = &'static str> {
    LEVELS.iter().map(|(id, _)| *id)
}

#[derive(Clone, Debug, PartialEq)]
pub enum LevelError {
    Unknown(String),
    Parse(String),
    Invalid(String),
}

impl std::fmt::Display for LevelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LevelError::Unknown(id) => write!(f, "unknown level '{id}'"),
            LevelError::Parse(message) => write!(f, "level file: {message}"),
            LevelError::Invalid(message) => write!(f, "invalid level: {message}"),
        }
    }
}

impl std::error::Error for LevelError {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min: Vec2,
    pub max: Vec2,
}

impl Bounds {
    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeDef {
    pub id: i32,
    pub position: Vec2,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SegmentDef {
    pub id: i32,
    #[serde(default)]
    pub name: String,
    pub from: i32,
    pub to: i32,
    /// Interior points of the centerline, between the two nodes.
    #[serde(default)]
    pub points: Vec<Vec2>,
    pub profile: Profile,
    pub speed_limit_kmh: f32,
}

/// Who gives way at a junction without signs or signals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JunctionControl {
    /// The French default rule.
    #[default]
    PriorityToRight,
    /// Lights, run by the junction's `signal_plan` (or a default plan made from its arms).
    Signals,
}

/// A sign on one approach of a junction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Sign {
    #[default]
    None,
    Yield,
    Stop,
}

/// A movement from one arm of a junction to another that some classes may not make.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TurnBan {
    pub from_segment: i32,
    pub to_segment: i32,
    /// The classes the ban applies to.
    pub access: Access,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JunctionDef {
    pub node: i32,
    /// The conflict area: lanes stop at its edge and movements cross it.
    pub polygon: Polygon,
    #[serde(default)]
    pub control: JunctionControl,
    /// (approach segment, sign).
    #[serde(default)]
    pub signs: Vec<(i32, Sign)>,
    /// The level's default turn bans; the player's come from the design.
    #[serde(default)]
    pub banned_turns: Vec<TurnBan>,
    /// Used when `control` is `Signals`; without one the run makes a default plan.
    #[serde(default)]
    pub signal_plan: Option<SignalPlan>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObstacleKind {
    Bollard,
    Barrier,
    Planter,
    Tree,
    Bench,
    Pillar,
    MetroExit,
    BusShelter,
    /// A van parked for good (in a parking strip or a delivery bay): it hides a child.
    ParkedVan,
}

impl ObstacleKind {
    /// Tall obstacles hide what is behind them.
    pub fn blocks_sight(self) -> bool {
        matches!(
            self,
            ObstacleKind::Tree
                | ObstacleKind::Pillar
                | ObstacleKind::MetroExit
                | ObstacleKind::BusShelter
                | ObstacleKind::ParkedVan
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Circle(Circle),
    Polygon(Polygon),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObstacleDef {
    pub kind: ObstacleKind,
    pub shape: Shape,
}

/// What a crossing is. Raised crossings, refuge islands and crossings with their own
/// signals come later (pivot plan §4, "Crossings").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CrossingKind {
    /// A "passage piéton": pedestrians on it have priority (Code de la route R415-11).
    #[default]
    Zebra,
}

/// The default width of a crossing along its street, metres: about a Paris zebra (at
/// least 2.5 m wide in France).
pub const DEFAULT_CROSSING_WIDTH: f32 = 3.0;

fn default_crossing_width() -> f32 {
    DEFAULT_CROSSING_WIDTH
}

/// A place where pedestrians cross a street by the rules: across every strip between
/// the sidewalks, centred `at` metres along the segment's centerline from its `from`
/// node, `width` metres wide along it. A crossing at a junction arm sits just outside
/// the junction area; one further along is mid-block.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrossingDef {
    #[serde(default)]
    pub kind: CrossingKind,
    pub segment: i32,
    pub at: f32,
    #[serde(default = "default_crossing_width")]
    pub width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AreaKind {
    Green,
    Rest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AreaDef {
    pub kind: AreaKind,
    pub polygon: Polygon,
}

/// Where agents enter and leave the level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    pub id: String,
    pub position: Vec2,
    /// The segment the gate opens onto.
    pub segment: i32,
    /// The classes that use it.
    pub access: Access,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    Car,
    Van,
    Bus,
    Bicycle,
    Pedestrian,
}

/// So many agents of one mode per hour from one gate to another.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    pub mode: Mode,
    pub from: String,
    pub to: String,
    pub per_hour: u32,
}

/// One time slice of the day (pivot plan §5.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slice {
    pub name: String,
    /// Play time at speed 1.
    pub duration_s: f32,
    pub flows: Vec<Flow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Constraint {
    /// Every sidewalk strip keeps at least this width.
    MinSidewalkWidth(f32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelFile {
    pub id: String,
    pub name: String,
    pub place: String,
    #[serde(default)]
    pub history: String,
    pub difficulty: u8,
    pub bounds: Bounds,
    /// Building blocks: drawing only.
    #[serde(default)]
    pub blocks: Vec<Polygon>,
    pub nodes: Vec<NodeDef>,
    pub segments: Vec<SegmentDef>,
    pub junctions: Vec<JunctionDef>,
    /// Walkable areas besides the sidewalk strips: corners, plazas, islands.
    #[serde(default)]
    pub plazas: Vec<Polygon>,
    /// Fixed furniture the player cannot remove.
    #[serde(default)]
    pub obstacles: Vec<ObstacleDef>,
    /// Crossings the level starts with; the player's come from the design.
    #[serde(default)]
    pub crossings: Vec<CrossingDef>,
    #[serde(default)]
    pub areas: Vec<AreaDef>,
    #[serde(default)]
    pub gates: Vec<Gate>,
    #[serde(default)]
    pub demand: Vec<Slice>,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// The tools the player may use in this level.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Scores for 2 and 3 stars.
    #[serde(default)]
    pub stars: Vec<f64>,
    pub seed: u64,
}

impl LevelFile {
    /// A level shipped with the game.
    pub fn load(id: &str) -> Result<LevelFile, LevelError> {
        let (_, text) = LEVELS
            .iter()
            .find(|(level_id, _)| *level_id == id)
            .ok_or_else(|| LevelError::Unknown(id.to_string()))?;
        Self::from_ron(text)
    }

    pub fn from_ron(text: &str) -> Result<LevelFile, LevelError> {
        let level: LevelFile =
            ron::from_str(text).map_err(|error| LevelError::Parse(error.to_string()))?;
        level.validate()?;
        Ok(level)
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("a level always serializes")
    }

    /// The references between the parts of the file hold.
    pub fn validate(&self) -> Result<(), LevelError> {
        let invalid = |message: String| Err(LevelError::Invalid(message));
        let has_node = |id: i32| self.nodes.iter().any(|node| node.id == id);
        let has_segment = |id: i32| self.segments.iter().any(|segment| segment.id == id);
        for (i, node) in self.nodes.iter().enumerate() {
            if self.nodes[..i].iter().any(|other| other.id == node.id) {
                return invalid(format!("node {} is defined twice", node.id));
            }
        }
        for (i, segment) in self.segments.iter().enumerate() {
            if self.segments[..i]
                .iter()
                .any(|other| other.id == segment.id)
            {
                return invalid(format!("segment {} is defined twice", segment.id));
            }
            if !has_node(segment.from) || !has_node(segment.to) || segment.from == segment.to {
                return invalid(format!("segment {} has bad end nodes", segment.id));
            }
            if segment.profile.strips.is_empty() {
                return invalid(format!("segment {} has no strips", segment.id));
            }
            for strip in &segment.profile.strips {
                if strip.width <= 0.0
                    || strip.use_.is_travel() != (strip.direction != super::space::Direction::None)
                {
                    return invalid(format!(
                        "segment {}: a strip has a bad width or direction",
                        segment.id
                    ));
                }
            }
        }
        for junction in &self.junctions {
            if !has_node(junction.node) {
                return invalid(format!("junction at unknown node {}", junction.node));
            }
            let arms_ok = junction.signs.iter().map(|(segment, _)| *segment).chain(
                junction
                    .banned_turns
                    .iter()
                    .flat_map(|ban| [ban.from_segment, ban.to_segment]),
            );
            for segment in arms_ok {
                if !self
                    .segments
                    .iter()
                    .any(|s| s.id == segment && (s.from == junction.node || s.to == junction.node))
                {
                    return invalid(format!(
                        "junction {}: segment {segment} is not one of its arms",
                        junction.node
                    ));
                }
            }
        }
        for crossing in &self.crossings {
            if !has_segment(crossing.segment) {
                return invalid(format!(
                    "a crossing is on unknown segment {}",
                    crossing.segment
                ));
            }
        }
        for gate in &self.gates {
            if !has_segment(gate.segment) {
                return invalid(format!(
                    "gate {} is on unknown segment {}",
                    gate.id, gate.segment
                ));
            }
        }
        for slice in &self.demand {
            for flow in &slice.flows {
                for gate in [&flow.from, &flow.to] {
                    if !self.gates.iter().any(|g| &g.id == gate) {
                        return invalid(format!("slice {}: unknown gate {gate}", slice.name));
                    }
                }
            }
        }
        Ok(())
    }
}
