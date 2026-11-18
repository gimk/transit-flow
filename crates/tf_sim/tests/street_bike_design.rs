//! Phase 6 W4a: the classic right-turn conflict (a vehicle turns right across a rider going
//! straight on its right) and the "done when" of Phase 6: on one street, a protected track
//! gives fewer serious conflicts than a painted lane, and a painted lane fewer than no
//! bicycle facility; a design change removes the right-turn conflict.

use tf_sim::street::level::{Flow, LevelFile, Mode, Slice};
use tf_sim::street::run::events::Grade;
use tf_sim::street::run::{Population, RunOptions, StreetEvent, StreetRun};
use tf_sim::street::space::{Direction, StripUse};
use tf_sim::street::{Design, DesignCommand};

/// The test street: the level "Carrefour école", west arm (segment 1, eastbound strip 3
/// is a parking strip). Cars from the west turn right (to the south gate) or go straight;
/// riders from the west go straight on. One long slice so that there are many meetings.
fn level(right_turners_per_hour: u32, riders_per_hour: u32) -> LevelFile {
    let mut level = LevelFile::load("carrefour-ecole").expect("the level loads");
    let flow = |mode, from: &str, to: &str, per_hour| Flow {
        mode,
        from: from.to_string(),
        to: to.to_string(),
        per_hour,
    };
    level.demand = vec![Slice {
        name: "test".to_string(),
        duration_s: 240.0,
        flows: vec![
            flow(Mode::Car, "west", "south", right_turners_per_hour),
            flow(Mode::Van, "west", "south", right_turners_per_hour / 3),
            flow(Mode::Car, "west", "east", 100),
            flow(Mode::Bicycle, "west", "east", riders_per_hour),
        ],
    }];
    level
}

fn use_(strip_use: StripUse) -> DesignCommand {
    DesignCommand::SetStripUse {
        segment: 1,
        strip: 3,
        use_: strip_use,
        direction: Direction::Forward,
    }
}

#[derive(Debug, Default, Clone, Copy)]
/// Conflicts (near misses, hard brakes, close passes) and contacts between a driver and a rider,
/// anywhere on the street; the junction right turn dominates them on this test street.
struct Counts {
    right_turn_serious: u32,
    right_turn_any: u32,
    contacts: u32,
}

fn measure(level: &LevelFile, commands: &[DesignCommand], seeds: std::ops::Range<u64>) -> Counts {
    let mut design = Design::new(&level.id);
    design.commands = commands.to_vec();
    let mut counts = Counts::default();
    for seed in seeds {
        let options = RunOptions {
            seed,
            population: Population::Mixed,
            demand_scale: 1.0,
        };
        let mut run = StreetRun::new(level, &design, options).expect("builds");
        run.run_to_end();
        let mut mode_of = std::collections::HashMap::new();
        for e in run.events() {
            if let StreetEvent::Spawned { agent, mode, .. } = &e.event {
                mode_of.insert(*agent, *mode);
            }
        }
        for e in run.events() {
            match &e.event {
                StreetEvent::Conflict { a, b, grade, .. } => {
                    let pair = (mode_of.get(a), mode_of.get(b));
                    let car_rider = matches!(
                        pair,
                        (Some(Mode::Car | Mode::Van), Some(Mode::Bicycle))
                            | (Some(Mode::Bicycle), Some(Mode::Car | Mode::Van))
                    );
                    if car_rider {
                        counts.right_turn_any += 1;
                        if *grade == Grade::Serious {
                            counts.right_turn_serious += 1;
                        }
                    }
                }
                StreetEvent::Contact {
                    a,
                    b,
                    causes,
                    impact_kmh,
                    position,
                    ..
                } => {
                    if std::env::var("SHOW").is_ok() {
                        println!(
                            "seed {seed} t{} {:?} {:?} {:?} {:.1} {:?}",
                            e.tick,
                            mode_of.get(a),
                            mode_of.get(b),
                            position,
                            impact_kmh,
                            causes
                                .iter()
                                .map(|c| (c.agent.0, c.what))
                                .collect::<Vec<_>>()
                        );
                    }
                    let pair = (mode_of.get(a), mode_of.get(b));
                    if matches!(
                        pair,
                        (Some(Mode::Car | Mode::Van), Some(Mode::Bicycle))
                            | (Some(Mode::Bicycle), Some(Mode::Car | Mode::Van))
                    ) {
                        counts.contacts += 1;
                    }
                }
                _ => {}
            }
        }
    }
    counts
}

const SEEDS: std::ops::Range<u64> = 1..25;

/// The classic right-turn conflict: with priority to the right and a painted bicycle lane,
/// vehicles that turn right across riders going straight on their right meet them (the
/// rate comes from perception: blind spots, a van, a look at the wrong place). Measured on
/// 24 seeds with 300 cars and 100 vans turning right and 200 riders going straight per hour:
/// about 50 serious conflicts and 25 contacts. A design change removes most of them:
/// signals (a conflict with a green of its own), then a bicycle box (the riders stand in
/// front of the cars and are seen), then a leading bicycle green (they clear the crossing
/// first).
#[test]
fn a_right_turn_conflict_with_riders_is_removed_by_design() {
    use tf_sim::street::design::SignalTemplate;
    use tf_sim::street::level::JunctionControl;
    let level = level(300, 200);
    let lane = use_(StripUse::BikeLane);
    let signals = DesignCommand::SetJunctionControl {
        node: 0,
        control: JunctionControl::Signals,
        template: Some(SignalTemplate::TwoPhase { green_s: 25.0 }),
    };
    let boxed = DesignCommand::AddBicycleBox {
        node: 0,
        segment: 1,
        depth: 4.0,
    };
    let lead = DesignCommand::SetBicycleLead {
        node: 0,
        lead_s: 4.0,
    };
    let priority = measure(&level, std::slice::from_ref(&lane), SEEDS);
    let with_signals = measure(&level, &[lane.clone(), signals.clone()], SEEDS);
    let with_box_and_lead = measure(&level, &[lane, signals, boxed, lead], SEEDS);
    println!("priority {priority:?}\nsignals {with_signals:?}\nbox and lead {with_box_and_lead:?}");
    // It happens, at a plausible rate: a few per hundred right turns, not every one.
    assert!(priority.contacts >= 5, "{priority:?}");
    assert!(priority.right_turn_serious >= 10, "{priority:?}");
    // Signals alone halve it, and the box with the leading green removes most of the rest.
    assert!(with_signals.right_turn_serious * 3 <= priority.right_turn_serious * 2);
    assert!(with_signals.contacts * 3 <= priority.contacts * 2);
    assert!(with_box_and_lead.right_turn_serious * 3 <= priority.right_turn_serious);
    assert!(with_box_and_lead.contacts * 3 <= priority.contacts);
    assert!(with_box_and_lead.right_turn_serious < with_signals.right_turn_serious);
}
