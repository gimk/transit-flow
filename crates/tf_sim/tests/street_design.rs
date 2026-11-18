//! Design commands of Phase 4: bollards and junction control with signal templates. Each
//! one applies, replays from JSON, validates and undoes; then runs show what it does to
//! walkers and drivers.

use glam::Vec2;
use tf_sim::street::design::{Side, SignalTemplate};
use tf_sim::street::level::{JunctionControl, LevelFile, ObstacleKind, Shape};
use tf_sim::street::model::{BOLLARD_RADIUS, EditError, Street};
use tf_sim::street::run::{Population, RunOptions, StreetEvent, StreetRun};
use tf_sim::street::signals::{Light, MAX_GREEN_S, MIN_GREEN_S};
use tf_sim::street::{Design, DesignCommand};

fn level() -> LevelFile {
    LevelFile::load("carrefour-ecole").expect("the test level loads")
}

fn design_of(level: &LevelFile, commands: Vec<DesignCommand>) -> Design {
    let mut design = Design::new(&level.id);
    design.commands = commands;
    design
}

fn run(level: &LevelFile, design: &Design, seed: u64, population: Population) -> StreetRun {
    let options = RunOptions {
        seed,
        population,
        demand_scale: 2.0,
    };
    StreetRun::new(level, design, options).expect("the design builds")
}

fn bollards(segment: i32, side: Side, from: f32, to: f32) -> DesignCommand {
    DesignCommand::AddBollards {
        segment,
        side,
        from,
        to,
    }
}

fn signals(template: Option<SignalTemplate>) -> DesignCommand {
    DesignCommand::SetJunctionControl {
        node: 0,
        control: JunctionControl::Signals,
        template,
    }
}

/// The circles of the bollards in a street.
fn bollard_circles(street: &Street) -> Vec<(Vec2, f32)> {
    street
        .obstacles
        .values()
        .filter(|o| o.kind == ObstacleKind::Bollard)
        .map(|o| match &o.shape {
            Shape::Circle(c) => (c.center, c.radius),
            Shape::Polygon(_) => panic!("a bollard is a circle"),
        })
        .collect()
}

#[test]
fn a_row_of_bollards_is_placed_along_the_kerb_and_removed_again() {
    let level = level();
    let base = Street::from_level(&level).unwrap();
    let mut street = base.clone();
    // Segment 2 runs east from the junction; its left side (north) is the school
    // sidewalk: 3 m wide, its kerb 3.5 m north of the centerline (y = 75 - 3.5 = 71.5).
    let change = street.apply(&bollards(2, Side::Left, 10.0, 40.0)).unwrap();
    assert_eq!(change.segment_ids, vec![2]);
    let row = &street.bollard_rows[&1];
    assert_eq!(row.obstacle_ids.len(), 21, "30 m at 1.5 m: 20 gaps");
    let circles = bollard_circles(&street);
    assert_eq!(circles.len(), 21);
    for (center, radius) in &circles {
        assert_eq!(*radius, BOLLARD_RADIUS);
        assert!((center.y - 71.5).abs() < 0.01, "on the kerb line: {center}");
    }
    assert!((circles[0].0.x - 85.0).abs() < 0.01, "{}", circles[0].0);
    assert!((circles[20].0.x - 115.0).abs() < 0.01);
    for pair in circles.windows(2) {
        assert!(pair[0].0.distance(pair[1].0) <= 1.5 + 1e-3);
    }
    // The level's own obstacles keep their ids; the player's come after.
    let fixed = level.obstacles.len() as i32;
    assert!(row.obstacle_ids.iter().all(|id| *id > fixed));
    assert!(
        row.obstacle_ids
            .iter()
            .all(|id| !street.obstacles[id].fixed)
    );
    // The other side: the south sidewalk of the same segment, on its kerb.
    street.apply(&bollards(2, Side::Right, 10.0, 40.0)).unwrap();
    let south: Vec<Vec2> = street.bollard_rows[&2]
        .obstacle_ids
        .iter()
        .map(|id| match &street.obstacles[id].shape {
            Shape::Circle(c) => c.center,
            Shape::Polygon(_) => unreachable!(),
        })
        .collect();
    assert!(south.iter().all(|c| (c.y - 79.5).abs() < 0.01), "{south:?}");
    street
        .apply(&DesignCommand::RemoveBollards { id: 2 })
        .unwrap();
    street
        .apply(&DesignCommand::RemoveBollards { id: 1 })
        .unwrap();
    assert_eq!(street.obstacles, base.obstacles);
    assert!(street.bollard_rows.is_empty());
    assert_eq!(
        street.apply(&DesignCommand::RemoveBollards { id: 1 }),
        Err(EditError::Invalid("no bollard row 1".into()))
    );
}

