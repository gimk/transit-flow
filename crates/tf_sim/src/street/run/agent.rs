//! Agents: a body, a personality and the state of the trip (pivot plan §5.2, §5.3).
//! Phase 2 has motor vehicles only; they follow their route's line.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::events::CauseKind;
use crate::rng::SimRng;
use crate::street::ids::{AgentId, StreetLaneId};
use crate::street::level::Mode;
use crate::street::space::Access;

/// Who drives in a run. The level's own archetype mix comes with Phase 8; until then a
/// run picks one of these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Population {
    /// Full compliance and attention: the reference that must never have a contact.
    Careful,
    /// The everyday mix of archetypes.
    #[default]
    Mixed,
    /// Hurried, distracted, rule-bending: the stress case.
    Careless,
}

/// The ranges a personality is drawn from: (lowest, highest) per trait.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Archetype {
    pub name: &'static str,
    pub speed_factor: (f32, f32),
    pub patience_s: (f32, f32),
    pub compliance: (f32, f32),
    pub attention: (f32, f32),
    pub reaction_s: (f32, f32),
    pub risk: (f32, f32),
    pub courtesy: (f32, f32),
}

// Start values, to be calibrated in Phase 8.
pub const CAREFUL: Archetype = Archetype {
    name: "careful",
    speed_factor: (0.85, 1.0),
    patience_s: (40.0, 90.0),
    compliance: (1.0, 1.0),
    attention: (1.0, 1.0),
    reaction_s: (0.6, 1.2),
    risk: (0.0, 0.0),
    courtesy: (1.0, 1.0),
};
pub const RESIDENT: Archetype = Archetype {
    name: "resident",
    speed_factor: (0.85, 1.05),
    patience_s: (40.0, 90.0),
    compliance: (0.9, 1.0),
    attention: (0.93, 1.0),
    reaction_s: (0.7, 1.3),
    risk: (0.0, 0.3),
    courtesy: (0.6, 1.0),
};
pub const COMMUTER: Archetype = Archetype {
    name: "commuter",
    speed_factor: (0.95, 1.25),
    patience_s: (20.0, 50.0),
    compliance: (0.7, 0.95),
    attention: (0.85, 0.98),
    reaction_s: (0.6, 1.1),
    risk: (0.2, 0.7),
    courtesy: (0.2, 0.8),
};
pub const TAXI: Archetype = Archetype {
    name: "taxi",
    speed_factor: (1.0, 1.3),
    patience_s: (15.0, 40.0),
    compliance: (0.6, 0.9),
    attention: (0.9, 1.0),
    reaction_s: (0.5, 0.9),
    risk: (0.4, 0.8),
    courtesy: (0.1, 0.6),
};
pub const DELIVERY: Archetype = Archetype {
    name: "delivery",
    speed_factor: (0.9, 1.2),
    patience_s: (15.0, 45.0),
    compliance: (0.6, 0.9),
    attention: (0.8, 0.97),
    reaction_s: (0.7, 1.3),
    risk: (0.3, 0.7),
    courtesy: (0.2, 0.7),
};
pub const CARELESS: Archetype = Archetype {
    name: "careless",
    speed_factor: (1.05, 1.5),
    patience_s: (10.0, 30.0),
    compliance: (0.2, 0.7),
    attention: (0.6, 0.9),
    reaction_s: (0.8, 1.6),
    risk: (0.5, 1.0),
    courtesy: (0.0, 0.5),
};

impl Population {
    /// One draw from the agent's stream picks the archetype.
    pub fn archetype(self, mode: Mode, rng: &mut SimRng) -> Archetype {
        let pick = rng.randf();
        match self {
            Population::Careful => CAREFUL,
            Population::Careless => CARELESS,
            Population::Mixed => match mode {
                Mode::Van | Mode::Bus => DELIVERY,
                _ if pick < 0.5 => RESIDENT,
                _ if pick < 0.85 => COMMUTER,
                _ => TAXI,
            },
        }
    }
}

/// The personality of one agent (pivot plan §5.3), fixed at spawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Traits {
    /// Desired speed as a factor of the limit.
    pub speed_factor: f64,
    /// Seconds of waiting that fill the frustration.
    pub patience_s: f64,
    /// Probability to obey a rule that costs time.
    pub compliance: f64,
    /// Probability that a check is made.
    pub attention: f64,
    /// Delay between what happens and what the agent has seen of it.
    pub reaction_s: f64,
    /// 0 keeps the full safety gaps, 1 almost none.
    pub risk: f64,
    /// Keeps the junction clear when the exit is full.
    pub courtesy: f64,
}

