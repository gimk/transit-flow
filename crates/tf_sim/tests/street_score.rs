//! Score, verdict and report of a star stage (pivot plan §6): the score is a pure function
//! of the event log, a careful population scores better than a careless one, a death
//! fails the stage, the worst mode counts double, and a star belongs to a design.

use glam::Vec2;
use tf_sim::street::level::{LevelFile, Mode};
use tf_sim::street::run::events::{
    Cause, CauseKind, EndReason, Grade, LoggedEvent, Outcome, StreetEvent,
};
use tf_sim::street::run::{Population, StreetRun};
use tf_sim::street::score::{
    FailReason, SERENITY_CAP, SERENITY_WEIGHT, report_from_events, serenity_penalty, stage_options,
    stage_report, stars_for_design,
};
use tf_sim::street::{AgentId, Design};

fn level() -> LevelFile {
    LevelFile::load("carrefour-ecole").expect("the test level loads")
}

fn at(tick: u64, event: StreetEvent) -> LoggedEvent {
    LoggedEvent { tick, event }
}

fn spawn(id: i32, mode: Mode) -> LoggedEvent {
    at(
        0,
        StreetEvent::Spawned {
            agent: AgentId(id),
            mode,
            archetype: "test".to_string(),
            from: "a".to_string(),
            to: "b".to_string(),
        },
    )
}

fn arrive(id: i32, annoyance: f64) -> LoggedEvent {
    at(
        100,
        StreetEvent::TripEnded {
            agent: AgentId(id),
            time_s: 10.0,
            delay_s: 2.0,
            annoyance,
        },
    )
}

fn contact(outcome: Outcome) -> LoggedEvent {
    at(
        60,
        StreetEvent::Contact {
            a: AgentId(1),
            b: AgentId(2),
            position: Vec2::new(12.0, 34.0),
            impact_kmh: 34.0,
            outcome,
            causes: vec![Cause {
                agent: AgentId(1),
                what: CauseKind::RanRed,
            }],
        },
    )
}

fn run_report(
    stage: usize,
    seed: u64,
    population: Population,
) -> tf_sim::street::score::StageReport {
    let level = level();
    let mut options = stage_options(&level, stage).expect("the stage exists");
    options.seed = seed;
    options.population = population;
    let mut run = StreetRun::new(&level, &Design::new(&level.id), options).expect("it builds");
    run.run_to_end();
    stage_report(&run, &level, stage)
}

#[test]
fn the_level_has_three_stages() {
    let level = level();
    assert_eq!(level.stages.len(), 3);
    assert!(level.stages[0].demand_scale < level.stages[1].demand_scale);
    assert!(level.stages[1].demand_scale < level.stages[2].demand_scale);
    assert!(stage_options(&level, 0).is_none());
    assert!(stage_options(&level, 4).is_none());
    assert_eq!(stage_options(&level, 2).unwrap().demand_scale, 1.6);
}

#[test]
fn the_score_is_deterministic() {
    let first = run_report(1, 1, Population::Mixed);
    let second = run_report(1, 1, Population::Mixed);
    assert_eq!(first, second);
    assert!((0.0..=100.0).contains(&first.score));
}

#[test]
fn the_empty_design_passes_stage_one() {
    let level = level();
    let (_, reports) =
        stars_for_design(&level, &Design::new(&level.id), 3).expect("the design builds");
    assert!(reports[0].passed, "stage 1: {:?}", reports[0]);
}

#[test]
fn a_careful_population_scores_better_than_a_careless_one() {
    let careful = run_report(2, 1, Population::Careful);
    let careless = run_report(2, 1, Population::Careless);
    assert!(
        careful.score > careless.score,
        "careful {} careless {}",
        careful.score,
        careless.score
    );
}

#[test]
fn a_death_fails_the_stage_whatever_the_score() {
    let events = [
        spawn(1, Mode::Car),
        spawn(2, Mode::Car),
        contact(Outcome::Death),
    ];
    let report = report_from_events(&events, Some(EndReason::Death), 1, 0.0, &[]);
    assert!(!report.passed);
    assert_eq!(report.hard_fail, Some(FailReason::Death));
    assert_eq!(report.incidents.len(), 1);
    assert!(report.incidents[0].text.contains("ran a red light"));
    // A serious injury fails too; a light one does not.
    let serious = report_from_events(&[contact(Outcome::SeriousInjury)], None, 1, 0.0, &[]);
    assert_eq!(serious.hard_fail, Some(FailReason::SeriousInjury));
    let light = report_from_events(&[contact(Outcome::LightInjury)], None, 1, 0.0, &[]);
    assert!(light.passed && light.hard_fail.is_none());
    // A broken constraint is the hook for later.
    let broken = report_from_events(&[], None, 1, 0.0, &["bus cannot pass".to_string()]);
    assert_eq!(
        broken.hard_fail,
        Some(FailReason::Constraint("bus cannot pass".to_string()))
    );
}

#[test]
fn the_worst_mode_counts_double() {
    // Two modes with a mean annoyance of 1 and 3: the worst (3) is counted twice.
    let expected = SERENITY_WEIGHT * (1.0 + 3.0 + 3.0) / 3.0;
    assert!(expected < SERENITY_CAP);
    assert!((serenity_penalty(&[1.0, 3.0]) - expected).abs() < 1e-9);
    assert_eq!(serenity_penalty(&[]), 0.0);
    assert_eq!(serenity_penalty(&[1000.0]), SERENITY_CAP);

    let events = [
        spawn(1, Mode::Car),
        arrive(1, 1.0),
        spawn(2, Mode::Bicycle),
        arrive(2, 3.0),
    ];
    let report = report_from_events(&events, Some(EndReason::Completed), 1, 0.0, &[]);
    assert!((report.parts.serenity - (expected * 100.0).round() / 100.0).abs() < 1e-9);
    assert_eq!(report.score, 100.0 - report.parts.serenity);
    assert!(
        report
            .top_reasons
            .iter()
            .any(|reason| reason.contains("cyclists"))
    );
}

#[test]
fn incidents_lose_points_and_give_top_reasons() {
    let conflict = |tick| {
        at(
            tick,
            StreetEvent::Conflict {
                kind: tf_sim::street::run::ConflictKind::Pet,
                grade: Grade::Serious,
                a: AgentId(1),
                b: AgentId(2),
                position: Vec2::new(3.0, 4.0),
                measure: 0.5,
                causes: vec![],
            },
        )
    };
    let events = [conflict(10), conflict(20), contact(Outcome::NoInjury)];
    let report = report_from_events(&events, None, 1, 0.0, &[]);
    assert_eq!(report.parts.safety, 2.0 * 4.0 + 6.0);
    assert_eq!(report.top_reasons.len(), 2);
    assert!(report.top_reasons[0].starts_with("2 serious conflicts"));
}

#[test]
fn stars_for_design_stops_at_the_first_failure() {
    let mut level = level();
    // An impossible second stage: the run stops there and the third never runs.
    level.stages[1].pass_score = 101.0;
    let (stars, reports) =
        stars_for_design(&level, &Design::new(&level.id), 3).expect("the design builds");
    assert_eq!(stars, 1);
    assert_eq!(reports.len(), 2);
    assert!(reports[0].passed && !reports[1].passed);
    assert_eq!(reports[1].stage, 2);
}
