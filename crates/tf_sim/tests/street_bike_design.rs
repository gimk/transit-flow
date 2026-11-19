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

/// The street of the "done when" test: the west arm of level 1, cars and vans going straight
/// on and riders going straight on, one long slice. The strip beside the eastbound car lane
/// (strip 3, a parking strip in the level) is what the design changes.
fn facility_level(cars_per_hour: u32, vans_per_hour: u32, riders_per_hour: u32) -> LevelFile {
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
            flow(Mode::Car, "west", "east", cars_per_hour),
            flow(Mode::Van, "west", "east", vans_per_hour),
            flow(Mode::Bicycle, "west", "east", riders_per_hour),
            // Oncoming traffic: without it a car can always use the other lane to pass a
            // rider with room to spare, and a shared lane looks safe.
            flow(Mode::Car, "east", "west", cars_per_hour / 2),
            flow(Mode::Van, "east", "west", vans_per_hour / 2),
        ],
    }];
    level
}

/// What a facility costs the riders: serious conflicts of any kind with a rider on one side,
/// close passes (light or serious: they count as conflicts), and contacts with a motor
/// vehicle.
#[derive(Debug, Default, Clone, Copy)]
struct Facility {
    serious: u32,
    close_passes: u32,
    contacts: u32,
    /// Cars and vans that entered the street: the comparison is fair only if it is the same.
    cars: u32,
}

impl Facility {
    /// The metric of the brief: serious conflicts, and close passes (light or serious) as
    /// conflicts. A contact is not a conflict: it is reported apart.
    fn conflicts(self) -> u32 {
        self.serious + self.close_passes
    }
}

fn measure_facility(
    level: &LevelFile,
    commands: &[DesignCommand],
    seeds: std::ops::Range<u64>,
) -> Facility {
    let mut design = Design::new(&level.id);
    design.commands = commands.to_vec();
    let mut total = Facility::default();
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
                if matches!(mode, Mode::Car | Mode::Van) {
                    total.cars += 1;
                }
            }
        }
        let car_rider = |a: &tf_sim::street::ids::AgentId, b: &tf_sim::street::ids::AgentId| {
            let pair = (mode_of.get(a), mode_of.get(b));
            matches!(
                pair,
                (Some(Mode::Car | Mode::Van), Some(Mode::Bicycle))
                    | (Some(Mode::Bicycle), Some(Mode::Car | Mode::Van))
            )
        };
        for e in run.events() {
            match &e.event {
                StreetEvent::Conflict {
                    kind, grade, a, b, ..
                } if car_rider(a, b) => {
                    if *kind == tf_sim::street::run::events::ConflictKind::ClosePass {
                        total.close_passes += 1;
                    }
                    if *grade == Grade::Serious {
                        total.serious += 1;
                    }
                }
                StreetEvent::Contact { a, b, .. } if car_rider(a, b) => total.contacts += 1,
                _ => {}
            }
        }
    }
    total
}

/// The "done when" of Phase 6: on one street, a protected track gives measurably fewer
/// serious conflicts than a painted lane, and a painted lane fewer than no bicycle facility.
/// Close passes count as conflicts. The cause is the separator: a kerb keeps vans out of the
/// lane, keeps cars from cutting the corner across it, and keeps riders in it (they brake
/// instead of swerving into the car lane); paint lets all three happen, and with no facility
/// the riders share the lane with the cars.
#[test]
fn a_track_gives_fewer_serious_conflicts_than_a_lane_and_a_lane_fewer_than_none() {
    let level = facility_level(400, 120, 400);
    // The facility runs on both arms of the school street, so that the riders keep their
    // lane through the junction (a lane that ends at the junction makes a merge conflict
    // that has nothing to do with the separator).
    let facility = |kind: StripUse| {
        vec![
            use_(kind),
            DesignCommand::SetStripUse {
                segment: 2,
                strip: 3,
                use_: kind,
                direction: Direction::Forward,
            },
        ]
    };
    let none = measure_facility(&level, &[], SEEDS);
    let lane = measure_facility(&level, &facility(StripUse::BikeLane), SEEDS);
    let track = measure_facility(&level, &facility(StripUse::BikeTrack), SEEDS);
    println!("none {none:?}\nlane {lane:?}\ntrack {track:?}");
    // The shared lane is the worst of the three: the cars come up behind the riders and
    // brake hard, and the ones that pass them do it in the lane, with less than the metre
    // of R414-4.
    assert!(none.serious > 0 && none.close_passes > 0, "{none:?}");
    // A painted lane separates them, so the riders are not passed in their lane.
    assert!(lane.close_passes < none.close_passes, "{lane:?} {none:?}");
    // Contacts follow the same order: track <= painted lane <= no facility.
    assert!(
        lane.contacts <= none.contacts,
        "lane {lane:?} against none {none:?}"
    );
    // A kerb keeps the vans out and the riders in: nothing meets anything.
    assert!(track.contacts == 0 && track.close_passes == 0, "{track:?}");
    assert!(
        lane.conflicts() < none.conflicts(),
        "lane {lane:?} against none {none:?}"
    );
    assert!(
        track.conflicts() < lane.conflicts(),
        "track {track:?} against lane {lane:?}"
    );
}

/// The painted lane on both arms of the school street (the "done when" street).
fn lane_on_both_arms() -> Vec<DesignCommand> {
    [1, 2]
        .into_iter()
        .map(|segment| DesignCommand::SetStripUse {
            segment,
            strip: 3,
            use_: StripUse::BikeLane,
            direction: Direction::Forward,
        })
        .collect()
}