impl Traits {
    /// Seven draws, always in this order.
    pub fn draw(archetype: &Archetype, rng: &mut SimRng) -> Self {
        let mut pick = |range: (f32, f32)| f64::from(rng.randf_range(range.0, range.1));
        Self {
            speed_factor: pick(archetype.speed_factor),
            patience_s: pick(archetype.patience_s),
            compliance: pick(archetype.compliance),
            attention: pick(archetype.attention),
            reaction_s: pick(archetype.reaction_s),
            risk: pick(archetype.risk),
            courtesy: pick(archetype.courtesy),
        }
    }
}

/// Length and width of the body, metres.
pub fn body_size(mode: Mode) -> (f64, f64) {
    match mode {
        Mode::Car => (4.3, 1.8),
        Mode::Van => (5.6, 2.0),
        Mode::Bus => (12.0, 2.5),
        Mode::Bicycle => (1.8, 0.6),
        Mode::Pedestrian => (0.5, 0.5),
    }
}

/// The class a mode travels as; `None` for the modes Phase 2 does not simulate yet.
pub fn vehicle_class(mode: Mode) -> Option<Access> {
    match mode {
        Mode::Car => Some(Access::CAR),
        Mode::Van => Some(Access::DELIVERY),
        Mode::Bus => Some(Access::BUS),
        Mode::Bicycle | Mode::Pedestrian => None,
    }
}

/// What an agent decided for the junction movement ahead of it. The draws are made once,
/// when the movement comes up, so a personality acts the same way through one junction.
#[derive(Clone, Debug, PartialEq)]
pub struct Approach {
    pub movement: StreetLaneId,
    /// Route distances of the stop line and of the end of the movement.
    pub line_d: f64,
    pub end_d: f64,
    pub obeys_signal: bool,
    /// Gives way as the rules ask.
    pub yields: bool,
    /// Makes a full stop at a stop sign.
    pub full_stop: bool,
    pub keeps_box: bool,
    /// The approach segments this agent did not look at.
    pub unseen: Vec<i32>,
    pub stopped_at_sign: bool,
    /// The last decision at the line.
    pub go: bool,
    /// The front has passed the stop line.
    pub committed: bool,
}

#[derive(Clone, Debug)]
pub struct Agent {
    pub id: AgentId,
    pub mode: Mode,
    pub archetype: &'static str,
    pub traits: Traits,
    pub(super) rng: SimRng,
    /// Index into the run's routes.
    pub route: usize,
    /// Distance of the front bumper along the route.
    pub d: f64,
    /// Metres per second.
    pub speed: f64,
    pub accel: f64,
    /// The hardest braking this vehicle does on a dry road, m/s² (`drive::draw_max_brake`).
    pub max_brake: f64,
    pub length: f64,
    pub width: f64,
    /// Middle of the body and unit heading; `prev_*` is the pose one tick before (for
    /// swept contact tests and for the views to interpolate).
    pub pos: Vec2,
    pub dir: Vec2,
    pub prev_pos: Vec2,
    pub prev_dir: Vec2,
    pub spawn_tick: u64,
    /// The trip time with nobody else on the street.
    pub free_time_s: f64,
    /// 0..1: grows while stopped, eases slowly while moving.
    pub frustration: f64,
    /// Ticks stopped in a row.
    pub wait_ticks: u32,
    pub approach: Option<Approach>,
    /// Looking away until this tick.
    pub distracted_until: u64,
    /// After a contact: stays in place until this tick, then is taken away.
    pub crashed_until: Option<u64>,
    /// The conflicting movements whose zone the body is in.
    pub(super) in_zones: Vec<StreetLaneId>,
    /// What this agent did wrong lately, for the cause chains.
    pub recent: Vec<(u64, CauseKind)>,
    pub last_hard_brake: Option<u64>,
    /// Held at its junction line by people on foot (see `Seen::held_by_walkers`), and
    /// the last tick it was.
    pub held_by_walkers: bool,
    pub last_held: Option<u64>,
    /// The zebra coming up on its route, and how it means to behave there.
    pub crossing: Option<super::yielding::CrossingApproach>,
    /// An electric car: almost silent at low speed (`perceive::sound_level`). Derived
    /// from the run seed and the id, not drawn, so no stream moves.
    pub electric: bool,
    /// The hash seed of its glances (`perceive::gaze_seed`).
    pub gaze_seed: u64,
    /// What it was doing at its last decision, which sets its gaze.
    pub gaze_task: super::perceive::DriverTask,
}

impl Agent {
    pub fn crashed(&self) -> bool {
        self.crashed_until.is_some()
    }

    pub fn velocity(&self) -> Vec2 {
        self.dir * self.speed as f32
    }
}

/// True with probability `p`. Always one draw, so the stream does not depend on `p`; a
/// `p` of 1 is always true and a `p` of 0 never.
pub fn chance(rng: &mut SimRng, p: f64) -> bool {
    let draw = f64::from(rng.randf());
    p >= 1.0 || (p > 0.0 && draw < p)
}
