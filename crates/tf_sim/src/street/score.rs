//! Score, verdict and report of one stage (pivot plan §6), read from the event log of a
//! run and from nothing else, so the game and the headless binary show the same thing.
//!
//! A level has star stages (`LevelFile::stages`): each one is the same level with more
//! traffic. A run plays one stage. Its score is 100 less a safety penalty (conflicts and
//! injuries) and a serenity penalty (annoyance per mode, the worst mode counting double).
//! A death, a serious injury or a broken constraint fails the stage whatever the score.
//! All the weights are start values to tune; they are named constants so a tuning is one
//! line. Stages are numbered from 1.

use indexmap::IndexMap;
use serde::Serialize;

use crate::street::design::Design;
use crate::street::ids::AgentId;
use crate::street::level::{LevelFile, Mode};
use crate::street::model::EditError;
use crate::street::run::agent::Population;
use crate::street::run::events::{
    Cause, CauseKind, ConflictKind, EndReason, Grade, LoggedEvent, Outcome, StreetEvent,
};
use crate::street::run::sight::Hider;
use crate::street::run::{RunOptions, StreetRun, TICK_RATE, is_walker};

/// Points lost for a light conflict (a near miss with a long enough margin). Start value.
pub const PENALTY_CONFLICT_LIGHT: f64 = 1.0;
/// Points lost for a serious conflict. Start value.
pub const PENALTY_CONFLICT_SERIOUS: f64 = 4.0;
/// Points lost for a contact that hurt nobody. Start value.
pub const PENALTY_CONTACT: f64 = 6.0;
/// Points lost for a light injury. Start value.
pub const PENALTY_LIGHT_INJURY: f64 = 12.0;
/// Points lost for a serious injury. The stage fails anyway; the points make the score
/// say so. Start value.
pub const PENALTY_SERIOUS_INJURY: f64 = 40.0;
/// Points lost for a death. The stage fails anyway. Start value.
pub const PENALTY_DEATH: f64 = 60.0;

/// The annoyance of an agent who gave up (pivot plan §5.6: a big annoyance). Start value.
pub const GAVE_UP_ANNOYANCE: f64 = 2.0;
/// Points lost per unit of mean annoyance (see `serenity_penalty`). Start value, set so
/// that the empty design of the school crossing scores in the 70s to 90s.
pub const SERENITY_WEIGHT: f64 = 12.0;
/// The most the serenity penalty can take. Start value.
pub const SERENITY_CAP: f64 = 50.0;

/// Side of the cells incidents are grouped by when the report names a place, metres.
pub const PLACE_CELL_M: f32 = 10.0;

/// Why a stage failed whatever its score.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum FailReason {
    Death,
    SeriousInjury,
    /// A level constraint was broken (nothing is checked in a run yet: the hook is the
    /// `constraints_broken` argument of `report_from_events`).
    Constraint(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct ScoreParts {
    /// Points lost to conflicts and injuries.
    pub safety: f64,
    /// Points lost to annoyance.
    pub serenity: f64,
    /// Bonus for green areas and rest (Phase 7; always 0 now).
    pub life: f64,
}

/// What happened to the people of one mode. Vans count with cars (`score_mode`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModeStats {
    pub mode: Mode,
    pub spawned: u32,
    pub arrived: u32,
    pub gave_up: u32,
    /// Still on the street at the end, and never entered: scored like give-ups.
    pub unfinished: u32,
    pub not_entered: u32,
    /// Mean delay of those who arrived or did not finish (the delay so far), seconds.
    pub mean_delay_s: f64,
    /// Mean annoyance of those who arrived or gave up (a give-up counts
    /// `GAVE_UP_ANNOYANCE`).
    pub mean_annoyance: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum IncidentKind {
    LightConflict,
    SeriousConflict,
    Contact { outcome: Outcome },
}

/// One event the report explains.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Incident {
    pub tick: u64,
    pub time_s: f64,
    pub kind: IncidentKind,
    pub position: glam::Vec2,
    pub agents: [AgentId; 2],
    pub causes: Vec<Cause>,
    /// One line of plain text built from the cause chain.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StageReport {
    /// From 1.
    pub stage: usize,
    pub passed: bool,
    /// 0 to 100.
    pub score: f64,
    pub pass_score: f64,
    pub parts: ScoreParts,
    pub hard_fail: Option<FailReason>,
    pub per_mode: Vec<ModeStats>,
    /// In the order they happened.
    pub incidents: Vec<Incident>,
    /// The three biggest losses of points, in words.
    pub top_reasons: Vec<String>,
    pub ended: Option<EndReason>,
}

