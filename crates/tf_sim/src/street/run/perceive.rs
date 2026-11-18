//! Perception (pivot plan §5.1 "Perceive", §5.7 additions 2, 3 and 7, and the paragraph
//! "Field of view"). Four rules, all pure functions of numbers so a test can set them up
//! without a run:
//!
//! - **Gaze and field of view.** An agent looks at one direction at a time. Its task sets
//!   a table of places to look at (`Focus`, with weights) and the gaze moves from one to
//!   another in glances of about `GLANCE_TICKS`. A thing is noticed with a chance and a
//!   delay that depend on the angle between the gaze and the thing (`Zone`): near the
//!   centre almost sure and at once, in the periphery (up to 90° each side) less often and
//!   later, behind never. The useful field of view is narrower with low attention and
//!   hurry (`useful_fov`).
//! - **Blind spots.** The pillars of a car hide a narrow sector at each front corner; a
//!   van or a bus has a large sector on its right that the mirrors do not cover. A
//!   shoulder check (a glance to the right, `Focus::Shoulder`) shows what is in it.
//! - **Surprise.** The reaction time of the agent is for an expected event (it looked, or
//!   scanned that place). A surprise costs `SURPRISE_EXTRA_S` more.
//! - **Hearing.** Each vehicle makes a sound that grows with its speed (`sound_level`);
//!   the street has a background noise that grows with the traffic (`background_noise`);
//!   an agent hears what is above the background by a margin, from any direction.
//!
//! Nothing here draws from a random stream: the choice of a glance and the chance to notice
//! come from a hash of ids and the tick (`roll`), so a run stays a pure function of
//! `level + design + seed` and adding perception does not shift any agent's stream. No
//! trigonometry: angles are cosines of fixed angles, compared with dot products.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::mix;
use crate::street::level::Mode;

// ---------------------------------------------------------------- field of view

/// Cosines of the edges of the zones of the visual field, from the gaze (fixed angles:
/// 10°, 30°, 60°, 90°). Start values: central vision is sharp to about 10°, the useful
/// field to about 30° and the periphery sees movement up to about 90°.
pub const COS_CENTRE: f32 = 0.9848;
pub const COS_NEAR: f32 = 0.8660;
pub const COS_MID: f32 = 0.5;
pub const COS_FAR: f32 = 0.0;

/// Where a thing is, from the gaze.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Zone {
    Centre,
    Near,
    Mid,
    Far,
    /// More than 90° from the gaze: not seen.
    Behind,
}

/// The zone of the direction `to` (any length) for a gaze `gaze` (unit). A zero `to` is
/// at the centre.
pub fn zone(gaze: Vec2, to: Vec2) -> Zone {
    let length = to.length();
    if length < 1e-4 {
        return Zone::Centre;
    }
    let cos = gaze.dot(to) / length;
    if cos >= COS_CENTRE {
        Zone::Centre
    } else if cos >= COS_NEAR {
        Zone::Near
    } else if cos >= COS_MID {
        Zone::Mid
    } else if cos > COS_FAR {
        Zone::Far
    } else {
        Zone::Behind
    }
}

/// The chance that a glance notices a thing in each zone for a full field of view, and the
/// extra delay in seconds before it is (start values from the plan's "useful field of
/// view": near the centre almost sure and at once, far out less likely and later).
const ZONE_CHANCE: [f64; 5] = [1.0, 0.97, 0.85, 0.45, 0.0];
const ZONE_DELAY_S: [f64; 5] = [0.0, 0.05, 0.2, 0.5, 0.0];

/// The share of the visual field that is useful, 0.5 to 1: low attention and hurry
/// narrow it (start values; age and workload come with the archetypes of Phase 8).
pub fn useful_fov(attention: f64, risk: f64) -> f64 {
    ((0.55 + 0.45 * attention) * (1.0 - 0.1 * risk)).clamp(0.5, 1.0)
}

/// What one glance makes of a thing in `zone` for a field of view `fov`: the chance to
/// notice it and the extra delay. A narrower field loses the periphery first.
pub fn look_in(zone: Zone, fov: f64) -> (f64, f64) {
    let i = zone as usize;
    let penalty = 1.0 / fov.clamp(0.5, 1.0);
    let chance = (1.0 - (1.0 - ZONE_CHANCE[i]) * penalty).clamp(0.0, 1.0);
    (chance, ZONE_DELAY_S[i] * penalty)
}

// ---------------------------------------------------------------- surprise

