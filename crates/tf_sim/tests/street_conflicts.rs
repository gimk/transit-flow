//! Phase 6 W4b: the classic conflicts on the street, and the separator that decides them.
//! Each test is a micro-scenario on a straight street 160 m long, 14 m wide, with the strip
//! beside the eastbound car lane chosen by the test: a parking strip (no bicycle facility),
//! a painted bicycle lane, a protected track, or a parking strip with a buffer.

use tf_sim::street::Design;
use tf_sim::street::level::{LevelFile, Mode};
use tf_sim::street::run::events::{ConflictKind, Grade};
use tf_sim::street::run::{Population, RunOptions, StreetEvent, StreetRun};

/// A straight street 160 m long, 14 m wide, from west (node 1) to east (node 2). `profile`
/// is the cross-section from the north building line to the south one, 14 m in all; the
/// eastbound car lane is the one before the strip the test gives it. `crossings` and
/// `flows` are RON text.
fn street(profile: &str, crossings: &str, flows: &str, slice_s: f32) -> LevelFile {
    let text = format!(
        r#"(
        id: "conflict-test", name: "Conflict test", place: "test", difficulty: 1,
        bounds: (min: (0.0, 0.0), max: (160.0, 14.0)),
        nodes: [(id: 1, position: (0.0, 7.0)), (id: 2, position: (160.0, 7.0))],
        segments: [(id: 1, from: 1, to: 2, speed_limit_kmh: 30.0, profile: [{profile}])],
        junctions: [],
        crossings: [{crossings}],
        gates: [
            (id: "west", position: (0.0, 7.0), segment: 1, access: 23),
            (id: "east", position: (160.0, 7.0), segment: 1, access: 23),
            (id: "north", position: (60.0, 0.0), segment: 1, access: 1),
            (id: "south", position: (60.0, 14.0), segment: 1, access: 1),
        ],
        demand: [(name: "test", duration_s: {slice_s:?}, flows: [{flows}])],
        seed: 1,
    )"#
    );
    LevelFile::from_ron(&text).expect("the micro-level parses")
}