/// The mode a person counts under in the score: vans are motor traffic like cars.
pub fn score_mode(mode: Mode) -> Mode {
    match mode {
        Mode::Van => Mode::Car,
        other => other,
    }
}

const SCORE_MODES: [Mode; 4] = [Mode::Car, Mode::Bus, Mode::Bicycle, Mode::Pedestrian];

fn mode_noun(mode: Mode) -> &'static str {
    match mode {
        Mode::Car | Mode::Van => "drivers",
        Mode::Bus => "buses",
        Mode::Bicycle => "cyclists",
        Mode::Pedestrian => "pedestrians",
    }
}

/// The options of a run of `stage` (from 1): the level's seed, a mixed population and the
/// stage's demand scale. `None` for a stage the level does not have.
pub fn stage_options(level: &LevelFile, stage: usize) -> Option<RunOptions> {
    let stage = level.stages.get(stage.checked_sub(1)?)?;
    Some(RunOptions {
        seed: level.seed,
        population: Population::Mixed,
        demand_scale: stage.demand_scale,
    })
}

/// The serenity penalty: weight × (the sum of the mode means plus the worst mean) divided
/// by (the number of modes plus one), so the worst mode counts double (pivot plan §6),
/// capped. `means` holds the mean annoyance of each mode that had people.
pub fn serenity_penalty(means: &[f64]) -> f64 {
    if means.is_empty() {
        return 0.0;
    }
    let sum: f64 = means.iter().sum();
    let worst = means.iter().copied().fold(0.0, f64::max);
    (SERENITY_WEIGHT * (sum + worst) / (means.len() as f64 + 1.0)).min(SERENITY_CAP)
}

fn rounded(value: f64, per_unit: f64) -> f64 {
    (value * per_unit).round() / per_unit
}

#[derive(Default)]
struct ModeTally {
    spawned: u32,
    arrived: u32,
    gave_up: u32,
    unfinished: u32,
    not_entered: u32,
    /// How many `delay` adds up (arrived, and not finished).
    delayed: u32,
    delay: f64,
    annoyance: f64,
}

fn label(modes: &IndexMap<AgentId, Mode>, agent: AgentId) -> String {
    let noun = match modes.get(&agent) {
        Some(Mode::Car) => "Car",
        Some(Mode::Van) => "Van",
        Some(Mode::Bus) => "Bus",
        Some(Mode::Bicycle) => "Cyclist",
        Some(Mode::Pedestrian) => "Pedestrian",
        None if is_walker(agent) => "Pedestrian",
        None => "Vehicle",
    };
    format!("{noun} {agent}")
}

fn cause_text(modes: &IndexMap<AgentId, Mode>, cause: &Cause) -> String {
    let who = label(modes, cause.agent);
    let other = |id: AgentId| label(modes, id);
    match cause.what {
        CauseKind::RanRed => format!("{who} ran a red light"),
        CauseKind::RolledStop => format!("{who} rolled through a stop sign"),
        CauseKind::DidNotYield { to } => format!("{who} did not give way to {}", other(to)),
        CauseKind::DidNotLook { at } => {
            format!("{who} did not look at the approach of {}", other(at))
        }
        CauseKind::GapTooShort { to } => {
            format!("{who} went into a gap too short for {}", other(to))
        }
        CauseKind::BlockedBox => format!("{who} entered the junction with no room to leave it"),
        CauseKind::Distracted => format!("{who} was looking away"),
        CauseKind::Speeding { kmh, limit_kmh } => {
            format!("{who} drove at {kmh:.0} km/h where the limit is {limit_kmh:.0}")
        }
        CauseKind::TooFastToStop => format!("{who} could not stop in time"),
        CauseKind::DidNotYieldToWalker { to } => {
            format!("{who} did not stop at the crossing for {}", other(to))
        }
        CauseKind::HiddenWalker { walker, by } => {
            let hider = match by {
                Hider::Obstacle(id) => format!("obstacle {id}"),
                Hider::Vehicle(id) => other(id),
            };
            format!("{who} could not see {}: hidden by {hider}", other(walker))
        }
        CauseKind::TooFastForSight { kmh, safe_kmh } => format!(
            "{who} came up to a crossing at {kmh:.0} km/h, more than the {safe_kmh:.0} km/h \
             the sight allowed"
        ),
        CauseKind::CrossedOutOfCrossing => format!("{who} crossed out of the crossing"),
        CauseKind::WalkedOnRed => format!("{who} stepped off on red"),
        CauseKind::AcceptedShortGap { gap_s } => {
            format!("{who} stepped off in a gap of {gap_s:.1} s")
        }
        CauseKind::HurriedOnRoad => format!("{who} had to hurry across to clear a vehicle"),
        CauseKind::KeptPaceOnRoad => {
            format!("{who} kept its pace on the road, expecting the car to stop")
        }
    }
}

