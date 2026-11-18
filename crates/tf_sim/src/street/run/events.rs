//! What a run reports (pivot plan §5.5, §5.6): the event log is the result of a run. The
//! score, the report, the replay and the replay hashes are all read from it. Numbers in
//! events are rounded, so a log reads well and its hash does not hang on the last bit of
//! a float.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::perceive::Focus;
use super::sight::Hider;
use crate::rng::SimRng;
use crate::street::ids::AgentId;
use crate::street::level::Mode;

/// What an agent did that led to a conflict or a contact.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum CauseKind {
    RanRed,
    RolledStop,
    /// Had to give way to `to` and did not.
    DidNotYield {
        to: AgentId,
    },
    /// Did not look at the approach `at` came from.
    DidNotLook {
        at: AgentId,
    },
    /// Saw `to` and went into a gap that was too short.
    GapTooShort {
        to: AgentId,
    },
    /// Entered the junction with no room to leave it.
    BlockedBox,
    /// Was looking away (a phone).
    Distracted,
    Speeding {
        kmh: f64,
        limit_kmh: f64,
    },
    /// Braked as hard as the vehicle can and still could not stop.
    TooFastToStop,
    /// Drove onto a zebra where `to` was crossing or waiting to cross.
    DidNotYieldToWalker {
        to: AgentId,
    },
    /// Could not see `walker`: the sight line was cut by `by`.
    HiddenWalker {
        walker: AgentId,
        by: Hider,
    },
    /// Came up to a zebra with a hidden kerb faster than it could stop at.
    TooFastForSight {
        kmh: f64,
        safe_kmh: f64,
    },
    /// A walker crossed the carriageway out of a crossing.
    CrossedOutOfCrossing,
    /// A walker stepped off on red (or too late for its crossing).
    WalkedOnRed,
    /// A walker stepped off in a gap a careful walker would not have taken, seconds.
    AcceptedShortGap {
        gap_s: f64,
    },
    /// A walker on the road had to hurry to clear a vehicle that did not slow down.
    HurriedOnRoad,
    /// A walker already on the road kept its pace when a car came, expecting the driver
    /// to stop (it has priority once engaged, R415-11 at a crossing).
    KeptPaceOnRoad,
    /// Did not look at `at`: the gaze was on something else (a left turn on the oncoming
    /// cars, not on the exit zebra) or `at` was outside the useful field of view.
    LookedElsewhere {
        at: AgentId,
        focus: Focus,
    },
    /// `walker` was in a blind spot of the vehicle (a pillar, or the right side of a van or
    /// a bus) and no shoulder check was made.
    InBlindSpot {
        walker: AgentId,
    },
    /// A surprise (the thing came out of a hiding place or was not looked at): the reaction
    /// took this long, seconds.
    Surprise {
        reaction_s: f64,
    },
    /// Could not see `vehicle`: the sight line was cut by `by`.
    HiddenVehicle {
        vehicle: AgentId,
        by: Hider,
    },
    /// Did not see and did not hear `vehicle`: the street noise hid it (a quiet car), or the
    /// person does not hear the street.
    NotHeard {
        vehicle: AgentId,
        kmh: f64,
        electric: bool,
        deaf: bool,
    },
    /// Passed `to` with less side room than the rule asks (1 m in town, R414-4) or than
    /// a careful rider keeps.
    PassedTooClose {
        to: AgentId,
    },
    /// A cyclist rode on the sidewalk because the street felt hostile.
    RodeOnSidewalk,
    /// Went past the stop line into the junction although its own last decision at the
    /// line was "stop": it could no longer stop without braking hard (a car that stopped
    /// with its front on the line counts as committed). Whoever it meets inside is not the
    /// only one at fault.
    EnteredWithoutGap,
    /// Went first as the leader of a ring of waiting vehicles that give way to each other, in
    /// front of `other`, which had priority over it: two of a ring may leave together when each
    /// reads a view as old as its reaction time.
    LedRingAgainst {
        other: AgentId,
    },
}