/// The reaction time of a person to an expected event and to a surprise, seconds: the
/// plan's values (about 0.7 s and about 1.5 s; Green 2000, "How long does it take to
/// stop?", Transportation Human Factors 2(3)).
pub const EXPECTED_REACTION_S: f64 = 0.7;
pub const SURPRISE_REACTION_S: f64 = 1.5;
/// What a surprise adds to the reaction time of an agent (the trait is the expected one).
pub const SURPRISE_EXTRA_S: f64 = SURPRISE_REACTION_S - EXPECTED_REACTION_S;

/// The reaction time of an agent of trait `base_s` to an event that is, or is not, a
/// surprise.
pub fn reaction_s(base_s: f64, surprise: bool) -> f64 {
    if surprise {
        base_s + SURPRISE_EXTRA_S
    } else {
        base_s
    }
}

// ---------------------------------------------------------------- gaze

/// What an agent looks at in a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Focus {
    /// Along its way.
    Ahead,
    /// The car in front of it (a queue).
    CarAhead,
    /// The side of the road, a little to the right.
    Roadside,
    /// A kerb of the zebra coming up.
    Kerb,
    /// The oncoming cars (a left turn).
    Oncoming,
    /// The traffic that comes from its right: priority to the right (R415-6).
    CrossRight,
    CrossLeft,
    /// Where the turn leads: the exit and its zebra.
    Exit,
    /// A glance over the right shoulder, to the blind spot.
    Shoulder,
    /// Left and right of a walker at a kerb or on the road: toward the traffic.
    Left,
    Right,
}

impl Focus {
    /// Words for the cause chain: what the agent was looking at.
    pub fn words(self) -> &'static str {
        match self {
            Focus::Ahead => "looked ahead",
            Focus::CarAhead => "looked at the car in front",
            Focus::Roadside => "looked at the roadside",
            Focus::Kerb => "looked at the kerbs",
            Focus::Oncoming => "looked at the oncoming cars for a gap",
            Focus::CrossRight => "looked right for priority",
            Focus::CrossLeft => "looked left for the traffic",
            Focus::Exit => "looked at the exit",
            Focus::Shoulder => "looked over the shoulder",
            Focus::Left => "looked left",
            Focus::Right => "looked right",
        }
    }
}

/// What a driver is doing, which sets where it looks (plan: "the task sets the gaze").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DriverTask {
    #[default]
    Cruise,
    /// In a queue: at the car ahead.
    Queue,
    /// Coming up to a zebra: a careful driver scans its kerbs.
    ZebraScan,
    /// A left turn: the oncoming cars and the gap, then the exit.
    LeftTurn,
    /// A left turn, with its exit zebra close.
    LeftTurnExit,
    /// A right turn: priority from the right first, then the left, and the exit with
    /// its zebra later.
    RightTurn,
    RightTurnExit,
    /// Going straight on through a junction: the cross traffic from the right (priority to
    /// the right) and from the left, and the way ahead.
    Junction,
}

/// What a walker is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WalkerTask {
    /// At a kerb: left and right, a short look ahead.
    Kerb,
    /// On the road: glances toward the traffic.
    Road,
}

/// The table of places to look at: (focus, weight). Weights add up to 1. Start values.
pub fn driver_table(task: DriverTask) -> &'static [(Focus, f64)] {
    match task {
        DriverTask::Cruise => &[(Focus::Ahead, 0.85), (Focus::Roadside, 0.15)],
        DriverTask::Queue => &[(Focus::CarAhead, 1.0)],
        DriverTask::ZebraScan => &[(Focus::Kerb, 0.55), (Focus::Ahead, 0.45)],
        DriverTask::LeftTurn => &[
            (Focus::Oncoming, 0.55),
            (Focus::Ahead, 0.2),
            (Focus::Exit, 0.25),
        ],
        DriverTask::LeftTurnExit => &[
            (Focus::Exit, 0.5),
            (Focus::Ahead, 0.25),
            (Focus::Oncoming, 0.25),
        ],
        DriverTask::Junction => &[
            (Focus::CrossRight, 0.4),
            (Focus::CrossLeft, 0.3),
            (Focus::Ahead, 0.3),
        ],
        DriverTask::RightTurn => &[
            (Focus::CrossRight, 0.35),
            (Focus::CrossLeft, 0.25),
            (Focus::Shoulder, 0.15),
            (Focus::Ahead, 0.15),
            (Focus::Exit, 0.1),
        ],
        DriverTask::RightTurnExit => &[
            (Focus::Exit, 0.45),
            (Focus::Ahead, 0.3),
            (Focus::Shoulder, 0.15),
            (Focus::CrossRight, 0.1),
        ],
    }
}