#[test]
fn bollard_commands_are_validated() {
    let level = level();
    let mut street = Street::from_level(&level).unwrap();
    let before = street.clone();
    let bad = [
        bollards(99, Side::Left, 10.0, 20.0),
        bollards(2, Side::Left, 20.0, 10.0),
        bollards(2, Side::Left, 10.0, 10.0),
        bollards(2, Side::Left, 0.0, 20.0), // inside the junction
        bollards(2, Side::Left, 10.0, 200.0),
        bollards(2, Side::Left, f32::NAN, 20.0),
    ];
    for command in &bad {
        assert!(street.apply(command).is_err(), "{command:?}");
    }
    assert_eq!(street, before, "an error changes nothing");
    street.apply(&bollards(2, Side::Left, 10.0, 30.0)).unwrap();
    assert!(street.apply(&bollards(2, Side::Left, 25.0, 40.0)).is_err());
    street.apply(&bollards(2, Side::Left, 30.0, 40.0)).unwrap();
    // A segment without a sidewalk on a side refuses.
    let mut flat = level.clone();
    flat.segments[1].profile.strips.remove(4);
    let mut street = Street::from_level(&flat).unwrap();
    assert!(street.apply(&bollards(2, Side::Right, 10.0, 30.0)).is_err());
}

#[test]
fn bollards_replay_from_json_and_undo() {
    let level = level();
    let commands = vec![
        bollards(2, Side::Left, 10.0, 40.0),
        bollards(3, Side::Right, 12.0, 30.0),
        DesignCommand::RemoveBollards { id: 1 },
        bollards(4, Side::Left, 15.0, 25.0),
    ];
    let design = design_of(&level, commands);
    let parsed = Design::from_json(&design.to_json()).unwrap();
    assert_eq!(parsed, design);
    let full = Street::from_level_and_design(&level, &parsed).unwrap();
    assert_eq!(
        full,
        Street::from_level_and_design(&level, &design).unwrap()
    );
    assert_eq!(
        full.bollard_rows.keys().copied().collect::<Vec<_>>(),
        [2, 3]
    );
    // Undo: the design without its last command gives the street before it.
    let mut undone = design.clone();
    let last = undone.commands.pop().unwrap();
    let before = Street::from_level_and_design(&level, &undone).unwrap();
    let mut again = before.clone();
    again.apply(&last).unwrap();
    assert_eq!(again, full);
    assert_eq!(before.bollard_rows.len(), 1);
    let empty = design_of(&level, Vec::new());
    assert_eq!(
        Street::from_level_and_design(&level, &empty).unwrap(),
        Street::from_level(&level).unwrap()
    );
}

#[test]
fn walkers_walk_round_bollards_and_still_arrive() {
    let level = level();
    // On both sidewalks of segment 2 (the school gate is on the north one).
    let design = design_of(
        &level,
        vec![
            bollards(2, Side::Left, 8.0, 70.0),
            bollards(2, Side::Right, 8.0, 70.0),
        ],
    );
    let street = Street::from_level_and_design(&level, &design).unwrap();
    let circles = bollard_circles(&street);
    assert!(circles.len() > 80);
    for seed in 1..=2 {
        let mut plain = run(
            &level,
            &design_of(&level, Vec::new()),
            seed,
            Population::Careful,
        );
        plain.run_to_end();
        let mut with = run(&level, &design, seed, Population::Careful);
        let mut closest = f32::MAX;
        while !with.finished() {
            with.step();
            for walker in with.walkers.values() {
                for (center, radius) in &circles {
                    let gap = walker.pos.distance(*center) - radius;
                    closest = closest.min(gap);
                    assert!(gap > 0.0, "seed {seed}: a walker is inside a bollard");
                }
            }
        }
        let (a, b) = (plain.summary().walkers, with.summary().walkers);
        assert_eq!(b.spawned, a.spawned);
        assert!(b.arrived + 2 >= a.arrived, "seed {seed}: {a:?} vs {b:?}");
        assert_eq!(b.gave_up, a.gave_up, "seed {seed}");
        // Walkers still on their way when the run ends: a person more or less is chance
        // (cyclists change the timing of the traffic they cross).
        assert!(
            b.still_walking.abs_diff(a.still_walking) <= 1,
            "seed {seed}"
        );
        println!("seed {seed}: closest walker centre to a bollard edge {closest:.2} m");
    }
}