fn flow(mode: &str, from: &str, to: &str, per_hour: u32) -> String {
    format!(r#"(mode: {mode}, from: "{from}", to: "{to}", per_hour: {per_hour}),"#)
}

/// The cross-section of a street with no bicycle facility: the riders share the car lane,
/// beside a parking strip (the door zone).
const NO_FACILITY: &str = "
    (use: Sidewalk, width: 3.0, right: Kerb),
    (use: CarLane, width: 3.0, direction: Backward, left: Kerb, right: Paint),
    (use: CarLane, width: 3.0, direction: Forward, left: Paint),
    (use: Parking, width: 2.0),
    (use: Sidewalk, width: 3.0, left: Kerb),
";

/// A painted bicycle lane on the right of the eastbound car lane.
const PAINTED_LANE: &str = "
    (use: Sidewalk, width: 3.0, right: Kerb),
    (use: CarLane, width: 3.0, direction: Backward, left: Kerb, right: Paint),
    (use: CarLane, width: 3.0, direction: Forward, left: Paint, right: Paint),
    (use: BikeLane, width: 2.0, direction: Forward, left: Paint),
    (use: Sidewalk, width: 3.0, left: Kerb),
";

/// A protected track: the same lane behind a kerb.
const TRACK: &str = "
    (use: Sidewalk, width: 3.0, right: Kerb),
    (use: CarLane, width: 3.0, direction: Backward, left: Kerb, right: Paint),
    (use: CarLane, width: 3.0, direction: Forward, left: Paint, right: Kerb),
    (use: BikeTrack, width: 2.0, direction: Forward, left: Kerb),
    (use: Sidewalk, width: 3.0, left: Kerb),
";

/// A painted bicycle lane with a parking strip on its right: the door zone.
const LANE_BESIDE_PARKING: &str = "
    (use: Sidewalk, width: 3.0, right: Kerb),
    (use: CarLane, width: 3.0, direction: Backward, left: Kerb, right: Paint),
    (use: CarLane, width: 3.0, direction: Forward, left: Paint, right: Paint),
    (use: BikeLane, width: 1.5, direction: Forward, left: Paint, right: Paint),
    (use: Parking, width: 2.0),
    (use: Sidewalk, width: 1.5, left: Kerb),
";

/// The same, with a planting strip between the parking and the lane: a buffer.
const LANE_BEHIND_A_BUFFER: &str = "
    (use: Sidewalk, width: 3.0, right: Kerb),
    (use: CarLane, width: 3.0, direction: Backward, left: Kerb, right: Paint),
    (use: CarLane, width: 3.0, direction: Forward, left: Paint, right: Paint),
    (use: BikeLane, width: 1.5, direction: Forward, left: Paint, right: Paint),
    (use: Planting, width: 1.0),
    (use: Parking, width: 1.5),
    (use: Sidewalk, width: 1.0, left: Kerb),
";

fn run(level: &LevelFile, seed: u64, population: Population) -> StreetRun {
    let options = RunOptions {
        seed,
        population,
        demand_scale: 1.0,
    };
    StreetRun::new(level, &Design::new(&level.id), options).expect("the level builds")
}

/// Close passes and contacts between a motor vehicle and a rider, and the serious conflicts
/// of any kind with a rider on one side.
#[derive(Debug, Default, Clone, Copy)]
struct Counts {
    close_passes: u32,
    serious: u32,
    contacts: u32,
}

fn counts(run: &StreetRun) -> Counts {
    let mut mode_of = std::collections::HashMap::new();
    for e in run.events() {
        if let StreetEvent::Spawned { agent, mode, .. } = &e.event {
            mode_of.insert(*agent, *mode);
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
    let mut counts = Counts::default();
    for e in run.events() {
        match &e.event {
            StreetEvent::Conflict {
                kind, grade, a, b, ..
            } if car_rider(a, b) => {
                if *kind == ConflictKind::ClosePass {
                    counts.close_passes += 1;
                }
                if *grade == Grade::Serious {
                    counts.serious += 1;
                }
            }
            StreetEvent::Contact { a, b, .. } if car_rider(a, b) => counts.contacts += 1,
            _ => {}
        }
    }
    counts
}

/// In a shared lane, a careful driver waits behind a rider rather than pass it with less
/// than the metre the rule asks (R414-4); an impatient one squeezes past, which is a close
/// pass and sometimes a contact.
#[test]
fn a_careful_driver_waits_behind_a_rider_and_an_impatient_one_squeezes_past() {
    let flows = format!(
        "{}{}",
        flow("Car", "west", "east", 300),
        flow("Bicycle", "west", "east", 200)
    );
    let level = street(NO_FACILITY, "", &flows, 90.0);
    let mut careful = Counts::default();
    let mut careless = Counts::default();
    for seed in 1..=6 {
        let mut a = run(&level, seed, Population::Careful);
        a.run_to_end();
        let c = counts(&a);
        careful.close_passes += c.close_passes;
        careful.contacts += c.contacts;
        let mut b = run(&level, seed, Population::Careless);
        b.run_to_end();
        let c = counts(&b);
        careless.close_passes += c.close_passes;
        careless.contacts += c.contacts;
    }
    println!("careful {careful:?}\ncareless {careless:?}");
    assert_eq!(careful.close_passes, 0, "{careful:?}");
    assert_eq!(careful.contacts, 0, "{careful:?}");
    assert!(careless.close_passes > 0, "{careless:?}");
}

/// A van pulls over into a painted bicycle lane and stands there; the riders behind leave
/// the lane to go round it. A kerb track keeps the van out, so no rider has to leave.
#[test]
fn a_van_stops_in_a_painted_lane_and_not_in_a_track() {
    let flows = format!(
        "{}{}",
        flow("Van", "west", "east", 120),
        flow("Bicycle", "west", "east", 200)
    );
    let painted = street(PAINTED_LANE, "", &flows, 120.0);
    let track = street(TRACK, "", &flows, 120.0);
    let mut stopped_in_the_lane = false;
    let mut rider_left_the_lane = false;
    for seed in 1..=4 {
        let mut run = run(&painted, seed, Population::Mixed);
        while !run.finished() {
            run.step();
            stopped_in_the_lane |= run.agents.values().any(|a| {
                a.mode == Mode::Van && a.stop.is_some() && a.speed < 0.3 && a.lateral > 1.0
            });
            rider_left_the_lane |= run
                .agents
                .values()
                .any(|a| a.mode == Mode::Bicycle && a.lateral < -1.0);
        }
    }
    assert!(stopped_in_the_lane, "no van stopped in the painted lane");
    assert!(rider_left_the_lane, "no rider left the painted lane");
    // A track: the van cannot pull over, so it makes no stop there.
    for seed in 1..=4 {
        let mut run = run(&track, seed, Population::Mixed);
        while !run.finished() {
            run.step();
            assert!(
                !run.agents
                    .values()
                    .any(|a| a.mode == Mode::Van && a.stop.is_some()),
                "a van planned a stop in a track"
            );
        }
    }
}

/// A parked car opens its door into the bicycle lane; a rider in the door zone brakes for
/// it or leaves the lane. A buffer strip between the parking and the lane puts the door out
/// of reach.
#[test]
fn a_door_in_the_bicycle_lane_makes_a_rider_brake_and_a_buffer_removes_it() {
    let flows = format!(
        "{}{}",
        flow("Car", "west", "east", 200),
        flow("Bicycle", "west", "east", 1200)
    );
    let beside = street(LANE_BESIDE_PARKING, "", &flows, 180.0);
    let buffered = street(LANE_BEHIND_A_BUFFER, "", &flows, 180.0);
    let mut doors = 0;
    let mut braked = false;
    for seed in 1..=4 {
        let mut run = run(&beside, seed, Population::Mixed);
        while !run.finished() {
            run.step();
            doors += run.open_doors().len();
            for door in run.open_doors() {
                braked |= run.agents.values().any(|a| {
                    a.mode == Mode::Bicycle
                        && a.pos.distance(door.pos) < 10.0
                        && (a.speed < 3.0 || a.lateral < -1.0)
                });
            }
        }
    }
    assert!(doors > 0, "no door opened in the bicycle lane");
    assert!(braked, "no rider braked or left the lane for an open door");
    // A buffer strip: the parking strip has no travel strip beside it, so no door opens.
    for seed in 1..=4 {
        let mut run = run(&buffered, seed, Population::Mixed);
        while !run.finished() {
            run.step();
            assert!(
                run.open_doors().is_empty(),
                "a door opened behind a buffer strip"
            );
        }
    }
}

/// A driver that turns right cuts the painted bicycle lane before the corner; a kerb track
/// forces the turn at the corner.
#[test]
fn a_right_turn_cuts_a_painted_lane_and_a_track_forces_the_turn_at_the_corner() {
    use tf_sim::street::design::DesignCommand;
    use tf_sim::street::level::{Flow, Slice};
    use tf_sim::street::space::{Direction, StripUse};

    let mut level = LevelFile::load("carrefour-ecole").expect("the level loads");
    level.demand = vec![Slice {
        name: "test".to_string(),
        duration_s: 120.0,
        flows: vec![
            Flow {
                mode: Mode::Car,
                from: "west".to_string(),
                to: "south".to_string(),
                per_hour: 300,
            },
            Flow {
                mode: Mode::Bicycle,
                from: "west".to_string(),
                to: "east".to_string(),
                per_hour: 200,
            },
        ],
    }];
    let design = |use_: StripUse| {
        let mut design = Design::new(&level.id);
        design.commands = vec![DesignCommand::SetStripUse {
            segment: 1,
            strip: 3,
            use_,
            direction: Direction::Forward,
        }];
        design
    };
    let cut = |use_: StripUse| -> bool {
        let design = design(use_);
        for seed in 1..=4 {
            let options = RunOptions {
                seed,
                population: Population::Mixed,
                demand_scale: 1.0,
            };
            let mut run = StreetRun::new(&level, &design, options).expect("builds");
            while !run.finished() {
                run.step();
                let cutting = run.agents.values().any(|a| {
                    a.mode == Mode::Car
                        && a.lateral > 1.0
                        && a.approach
                            .as_ref()
                            .is_some_and(|approach| a.d < approach.line_d)
                });
                if cutting {
                    return true;
                }
            }
        }
        false
    };
    assert!(cut(StripUse::BikeLane), "no car cut the painted lane");
    assert!(!cut(StripUse::BikeTrack), "a car cut a kerb track");
}

/// People on foot cross the bicycle lane, at a zebra and away from one; riders that do not
/// yield meet them, and the conflicts are logged both ways.
#[test]
fn walkers_crossing_a_bicycle_lane_meet_riders() {
    let flows = format!(
        "{}{}",
        flow("Bicycle", "west", "east", 300),
        flow("Pedestrian", "north", "south", 400)
    );
    // The zebra is 40 m west of the desire line of the walkers: most of them cross the
    // bicycle lane away from it.
    let level = street(
        PAINTED_LANE,
        "(kind: Zebra, segment: 1, at: 20.0, width: 3.0)",
        &flows,
        120.0,
    );
    let mut with_walkers = 0;
    let mut contacts = 0;
    for seed in 1..=6 {
        let mut run = run(&level, seed, Population::Careless);
        run.run_to_end();
        let c = run.summary().cyclists;
        with_walkers += c.pet_with_walkers;
        contacts += c.contacts_with_walkers;
    }
    println!("pet with walkers {with_walkers}, contacts {contacts}");
    assert!(
        with_walkers + contacts > 0,
        "no conflict between a rider and a walker"
    );
}

/// Head-on close passes: a driver that squeezes past a rider into the lane of the oncoming
/// traffic and meets a vehicle there with little room (two motor vehicles in the event).
fn oncoming_close_passes(run: &StreetRun) -> u32 {
    let mut mode_of = std::collections::HashMap::new();
    for e in run.events() {
        if let StreetEvent::Spawned { agent, mode, .. } = &e.event {
            mode_of.insert(*agent, *mode);
        }
    }
    let motor = |id| matches!(mode_of.get(id), Some(Mode::Car | Mode::Van));
    run.events()
        .iter()
        .filter(|e| {
            matches!(&e.event, StreetEvent::Conflict { kind: ConflictKind::ClosePass, a, b, .. }
                if motor(a) && motor(b))
        })
        .count() as u32
}

/// Option (b) of the squeeze: a driver that squeezes past a rider also accepts a tight gap to
/// the oncoming traffic, so a shared lane is not safer than it looks. It shows as a close
/// pass between the two vehicles. A careful driver keeps the full rule and never does.
#[test]
fn an_impatient_driver_squeezes_past_a_rider_against_oncoming_traffic() {
    let flows = format!(
        "{}{}{}",
        flow("Car", "west", "east", 300),
        flow("Car", "east", "west", 300),
        flow("Bicycle", "west", "east", 200)
    );
    let level = street(NO_FACILITY, "", &flows, 90.0);
    let (mut careful, mut careless) = (0, 0);
    for seed in 1..=12 {
        let mut a = run(&level, seed, Population::Careful);
        a.run_to_end();
        careful += oncoming_close_passes(&a);
        let mut b = run(&level, seed, Population::Careless);
        b.run_to_end();
        careless += oncoming_close_passes(&b);
    }
    println!("oncoming close passes: careful {careful}, careless {careless}");
    assert_eq!(careful, 0);
    assert!(careless > 0);
}