pub fn walker_table(task: WalkerTask) -> &'static [(Focus, f64)] {
    match task {
        WalkerTask::Kerb => &[
            (Focus::Left, 0.45),
            (Focus::Right, 0.45),
            (Focus::Ahead, 0.1),
        ],
        WalkerTask::Road => &[
            (Focus::Left, 0.35),
            (Focus::Right, 0.35),
            (Focus::Ahead, 0.3),
        ],
    }
}

/// A field of view this wide belongs to a fully attentive agent (`useful_fov` at full
/// attention and no taste for risk). Such an agent that looks anywhere near a visible thing
/// within `SURE_RANGE_M` takes it in whatever the zone (a glance to the side still has the
/// way ahead in the corner of the eye): a careful person does not miss a car
/// or a walker that is close and in view. Only hidden things, blind spots and what is
/// behind are missed (decision of the user, 2026-11-18).
pub const FULL_FOV: f64 = 0.999;
/// The range of that sure notice, metres: about the braking distance at 50 km/h.
pub const SURE_RANGE_M: f32 = 40.0;

/// A glance lasts this many ticks (start value: 0.4 s, inside the 0.3 to 0.5 s of the
/// fixations of drivers).
pub const GLANCE_TICKS: u64 = 8;
/// A thing noticed in one of the last `MEMORY_GLANCES` glances is still known (a person
/// who has seen a car does not forget it when the eyes move on; 4 × 0.4 s).
pub const MEMORY_GLANCES: u64 = 4;

/// A number in 0..1 from a hash of the ids, the same on every machine.
pub fn roll(seed: u64, a: u64, b: u64) -> f64 {
    (mix(mix(seed, a), b) >> 11) as f64 / (1u64 << 53) as f64
}

/// The glance number of an agent at `tick`: each agent's glances start at its own tick.
pub fn glance_index(seed: u64, tick: u64) -> u64 {
    (tick + mix(seed, GLANCE_STREAM) % GLANCE_TICKS) / GLANCE_TICKS
}
const GLANCE_STREAM: u64 = 0x61_7a_65;

/// What the agent looks at in glance `index`: the entry of `table` the roll falls in.
pub fn focus_of(table: &[(Focus, f64)], seed: u64, index: u64) -> Focus {
    let r = roll(seed, index, FOCUS_STREAM);
    let mut up_to = 0.0;
    for (focus, weight) in table {
        up_to += weight;
        if r < up_to {
            return *focus;
        }
    }
    table.last().expect("a table has entries").0
}
const FOCUS_STREAM: u64 = 0x66_6f_63;

/// Fixed turns of a heading (cosine, sine of 25°, 30°, 55°, 100°): a gaze that is that
/// much to the right of it.
const TURN_25: (f32, f32) = (0.9063, 0.4226);
const TURN_30: (f32, f32) = (0.8660, 0.5);
const TURN_55: (f32, f32) = (0.5736, 0.8192);
const TURN_100: (f32, f32) = (-0.1736, 0.9848);

/// The right-hand side of a heading. The street's y axis points down, so the right
/// hand of a heading is its perpendicular.
pub fn right_of(dir: Vec2) -> Vec2 {
    dir.perp()
}

/// `dir` turned toward its right by the fixed angle (a negative sign: toward the left).
fn turned(dir: Vec2, (cos, sin): (f32, f32), toward_right: bool) -> Vec2 {
    let side = if toward_right {
        right_of(dir)
    } else {
        -right_of(dir)
    };
    dir * cos + side * sin
}

/// The gaze for `focus` of something that heads `dir`; `place` is the direction to the
/// place of an `Exit` or a `Kerb` focus (the caller knows the geometry), and `parity` (0
/// or 1) picks a side for the roadside glance. A walker's `Left` and `Right` are those of
/// its own heading; `Ahead` is its way across.
pub fn gaze_vector(focus: Focus, dir: Vec2, place: Vec2, parity: u64) -> Vec2 {
    match focus {
        Focus::Ahead | Focus::CarAhead => dir,
        Focus::Roadside => turned(dir, TURN_30, parity == 0),
        Focus::Oncoming => turned(dir, TURN_25, false),
        Focus::CrossRight => turned(dir, TURN_55, true),
        Focus::CrossLeft => turned(dir, TURN_55, false),
        Focus::Shoulder => turned(dir, TURN_100, true),
        Focus::Left => -right_of(dir),
        Focus::Right => right_of(dir),
        Focus::Exit | Focus::Kerb => {
            let place = place.normalize_or_zero();
            if place == Vec2::ZERO { dir } else { place }
        }
    }
}

