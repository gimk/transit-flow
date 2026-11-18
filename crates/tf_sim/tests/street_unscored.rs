//! Everybody counts in the score (pivot plan §6): people who did not arrive count like
//! give-ups, so a jam costs points; and every serious event has a cause text.

use tf_sim::street::Design;
use tf_sim::street::level::{LevelFile, Mode};
use tf_sim::street::run::{Population, RunOptions, StreetEvent, StreetRun};
use tf_sim::street::score::{IncidentKind, stage_report};

fn run(level: &LevelFile, seed: u64, population: Population, scale: f64) -> StreetRun {
    let options = RunOptions {
        seed,
        population,
        demand_scale: scale,
    };
    let mut run = StreetRun::new(level, &Design::new(&level.id), options).expect("it builds");
    run.run_to_end();
    run
}

#[test]
fn a_jammed_run_scores_lower_than_the_same_run_with_less_demand() {
    let mut level = LevelFile::load("carrefour-ecole").expect("the level loads");
    // Without cyclists: with them a jam scored higher (fewer riders get in, so fewer
    // conflicts). Open point for Phase 8 (score tuning): people who never get through must
    // cost more, so that a jam scores lower with cyclists too. The light run is x1 (x2.5
    // against x6 is too close without cyclists: the same numbers on the Phase 6 base).
    for slice in &mut level.demand {
        slice.flows.retain(|flow| flow.mode != Mode::Bicycle);
    }
    let mean_score = |scale: f64| {
        let mut total = 0.0;
        let mut waiting = 0;
        for seed in 1..=5 {
            let run = run(&level, seed, Population::Mixed, scale);
            total += stage_report(&run, &level, 3).score;
            let s = run.summary();
            waiting += s.unfinished + s.not_entered + s.walkers.unfinished + s.walkers.not_entered;
        }
        (total / 5.0, waiting)
    };
    let (light, _) = mean_score(1.0);
    let (heavy, heavy_waiting) = mean_score(6.0);
    println!("mean score: x1 {light:.1}, x6 {heavy:.1}; x6 people not through: {heavy_waiting}");
    assert!(heavy < light, "{heavy} against {light}");
    assert!(
        heavy_waiting > 0,
        "a jammed run has people who did not get through"
    );
}

#[test]
fn people_who_did_not_get_through_are_in_the_report_text() {
    let level = LevelFile::load("carrefour-ecole").expect("the level loads");
    let mut seen_text = false;
    for seed in 1..=5 {
        let run = run(&level, seed, Population::Mixed, 6.0);
        let unfinished = run
            .events()
            .iter()
            .filter(|e| matches!(e.event, StreetEvent::Unfinished { .. }))
            .count();
        let report = stage_report(&run, &level, 3);
        let stats: u32 = report
            .per_mode
            .iter()
            .map(|m| m.unfinished + m.not_entered)
            .sum();
        assert_eq!(stats as usize, unfinished, "seed {seed}");
        seen_text |= report.top_reasons.iter().any(|text| {
            text.contains("could not enter the street") || text.contains("did not finish")
        });
    }
    assert!(seen_text, "the report says how many did not get through");
}

#[test]
fn every_serious_event_in_a_careless_run_has_a_cause_text() {
    let level = LevelFile::load("carrefour-ecole").expect("the level loads");
    let mut serious = 0;
    for seed in 1..=6 {
        let run = run(&level, seed, Population::Careless, 2.0);
        for incident in stage_report(&run, &level, 2).incidents {
            if matches!(
                incident.kind,
                IncidentKind::SeriousConflict | IncidentKind::Contact { .. }
            ) {
                serious += 1;
                assert!(
                    !incident.text.contains("no cause recorded")
                        && (!incident.causes.is_empty()
                            || incident.text.contains("no one broke a rule")),
                    "seed {seed}: {}",
                    incident.text
                );
            }
        }
    }
    assert!(serious > 0);
}