fn causes_text(modes: &IndexMap<AgentId, Mode>, causes: &[Cause]) -> String {
    if causes.is_empty() {
        return "no one broke a rule: the design brings them too close".to_string();
    }
    causes
        .iter()
        .map(|cause| cause_text(modes, cause))
        .collect::<Vec<_>>()
        .join("; ")
}

fn place(position: glam::Vec2) -> (i32, i32) {
    (
        (position.x / PLACE_CELL_M).floor() as i32,
        (position.y / PLACE_CELL_M).floor() as i32,
    )
}

fn place_name(cell: (i32, i32)) -> String {
    format!(
        "near ({:.0}, {:.0}) m",
        cell.0 as f32 * PLACE_CELL_M,
        cell.1 as f32 * PLACE_CELL_M
    )
}

/// What one kind of incident costs, and the words for it in a group of `n`.
fn incident_cost(kind: IncidentKind) -> (f64, &'static str, &'static str) {
    match kind {
        IncidentKind::LightConflict => {
            (PENALTY_CONFLICT_LIGHT, "light conflict", "light conflicts")
        }
        IncidentKind::SeriousConflict => (
            PENALTY_CONFLICT_SERIOUS,
            "serious conflict",
            "serious conflicts",
        ),
        IncidentKind::Contact { outcome } => match outcome {
            Outcome::NoInjury => (
                PENALTY_CONTACT,
                "contact without injury",
                "contacts without injury",
            ),
            Outcome::LightInjury => (PENALTY_LIGHT_INJURY, "light injury", "light injuries"),
            Outcome::SeriousInjury => {
                (PENALTY_SERIOUS_INJURY, "serious injury", "serious injuries")
            }
            Outcome::Death => (PENALTY_DEATH, "death", "deaths"),
        },
    }
}