#[test]
fn junction_control_applies_replays_validates_and_undoes() {
    let level = level();
    let base = Street::from_level(&level).unwrap();
    let mut street = base.clone();
    let change = street
        .apply(&signals(Some(SignalTemplate::TwoPhase { green_s: 25.0 })))
        .unwrap();
    assert_eq!(change.node_ids, vec![0]);
    let junction = &street.junctions[&0];
    assert_eq!(junction.control, JunctionControl::Signals);
    let plan = junction.signal_plan.as_ref().expect("a plan");
    assert_eq!(plan.phases.len(), 2, "two streets, one phase each");
    let mut arms: Vec<i32> = plan.phases.iter().flat_map(|p| p.green.clone()).collect();
    arms.sort_unstable();
    assert_eq!(arms, vec![1, 2, 3, 4]);
    for phase in &plan.phases {
        assert_eq!(phase.green_s, 25.0);
        let mut pair = phase.green.clone();
        pair.sort_unstable();
        assert!(pair == [1, 2] || pair == [3, 4], "{pair:?}");
    }
    // With a walk phase.
    street
        .apply(&signals(Some(SignalTemplate::TwoPhaseWithWalk {
            green_s: 20.0,
            walk_s: 12.0,
        })))
        .unwrap();
    let plan = street.junctions[&0].signal_plan.as_ref().unwrap();
    assert_eq!(plan.phases.len(), 3);
    assert!(plan.phases[2].green.is_empty());
    assert_eq!(plan.phases[2].green_s, 12.0);
    // No template: the run's default plan (no stored plan).
    street.apply(&signals(None)).unwrap();
    assert_eq!(street.junctions[&0].signal_plan, None);
    assert_eq!(street.junctions[&0].control, JunctionControl::Signals);
    // Back to priority to the right removes the plan.
    street
        .apply(&signals(Some(SignalTemplate::TwoPhase { green_s: 30.0 })))
        .unwrap();
    street
        .apply(&DesignCommand::SetJunctionControl {
            node: 0,
            control: JunctionControl::PriorityToRight,
            template: None,
        })
        .unwrap();
    assert_eq!(street.junctions[&0].signal_plan, None);
    assert_eq!(
        street.junctions[&0].control,
        JunctionControl::PriorityToRight
    );

    // Validation.
    let before = street.clone();
    for green_s in [MIN_GREEN_S - 0.5, MAX_GREEN_S + 0.5, f32::NAN] {
        let command = signals(Some(SignalTemplate::TwoPhase { green_s }));
        assert!(street.apply(&command).is_err(), "{green_s}");
    }
    let short_walk = signals(Some(SignalTemplate::TwoPhaseWithWalk {
        green_s: 20.0,
        walk_s: 5.0,
    }));
    assert!(street.apply(&short_walk).is_err());
    assert_eq!(
        street.apply(&DesignCommand::SetJunctionControl {
            node: 3,
            control: JunctionControl::Signals,
            template: None
        }),
        Err(EditError::UnknownJunction(3))
    );
    let plan_without_signals = DesignCommand::SetJunctionControl {
        node: 0,
        control: JunctionControl::PriorityToRight,
        template: Some(SignalTemplate::TwoPhase { green_s: 20.0 }),
    };
    assert!(street.apply(&plan_without_signals).is_err());
    assert_eq!(street, before, "an error changes nothing");
    for green_s in [MIN_GREEN_S, MAX_GREEN_S] {
        let command = signals(Some(SignalTemplate::TwoPhase { green_s }));
        assert!(street.apply(&command).is_ok(), "{green_s}");
    }

    // Replay from JSON and undo.
    let design = design_of(
        &level,
        vec![
            signals(Some(SignalTemplate::TwoPhaseWithWalk {
                green_s: 20.0,
                walk_s: 12.0,
            })),
            bollards(1, Side::Left, 10.0, 20.0),
        ],
    );
    let parsed = Design::from_json(&design.to_json()).unwrap();
    assert_eq!(parsed, design);
    let full = Street::from_level_and_design(&level, &parsed).unwrap();
    let mut undone = design.clone();
    undone.commands.pop();
    let mut again = Street::from_level_and_design(&level, &undone).unwrap();
    assert!(again.junctions[&0].signal_plan.is_some());
    again.apply(&design.commands[1]).unwrap();
    assert_eq!(again, full);
    undone.commands.pop();
    assert_eq!(
        Street::from_level_and_design(&level, &undone).unwrap(),
        base
    );
}