// ---------------------------------------------------------------- blind spots

/// Is the thing in direction `to` from the eye of a vehicle of `mode` heading `dir` in a
/// blind spot? Pillars of a car (the cosine band of 35° to 42° from the heading, either
/// side: about 7°, so a thing hides in it only while it stays there) and, for a van or a
/// bus, the sector 60° to 120° (a bus: 45° to 135°) on the right of the heading, which
/// the mirrors do not cover. Start values.
pub fn in_blind_spot(mode: Mode, dir: Vec2, to: Vec2) -> bool {
    let length = to.length();
    if length < 0.5 {
        return false;
    }
    let to = to / length;
    let forward = dir.dot(to);
    let right = right_of(dir).dot(to);
    match mode {
        Mode::Car => (0.743..=0.819).contains(&forward),
        Mode::Van => right > 0.0 && (-0.5..=0.5).contains(&forward),
        Mode::Bus => right > 0.0 && (-0.707..=0.707).contains(&forward),
        Mode::Bicycle | Mode::Pedestrian => false,
    }
}

// ---------------------------------------------------------------- noticing

/// One agent looking: what it needs to know whether it notices a thing.
pub struct Looker<'a> {
    pub mode: Mode,
    /// Where its eyes are and its heading.
    pub eye: Vec2,
    pub dir: Vec2,
    pub table: &'a [(Focus, f64)],
    /// The hash seed of this agent's glances.
    pub seed: u64,
    pub fov: f64,
    /// A walker at a kerb looks left and right in turn, never twice the same side in a
    /// row: the road check people are taught. Others pick from the table.
    pub alternate: bool,
}

/// What a look made of a thing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noticed {
    /// Seconds after it was in view before it is noticed.
    pub delay_s: f64,
    pub focus: Focus,
    pub zone: Zone,
}

impl Looker<'_> {
    /// The gaze in glance `index`; `place` is the direction to the thing the focus is on
    /// when the focus has a place (`Exit`, `Kerb`).
    pub fn gaze(&self, index: u64, place: impl Fn(Focus, u64) -> Vec2) -> (Vec2, Focus) {
        let focus = if self.alternate {
            if (index + self.seed).is_multiple_of(2) {
                Focus::Left
            } else {
                Focus::Right
            }
        } else {
            focus_of(self.table, self.seed, index)
        };
        (
            gaze_vector(focus, self.dir, place(focus, index), index % 2),
            focus,
        )
    }

    /// Does the agent notice the thing at `target` (id `who`) at `tick`? In each of the last
    /// `MEMORY_GLANCES` glances the thing is in a zone of the gaze; it is noticed in a glance
    /// with the chance of that zone (a hash of the ids, so a person does not flicker from
    /// tick to tick), unless it is in a blind spot that the glance is not looking into.
    /// `Some` with the shortest delay among the glances that noticed it.
    pub fn notices(
        &self,
        who: u64,
        target: Vec2,
        tick: u64,
        place: impl Fn(Focus, u64) -> Vec2 + Copy,
    ) -> Option<Noticed> {
        let to = target - self.eye;
        let now = glance_index(self.seed, tick);
        let blind_spot = in_blind_spot(self.mode, self.dir, to);
        let mut best: Option<Noticed> = None;
        for index in (now.saturating_sub(MEMORY_GLANCES - 1)..=now).rev() {
            let (gaze, focus) = self.gaze(index, place);
            let zone = zone(gaze, to);
            // A pillar is passed by a small head movement: it hides only in every other
            // glance. The large blind sector of a van or a bus needs a shoulder check.
            let blind = blind_spot && (self.mode != Mode::Car || index % 2 == 0);
            if blind && !(focus == Focus::Shoulder && zone <= Zone::Near) {
                continue;
            }
            let (mut chance, delay_s) = look_in(zone, self.fov);
            // A shoulder check into a blind spot shows what is in it.
            let checked = blind_spot && focus == Focus::Shoulder;
            if checked
                || (self.fov >= FULL_FOV
                    && (zone != Zone::Behind || self.dir.dot(to) > 0.0)
                    && to.length() <= SURE_RANGE_M)
            {
                chance = 1.0;
            }
            if chance > 0.0
                && roll(self.seed, who, index) < chance
                && best.is_none_or(|b| delay_s < b.delay_s)
            {
                best = Some(Noticed {
                    delay_s,
                    focus,
                    zone,
                });
            }
        }
        best
    }
}