/// The report of a stage, from the event log. `constraints_broken` holds the messages of
/// the level constraints the design breaks (nothing checks them in a run yet).
pub fn report_from_events(
    events: &[LoggedEvent],
    ended: Option<EndReason>,
    stage: usize,
    pass_score: f64,
    constraints_broken: &[String],
) -> StageReport {
    let mut modes: IndexMap<AgentId, Mode> = IndexMap::new();
    let mut tallies: IndexMap<Mode, ModeTally> = SCORE_MODES
        .iter()
        .map(|mode| (*mode, ModeTally::default()))
        .collect();
    let mut incidents: Vec<Incident> = Vec::new();
    let mut safety = 0.0;
    let mut death = ended == Some(EndReason::Death);
    let mut serious_injury = false;

    // Spawns come before everything else an agent does, so one pass is enough.
    for logged in events {
        let tick = logged.tick;
        let time_s = rounded(tick as f64 / f64::from(TICK_RATE), 10.0);
        match &logged.event {
            StreetEvent::Spawned { agent, mode, .. } => {
                modes.insert(*agent, *mode);
                tallies.entry(score_mode(*mode)).or_default().spawned += 1;
            }
            StreetEvent::WalkerSpawned { agent, .. } => {
                modes.insert(*agent, Mode::Pedestrian);
                tallies.entry(Mode::Pedestrian).or_default().spawned += 1;
            }
            StreetEvent::TripEnded {
                agent,
                delay_s,
                annoyance,
                ..
            } => {
                if let Some(mode) = modes.get(agent) {
                    let tally = tallies.entry(score_mode(*mode)).or_default();
                    tally.arrived += 1;
                    tally.delayed += 1;
                    tally.delay += delay_s;
                    tally.annoyance += annoyance;
                }
            }
            StreetEvent::WalkEnded {
                delay_s, annoyance, ..
            } => {
                let tally = tallies.entry(Mode::Pedestrian).or_default();
                tally.arrived += 1;
                tally.delayed += 1;
                tally.delay += delay_s;
                tally.annoyance += annoyance;
            }
            StreetEvent::Unfinished {
                mode,
                delay_s,
                entered,
                ..
            } => {
                let tally = tallies.entry(score_mode(*mode)).or_default();
                if *entered {
                    tally.unfinished += 1;
                } else {
                    tally.not_entered += 1;
                }
                tally.delayed += 1;
                tally.delay += delay_s;
                tally.annoyance += GAVE_UP_ANNOYANCE;
            }
            StreetEvent::GaveUp { agent, .. } => {
                let mode = modes.get(agent).copied().unwrap_or(if is_walker(*agent) {
                    Mode::Pedestrian
                } else {
                    Mode::Car
                });
                let tally = tallies.entry(score_mode(mode)).or_default();
                tally.gave_up += 1;
                tally.annoyance += GAVE_UP_ANNOYANCE;
            }
            StreetEvent::Conflict {
                kind,
                grade,
                a,
                b,
                position,
                measure,
                causes,
            } => {
                let incident_kind = match grade {
                    Grade::Light => IncidentKind::LightConflict,
                    Grade::Serious => IncidentKind::SeriousConflict,
                };
                safety += incident_cost(incident_kind).0;
                let what = match kind {
                    ConflictKind::Pet => format!("passed {measure:.1} s after each other"),
                    ConflictKind::HardBrake => format!("hard braking at {measure:.1} m/s²"),
                };
                let severity = match grade {
                    Grade::Light => "Near miss",
                    Grade::Serious => "Serious near miss",
                };
                incidents.push(Incident {
                    tick,
                    time_s,
                    kind: incident_kind,
                    position: *position,
                    agents: [*a, *b],
                    causes: causes.clone(),
                    text: format!(
                        "{severity} between {} and {} ({what}): {}",
                        label(&modes, *a),
                        label(&modes, *b),
                        causes_text(&modes, causes)
                    ),
                });
            }
            StreetEvent::Contact {
                a,
                b,
                position,
                impact_kmh,
                outcome,
                causes,
            } => {
                let incident_kind = IncidentKind::Contact { outcome: *outcome };
                safety += incident_cost(incident_kind).0;
                match outcome {
                    Outcome::Death => death = true,
                    Outcome::SeriousInjury => serious_injury = true,
                    _ => {}
                }
                let result = match outcome {
                    Outcome::NoInjury => "no injury",
                    Outcome::LightInjury => "light injury",
                    Outcome::SeriousInjury => "serious injury",
                    Outcome::Death => "death",
                };
                let (first, second) = (label(&modes, *a), label(&modes, *b));
                let verb = if is_walker(*b) || is_walker(*a) {
                    format!("{first} hit {second}")
                } else {
                    format!("{first} and {second} collided")
                };
                incidents.push(Incident {
                    tick,
                    time_s,
                    kind: incident_kind,
                    position: *position,
                    agents: [*a, *b],
                    causes: causes.clone(),
                    text: format!(
                        "{verb} at {impact_kmh:.0} km/h ({result}): {}",
                        causes_text(&modes, causes)
                    ),
                });
            }
            _ => {}
        }
    }

    // Per mode: the modes that had people count in the serenity penalty.
    let mut per_mode = Vec::new();
    let mut means = Vec::new();
    for (mode, tally) in &tallies {
        let people = tally.arrived + tally.gave_up + tally.unfinished + tally.not_entered;
        let mean_annoyance = if people > 0 {
            tally.annoyance / f64::from(people)
        } else {
            0.0
        };
        if people > 0 {
            means.push((*mode, mean_annoyance));
        }
        per_mode.push(ModeStats {
            mode: *mode,
            spawned: tally.spawned,
            arrived: tally.arrived,
            gave_up: tally.gave_up,
            unfinished: tally.unfinished,
            not_entered: tally.not_entered,
            mean_delay_s: if tally.delayed > 0 {
                rounded(tally.delay / f64::from(tally.delayed), 10.0)
            } else {
                0.0
            },
            mean_annoyance: rounded(mean_annoyance, 1000.0),
        });
    }
    let mean_values: Vec<f64> = means.iter().map(|(_, mean)| *mean).collect();
    let serenity = serenity_penalty(&mean_values);
    let parts = ScoreParts {
        safety: rounded(safety, 100.0),
        serenity: rounded(serenity, 100.0),
        life: 0.0,
    };
    let score = rounded(
        (100.0 - parts.safety - parts.serenity + parts.life).clamp(0.0, 100.0),
        100.0,
    );

    let hard_fail = if death {
        Some(FailReason::Death)
    } else if serious_injury {
        Some(FailReason::SeriousInjury)
    } else {
        constraints_broken
            .first()
            .map(|message| FailReason::Constraint(message.clone()))
    };

    let top_reasons = top_reasons(&incidents, &per_mode, &means, serenity);
    StageReport {
        stage,
        passed: hard_fail.is_none() && score >= pass_score,
        score,
        pass_score,
        parts,
        hard_fail,
        per_mode,
        incidents,
        top_reasons,
        ended,
    }
}