/// The run's own default plan and the two-phase template at the default green group the
/// arms alike, so the two runs are the same run.
#[test]
fn the_default_plan_and_the_two_phase_template_are_the_same_plan() {
    let level = level();
    let templated = design_of(
        &level,
        vec![signals(Some(SignalTemplate::TwoPhase { green_s: 20.0 }))],
    );
    let default = design_of(&level, vec![signals(None)]);
    for seed in 1..=2 {
        let mut a = run(&level, &templated, seed, Population::Mixed);
        let mut b = run(&level, &default, seed, Population::Mixed);
        a.run_to_end();
        b.run_to_end();
        assert_eq!(a.log_hash(), b.log_hash(), "seed {seed}");
    }
}

const WALK_S: f32 = 15.0;

fn walk_template() -> Design {
    let level = level();
    design_of(
        &level,
        vec![signals(Some(SignalTemplate::TwoPhaseWithWalk {
            green_s: 15.0,
            walk_s: WALK_S,
        }))],
    )
}

/// (cycle length, start of the walk phase), seconds.
fn walk_phase(level: &LevelFile, design: &Design) -> (f64, f64) {
    let street = Street::from_level_and_design(level, design).unwrap();
    let plan = street.junctions[&0].signal_plan.clone().unwrap();
    let start = plan.phases[..2]
        .iter()
        .map(|p| f64::from(p.green_s + plan.amber_s + plan.all_red_s))
        .sum();
    (plan.cycle_s(), start)
}

/// With a walk phase, walkers cross while every approach is red, no approach has green
/// during the walk phase, and careful drivers and walkers never touch (10 seeds).
#[test]
fn walkers_cross_in_the_walk_phase_and_nobody_is_hurt() {
    let level = level();
    let design = walk_template();
    let (cycle, walk_start) = walk_phase(&level, &design);
    let walk_end = walk_start + f64::from(WALK_S);
    let (mut walk_on_all_red, mut crossings) = (0, 0);
    for seed in 1..=10 {
        let mut run = run(&level, &design, seed, Population::Careful);
        let areas: Vec<usize> = (0..run.walk_space.areas.len())
            .filter(|a| run.walk_light(*a).is_some())
            .collect();
        assert_eq!(areas.len(), 4);
        while !run.finished() {
            let t = (run.tick as f64 * 0.05).rem_euclid(cycle);
            let in_walk_phase = t > walk_start + 0.2 && t < walk_end - 0.2;
            if in_walk_phase {
                for approach in 1..=4 {
                    let light = run.approach_light(0, approach).expect("signals");
                    assert!(
                        matches!(light, Light::Red(_)),
                        "seed {seed}, t {t}: approach {approach} shows {light:?}"
                    );
                }
                if areas.iter().all(|a| run.walk_light(*a) == Some(true)) {
                    walk_on_all_red += 1;
                }
            }
            run.step();
        }
        let summary = run.summary();
        assert_eq!(summary.contacts, 0, "seed {seed}: {summary:?}");
        assert_eq!(summary.walkers.contacts, 0, "seed {seed}: {summary:?}");
        assert_eq!(summary.walkers.on_red, 0, "seed {seed}");
        crossings += summary.walkers.crossings;
    }
    println!("{walk_on_all_red} ticks of walk on all four crossings, {crossings} crossings");
    assert!(walk_on_all_red > 100, "{walk_on_all_red}");
    assert!(crossings >= 50, "{crossings}");
}

/// Walkers do step off during the walk phase itself, not only around it.
#[test]
fn some_walkers_step_off_during_the_walk_phase() {
    let level = level();
    let design = walk_template();
    let (cycle, walk_start) = walk_phase(&level, &design);
    let mut in_phase = 0;
    for seed in 1..=5 {
        let mut run = run(&level, &design, seed, Population::Careful);
        run.run_to_end();
        for logged in run.events() {
            if matches!(logged.event, StreetEvent::CrossingStarted { .. }) {
                let t = (logged.tick as f64 * 0.05).rem_euclid(cycle);
                if t >= walk_start && t < walk_start + f64::from(WALK_S) {
                    in_phase += 1;
                }
            }
        }
    }
    println!("{in_phase} step-offs during the walk phase");
    assert!(in_phase > 0);
}