/// Contacts between a motor vehicle and a rider, with the speed of the vehicle and whether
/// it has reached its stop place, read tick by tick.
fn moving_contacts(level: &LevelFile, commands: &[DesignCommand]) -> Vec<(u64, u64, f64, bool)> {
    let mut design = Design::new(&level.id);
    design.commands = commands.to_vec();
    let mut found = Vec::new();
    for seed in SEEDS {
        let options = RunOptions {
            seed,
            population: Population::Mixed,
            demand_scale: 1.0,
        };
        let mut run = StreetRun::new(level, &design, options).expect("builds");
        let mut seen = 0;
        while !run.finished() {
            run.step();
            let events = run.events();
            for e in &events[seen..] {
                if let StreetEvent::Contact { a, b, .. } = &e.event {
                    let motor = [a, b].into_iter().find(|id| {
                        run.agents
                            .get(*id)
                            .is_some_and(|agent| agent.mode != Mode::Bicycle)
                    });
                    let rider = [a, b].into_iter().any(|id| {
                        run.agents
                            .get(id)
                            .is_some_and(|agent| agent.mode == Mode::Bicycle)
                    });
                    if let (Some(motor), true) = (motor, rider) {
                        let agent = &run.agents[motor];
                        let at_stop = agent.stop.as_ref().is_some_and(|s| agent.d >= s.at_d);
                        found.push((seed, e.tick, agent.speed, at_stop));
                    }
                }
            }
            seen = events.len();
        }
    }
    found
}

/// Cause A of the W4b review: a van that pulls over into the painted lane for its delivery
/// stop sweeps into the rider it overtakes, because the side limit compared the rider's
/// old position with its own. No vehicle that moves hits a rider on its way to its stop.
#[test]
fn a_van_that_pulls_over_does_not_sweep_into_a_rider_it_overtakes() {
    let level = facility_level(0, 120, 400);
    let found = moving_contacts(&level, &lane_on_both_arms());
    let bad: Vec<_> = found
        .iter()
        .filter(|(_, _, speed, at_stop)| *speed > 1.0 && !*at_stop)
        .collect();
    assert!(bad.is_empty(), "{bad:?}");
}

/// Cause B of the W4b review: a rider swerves into the car lane in the first metres of the
/// street and a car comes in right behind it. The gate holds the car back.
#[test]
fn a_car_does_not_come_in_behind_a_rider_that_swerves_at_the_gate() {
    let level = facility_level(1200, 0, 400);
    // A car that stands in a jam and a rider that rides into it is another matter.
    let found: Vec<_> = moving_contacts(&level, &lane_on_both_arms())
        .into_iter()
        .filter(|(_, _, speed, _)| *speed > 1.0)
        .collect();
    assert!(found.is_empty(), "{found:?}");
}

/// A rider looks back before it moves out of its lane (by its attention: 1 for a careful
/// one): careful riders never move out in front of a car that could not stop, so no car
/// brakes hard for a rider in the car lane. Mixed riders may forget.
#[test]
fn careful_riders_never_move_out_in_front_of_a_close_car() {
    let level = facility_level(400, 120, 400);
    let mut design = Design::new(&level.id);
    design.commands = lane_on_both_arms();
    let mut moved_out = 0;
    for seed in SEEDS {
        let options = RunOptions {
            seed,
            population: Population::Careful,
            demand_scale: 1.0,
        };
        let mut run = StreetRun::new(&level, &design, options).expect("builds");
        run.run_to_end();
        let mut mode_of = std::collections::HashMap::new();
        for e in run.events() {
            if let StreetEvent::Spawned { agent, mode, .. } = &e.event {
                mode_of.insert(*agent, *mode);
            }
        }
        for e in run.events() {
            if let StreetEvent::Conflict {
                kind: tf_sim::street::run::events::ConflictKind::HardBrake,
                a,
                b,
                ..
            } = &e.event
                && matches!(mode_of.get(a), Some(Mode::Car | Mode::Van))
                && mode_of.get(b) == Some(&Mode::Bicycle)
            {
                moved_out += 1;
            }
            assert!(
                !matches!(&e.event, StreetEvent::Contact { a, b, .. }
                if matches!(
                    (mode_of.get(a), mode_of.get(b)),
                    (Some(Mode::Car | Mode::Van), Some(Mode::Bicycle))
                        | (Some(Mode::Bicycle), Some(Mode::Car | Mode::Van))
                )),
                "seed {seed}: a contact"
            );
        }
    }
    assert_eq!(moved_out, 0);
}

/// Riders that stay in their own painted lane do not hold the cars back at the gate: the
/// same cars enter the street with and without them.
#[test]
fn riders_in_their_own_lane_do_not_cut_the_car_flow() {
    let with_riders = facility_level(400, 120, 400);
    let without = facility_level(400, 120, 0);
    let lane = lane_on_both_arms();
    // A rider flow of 0 is a flow that never spawns; the car streams are their own.
    let a = measure_facility(&with_riders, &lane, SEEDS);
    let b = measure_facility(&without, &lane, SEEDS);
    println!("cars entered: with riders {}, without {}", a.cars, b.cars);
    // Within one car per seed (a car that is still waiting at the end of the slice).
    assert!(
        a.cars.abs_diff(b.cars) <= SEEDS.end as u32 - SEEDS.start as u32,
        "{a:?} {b:?}"
    );
}