impl CauseKind {
    /// The other agent this cause is about, if it names one.
    pub fn about(&self) -> Option<AgentId> {
        match *self {
            CauseKind::DidNotYield { to }
            | CauseKind::GapTooShort { to }
            | CauseKind::DidNotYieldToWalker { to }
            | CauseKind::PassedTooClose { to } => Some(to),
            CauseKind::LedRingAgainst { other } => Some(other),
            CauseKind::HiddenWalker { walker, .. } => Some(walker),
            CauseKind::LookedElsewhere { at, .. } => Some(at),
            CauseKind::InBlindSpot { walker } => Some(walker),
            CauseKind::HiddenVehicle { vehicle, .. } | CauseKind::NotHeard { vehicle, .. } => {
                Some(vehicle)
            }
            CauseKind::DidNotLook { at } => Some(at),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cause {
    pub agent: AgentId,
    pub what: CauseKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConflictKind {
    /// Post-encroachment time: `b` entered the place where the paths cross this soon
    /// after `a` left it.
    Pet,
    /// `a` had to brake hard for `b`.
    HardBrake,
    /// A motor vehicle passed a rider with little side room at speed (`a` is the driver,
    /// `b` the rider); the measure is the room in metres.
    ClosePass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Grade {
    Light,
    Serious,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Outcome {
    NoInjury,
    LightInjury,
    SeriousInjury,
    Death,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EndReason {
    /// The day is over and the street is empty (or the grace time ran out).
    Completed,
    /// A death stops the run (pivot plan §6).
    Death,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StreetEvent {
    SliceStarted {
        index: usize,
        name: String,
    },
    Spawned {
        agent: AgentId,
        mode: Mode,
        archetype: String,
        from: String,
        to: String,
    },
    TripEnded {
        agent: AgentId,
        time_s: f64,
        /// Against the trip on an empty street.
        delay_s: f64,
        /// The delay weighted by the agent's patience (pivot plan §5.6).
        annoyance: f64,
    },
    /// Stuck for too long: the agent leaves the level without finishing its trip.
    GaveUp {
        agent: AgentId,
        waited_s: f64,
    },
    /// Still on the street, or still waiting to enter, when the run ended: the person did
    /// not get through. The score counts it like a give-up (a jam must cost points, not
    /// hide its slowest people). `agent` is `None` for one who never entered (a wish of the
    /// backlog); `delay_s` is the delay so far (time since it should have entered or
    /// since it entered, less the free trip time).
    Unfinished {
        agent: Option<AgentId>,
        mode: Mode,
        delay_s: f64,
        entered: bool,
    },
    Conflict {
        kind: ConflictKind,
        grade: Grade,
        a: AgentId,
        b: AgentId,
        position: Vec2,
        /// Seconds for `Pet`, metres per second squared for `HardBrake`.
        measure: f64,
        causes: Vec<Cause>,
    },
    Contact {
        a: AgentId,
        b: AgentId,
        position: Vec2,
        impact_kmh: f64,
        outcome: Outcome,
        causes: Vec<Cause>,
    },
    /// A pedestrian came in (pivot plan Phase 3). `group` is the first member's id, the
    /// walker's own when it walks alone.
    WalkerSpawned {
        agent: AgentId,
        archetype: String,
        from: String,
        to: String,
        group: AgentId,
        group_size: u32,
    },
    /// A pedestrian reached its destination.
    WalkEnded {
        agent: AgentId,
        time_s: f64,
        /// Against the walk on an empty sidewalk.
        delay_s: f64,
        /// Time spent in a dense crowd (`crowd::DENSE_CROWD`).
        crowded_s: f64,
        bumps: u32,
        /// Time spent waiting at kerbs (also in the delay).
        kerb_wait_s: f64,
        /// The extra walk the rule way asked for, seconds (0 when it crossed where it
        /// wanted to).
        detour_s: f64,
        /// Delay, crowding and bumps weighted by the walker's patience (pivot plan §5.6).
        annoyance: f64,
    },
    /// A pedestrian steps off a kerb onto a carriageway: on crossing `crossing` (an id
    /// in `Street::crossings`) or out of one (`None`), on a red light or not, with the
    /// soonest a vehicle would reach it as it saw it (`gap_s`, `None`: nobody coming),
    /// after waiting `waited_s`. Stage C reads it for the cause chain.
    CrossingStarted {
        agent: AgentId,
        segment: i32,
        crossing: Option<i32>,
        position: Vec2,
        on_red: bool,
        gap_s: Option<f64>,
        waited_s: f64,
    },
    /// Two pedestrians bumped into each other (pivot plan §5.5: annoyance, no injury).
    Bump {
        a: AgentId,
        b: AgentId,
        position: Vec2,
    },
    /// A vehicle stopped by a contact is taken away.
    Cleared {
        agent: AgentId,
    },
    RunEnded {
        reason: EndReason,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoggedEvent {
    pub tick: u64,
    pub event: StreetEvent,
}

/// Rounds to `1 / per_unit` (100 = two decimals).
pub fn rounded(value: f64, per_unit: f64) -> f64 {
    (value * per_unit).round() / per_unit
}

pub fn rounded_point(point: Vec2) -> Vec2 {
    Vec2::new(
        (point.x * 100.0).round() / 100.0,
        (point.y * 100.0).round() / 100.0,
    )
}

/// One row of a severity table: at `kmh`, the probabilities of at least a light injury,
/// at least a serious one, and of death.
pub type SeverityRow = (f64, f64, f64, f64);

/// Two motor vehicles, by closing speed. Anchors from Lubbe, Jeppsson, Sternlund &
/// Morando 2024 (Accident Analysis & Prevention 202:107586; German in-depth data,
/// head-on car to car): 10 % risk of a moderate injury or worse (MAIS2+) at 25 km/h and
/// 50 % at 95 km/h; 10 % risk of a serious injury or worse (MAIS3+) at 75 km/h. The other
/// points, and death as a quarter of the serious risk, are our own start values: no
/// table by speed was found for occupants at city speeds. To be calibrated in Phase 8.
pub const VEHICLE_SEVERITY: &[SeverityRow] = &[
    (0.0, 0.0, 0.0, 0.0),
    (10.0, 0.01, 0.0, 0.0),
    (25.0, 0.10, 0.005, 0.001),
    (50.0, 0.22, 0.03, 0.007),
    (75.0, 0.37, 0.10, 0.025),
    (95.0, 0.50, 0.20, 0.05),
    (130.0, 0.75, 0.45, 0.15),
];

/// A person on foot hit by the front of a car, by impact speed (used from Phase 3).
/// Death: Rosén & Sander 2009, "Pedestrian fatality risk as a function of car impact
/// speed" (Accident Analysis & Prevention 41(3)), P = 1 / (1 + exp(6.9 - 0.090 v)),
/// tabulated here so the sim needs no `exp`. Serious: Tefft 2011, "Impact Speed and a
/// Pedestrian's Risk of Severe Injury or Death" (AAA Foundation), AIS 4 or worse,
/// interpolated from its table. "At least light" is our own start value.
pub const PEDESTRIAN_SEVERITY: &[SeverityRow] = &[
    (0.0, 0.0, 0.0, 0.0),
    (10.0, 0.50, 0.03, 0.002),
    (20.0, 0.80, 0.07, 0.006),
    (30.0, 0.95, 0.16, 0.015),
    (40.0, 1.0, 0.31, 0.035),
    (50.0, 1.0, 0.50, 0.083),
    (60.0, 1.0, 0.70, 0.182),
    (70.0, 1.0, 0.85, 0.354),
    (80.0, 1.0, 0.93, 0.574),
    (100.0, 1.0, 1.0, 0.89),
];

/// The probabilities (light or worse, serious or worse, death) at `kmh`, between the
/// rows of `table`.
pub fn severity_at(table: &[SeverityRow], kmh: f64) -> (f64, f64, f64) {
    let last = table[table.len() - 1];
    if kmh >= last.0 {
        return (last.1, last.2, last.3);
    }
    for pair in table.windows(2) {
        let (low, high) = (pair[0], pair[1]);
        if kmh <= high.0 {
            let t = ((kmh - low.0) / (high.0 - low.0)).clamp(0.0, 1.0);
            let mix = |a: f64, b: f64| a + (b - a) * t;
            return (mix(low.1, high.1), mix(low.2, high.2), mix(low.3, high.3));
        }
    }
    (last.1, last.2, last.3)
}

/// The outcome of a contact: one draw against the table (pivot plan §5.5).
pub fn draw_outcome(table: &[SeverityRow], kmh: f64, rng: &mut SimRng) -> Outcome {
    let (light, serious, death) = severity_at(table, kmh);
    let draw = f64::from(rng.randf());
    if draw < death {
        Outcome::Death
    } else if draw < serious {
        Outcome::SeriousInjury
    } else if draw < light {
        Outcome::LightInjury
    } else {
        Outcome::NoInjury
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_grows_with_speed() {
        for table in [VEHICLE_SEVERITY, PEDESTRIAN_SEVERITY] {
            let mut before = (0.0, 0.0, 0.0);
            for kmh in 0..140 {
                let now = severity_at(table, f64::from(kmh));
                assert!(now.0 >= before.0 && now.1 >= before.1 && now.2 >= before.2);
                assert!(now.0 >= now.1 && now.1 >= now.2, "worse is rarer at {kmh}");
                before = now;
            }
        }
        // Rosén & Sander: death is more than five times as likely at 50 km/h as at 30.
        let at_30 = severity_at(PEDESTRIAN_SEVERITY, 30.0).2;
        let at_50 = severity_at(PEDESTRIAN_SEVERITY, 50.0).2;
        assert!(at_50 > 5.0 * at_30);
    }

    #[test]
    fn a_slow_bump_hurts_nobody() {
        let mut rng = SimRng::new(3);
        for _ in 0..200 {
            assert_eq!(
                draw_outcome(VEHICLE_SEVERITY, 0.0, &mut rng),
                Outcome::NoInjury
            );
        }
    }
}