// ---------------------------------------------------------------- hearing

/// The sound of a vehicle, in units of loudness at 1 m (start values, for Phase 8 to
/// calibrate): a base for the engine and a part that grows with the speed (m/s) from the
/// tyres and the air. A diesel van is loud; an electric car is quiet at low speed (its
/// engine makes almost no noise, and the tyres only above about 20 km/h); a bicycle and a
/// scooter are almost silent (a known danger for people on foot).
pub fn sound_level(mode: Mode, speed: f64, electric: bool) -> f64 {
    let (base, per_mps) = match mode {
        Mode::Car if electric => (0.6, 0.35),
        Mode::Car => (2.0, 0.45),
        Mode::Van => (3.0, 0.5),
        Mode::Bus => (4.0, 0.5),
        Mode::Bicycle => (0.15, 0.05),
        Mode::Pedestrian => (0.0, 0.0),
    };
    base + per_mps * speed
}

/// The noise of the street without any one vehicle: a floor, and a part that grows with
/// the number of vehicles moving (start values).
pub fn background_noise(moving_vehicles: usize) -> f64 {
    0.15 + 0.01 * moving_vehicles as f64
}

/// A vehicle is heard when its sound at the listener (it falls with the distance: loudness
/// divided by metres, never louder than at 1 m) is above the background by this factor
/// (start value).
pub const HEARING_MARGIN: f64 = 1.3;

/// Does a listener at `distance` metres hear a source of `level`, whatever the direction?
pub fn hears(level: f64, distance: f64, background: f64) -> bool {
    level / distance.max(1.0) > background * HEARING_MARGIN
}

/// `hears`, for a listener who may not hear the street at all (headphones, a call).
pub fn listener_hears(deaf: bool, level: f64, distance: f64, background: f64) -> bool {
    !deaf && hears(level, distance, background)
}

/// The share of cars that are electric (start value: the Paris fleet of 2026 is a
/// minority, and the share is for Phase 8 to calibrate).
pub const ELECTRIC_SHARE: f64 = 0.2;
/// The share of walkers who do not hear the street (headphones, a phone call, hearing
/// loss). Start value; the walker named "hurried walker" is on the phone more often.
pub const DEAF_SHARE: f64 = 0.12;
pub const DEAF_SHARE_HURRIED: f64 = 0.3;

const ELECTRIC_STREAM: u64 = 0x65_6c_65;
const DEAF_STREAM: u64 = 0x64_65_66;
const GAZE_STREAM: u64 = 0x67_61_7a;

/// Whether a car is electric: from the run seed and its id, so that no stream shifts.
pub fn is_electric(run_seed: u64, id: i32, mode: Mode) -> bool {
    mode == Mode::Car && roll(run_seed, id as u64, ELECTRIC_STREAM) < ELECTRIC_SHARE
}

/// Whether a walker does not hear the street.
pub fn is_deaf(run_seed: u64, id: i32, hurried: bool) -> bool {
    let share = if hurried {
        DEAF_SHARE_HURRIED
    } else {
        DEAF_SHARE
    };
    roll(run_seed, id as u64, DEAF_STREAM) < share
}

/// The seed of the glances of an agent.
pub fn gaze_seed(run_seed: u64, id: i32) -> u64 {
    mix(mix(run_seed, id as u64), GAZE_STREAM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_follow_the_angle() {
        let gaze = Vec2::X;
        assert_eq!(zone(gaze, Vec2::new(10.0, 0.5)), Zone::Centre);
        assert_eq!(zone(gaze, Vec2::new(10.0, 4.0)), Zone::Near);
        assert_eq!(zone(gaze, Vec2::new(10.0, 10.0)), Zone::Mid);
        assert_eq!(zone(gaze, Vec2::new(2.0, 10.0)), Zone::Far);
        assert_eq!(zone(gaze, Vec2::new(-1.0, 1.0)), Zone::Behind);
    }

    #[test]
    fn surprise_costs_the_difference() {
        assert!((reaction_s(0.7, true) - 1.5).abs() < 1e-9);
        assert_eq!(reaction_s(0.7, false), 0.7);
    }
}