/// The three biggest losses of points: incident groups (same kind, same 10 m cell) and
/// the serenity share of each mode.
fn top_reasons(
    incidents: &[Incident],
    per_mode: &[ModeStats],
    means: &[(Mode, f64)],
    serenity: f64,
) -> Vec<String> {
    let mut losses: Vec<(f64, String)> = Vec::new();

    let mut groups: IndexMap<(String, (i32, i32)), (IncidentKind, u32)> = IndexMap::new();
    for incident in incidents {
        let key = (
            incident_cost(incident.kind).1.to_string(),
            place(incident.position),
        );
        groups.entry(key).or_insert((incident.kind, 0)).1 += 1;
    }
    for ((_, cell), (kind, count)) in &groups {
        let (points, one, many) = incident_cost(*kind);
        let total = points * f64::from(*count);
        let noun = if *count == 1 { one } else { many };
        losses.push((
            total,
            format!(
                "{count} {noun} {}: -{}",
                place_name(*cell),
                rounded(total, 10.0)
            ),
        ));
    }

    // Each mode's share of the serenity penalty; the worst mode counts twice.
    if !means.is_empty() && serenity > 0.0 {
        let sum: f64 = means.iter().map(|(_, mean)| mean).sum();
        let worst = means.iter().map(|(_, mean)| *mean).fold(0.0, f64::max);
        let raw_total = sum + worst;
        if raw_total > 0.0 {
            let mut worst_given = false;
            for (mode, mean) in means {
                let is_worst = !worst_given && *mean == worst;
                worst_given |= is_worst;
                let share = (mean + if is_worst { *mean } else { 0.0 }) / raw_total * serenity;
                let stats = per_mode.iter().find(|stats| stats.mode == *mode);
                let delay = stats.map_or(0.0, |stats| stats.mean_delay_s);
                let gave_up = stats.map_or(0, |stats| stats.gave_up);
                let unfinished = stats.map_or(0, |stats| stats.unfinished);
                let not_entered = stats.map_or(0, |stats| stats.not_entered);
                let mut text = format!("{} were delayed {delay:.0} s on average", mode_noun(*mode));
                if gave_up > 0 {
                    text.push_str(&format!(" and {gave_up} gave up"));
                }
                if not_entered > 0 {
                    text.push_str(&format!(
                        ", {not_entered} {} could not enter the street",
                        mode_noun(*mode)
                    ));
                }
                if unfinished > 0 {
                    text.push_str(&format!(
                        ", {unfinished} {} did not finish",
                        mode_noun(*mode)
                    ));
                }
                losses.push((share, format!("{text}: -{}", rounded(share, 10.0))));
            }
        }
    }

    // Biggest first; a tie keeps the order of the list, so the result is the same on
    // every run.
    losses.sort_by(|a, b| b.0.total_cmp(&a.0));
    losses
        .into_iter()
        .filter(|(points, _)| *points > 0.0)
        .take(3)
        .map(|(_, text)| text)
        .collect()
}

/// The report of a finished run (`stage` from 1; the pass score comes from the level).
pub fn stage_report(run: &StreetRun, level: &LevelFile, stage: usize) -> StageReport {
    let pass_score = stage
        .checked_sub(1)
        .and_then(|index| level.stages.get(index))
        .map_or(0.0, |stage| stage.pass_score);
    report_from_events(run.events(), run.ended(), stage, pass_score, &[])
}

/// Plays the stages 1 to `up_to_stage` of `level` on `design` in order, headless, and
/// stops at the first one that fails (pivot plan §6: a star belongs to a design). Returns
/// the stars won (the stages passed) and the report of every stage that ran, the failed
/// one last.
pub fn stars_for_design(
    level: &LevelFile,
    design: &Design,
    up_to_stage: usize,
) -> Result<(usize, Vec<StageReport>), EditError> {
    let mut reports = Vec::new();
    let mut stars = 0;
    for stage in 1..=up_to_stage.min(level.stages.len()) {
        let options = stage_options(level, stage).expect("the stage is in range");
        let mut run = StreetRun::new(level, design, options)?;
        run.run_to_end();
        let report = stage_report(&run, level, stage);
        let passed = report.passed;
        reports.push(report);
        if !passed {
            break;
        }
        stars += 1;
    }
    Ok((stars, reports))
}
