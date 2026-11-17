//! Braking (pivot Phase 4, braking realism): the deceleration a driver needs is measured
//! over the distance left, not as the one-tick jump to a lower target speed, and braking
//! builds up over `BRAKE_RISE_S`.

use tf_sim::street::Design;
use tf_sim::street::ids::AgentId;
use tf_sim::street::level::{LevelFile, Mode};
use tf_sim::street::run::drive::{
    BRAKE_RISE_S, DT, HARD_BRAKE, SERIOUS_BRAKE, brake_build_up, draw_max_brake, needed_brake,
};
use tf_sim::street::run::{
    ConflictKind, Population, RunOptions, StreetEvent, StreetRun, is_walker,
};

/// A straight 30 km/h street with a zebra at x = 60 and busy cars and walkers.
fn level() -> LevelFile {
    let text = r#"(
        id: "brake-test", name: "Brake test", place: "test", difficulty: 1,
        bounds: (min: (0.0, 0.0), max: (160.0, 12.0)),
        nodes: [(id: 1, position: (0.0, 6.0)), (id: 2, position: (160.0, 6.0))],
        segments: [(id: 1, from: 1, to: 2, speed_limit_kmh: 30.0, profile: [
            (use: Sidewalk, width: 3.0, right: Kerb),
            (use: CarLane, width: 3.0, direction: Forward, left: Kerb),
            (use: CarLane, width: 3.0, direction: Backward, right: Kerb),
            (use: Sidewalk, width: 3.0, left: Kerb),
        ])],
        junctions: [],
        crossings: [(kind: Zebra, segment: 1, at: 60.0, width: 3.0)],
        gates: [
            (id: "west", position: (0.0, 6.0), segment: 1, access: 4),
            (id: "east", position: (160.0, 6.0), segment: 1, access: 4),
            (id: "north", position: (60.0, 0.0), segment: 1, access: 1),
            (id: "south", position: (60.0, 12.0), segment: 1, access: 1),
        ],
        demand: [(name: "test", duration_s: 150.0, flows: [
            (mode: Pedestrian, from: "north", to: "south", per_hour: 900),
            (mode: Pedestrian, from: "south", to: "north", per_hour: 900),
            (mode: Car, from: "west", to: "east", per_hour: 700),
            (mode: Car, from: "east", to: "west", per_hour: 700),
        ])],
        seed: 1,
    )"#;
    LevelFile::from_ron(text).expect("the micro-level parses")
}

#[test]
fn the_needed_brake_is_over_the_distance_left() {
    // 30 km/h, a stop 25 m ahead: v² / 2d, with half the build-up driven blind.
    let v = 30.0 / 3.6;
    let need = needed_brake(v, 0.0, 25.0);
    assert!(need < HARD_BRAKE && need > 1.0, "{need}");
    // The nearer the stop, the harder; nothing to do when slower than the target.
    assert!(needed_brake(v, 0.0, 10.0) > need);
    assert_eq!(needed_brake(3.0, 5.0, 10.0), 0.0);
    // No distance left: finite, and above any vehicle's limit.
    assert!(needed_brake(v, 0.0, 0.0).is_finite());
    assert!(needed_brake(v, 0.0, 0.0) > 9.5);
}

#[test]
fn braking_builds_up_and_lets_go_at_once() {
    let max = 8.0;
    let mut accel = 0.0;
    let mut ticks = 0;
    while accel > -max {
        accel = brake_build_up(accel, -max, max);
        ticks += 1;
        assert!(ticks < 100);
    }
    let seconds = f64::from(ticks) * DT;
    assert!(
        (seconds - BRAKE_RISE_S).abs() <= DT + 1e-9,
        "full braking after {seconds} s"
    );
    // Not braking harder than asked, and letting go is immediate.
    assert_eq!(brake_build_up(-2.0, -1.0, max), -1.0);
    assert_eq!(brake_build_up(-8.0, 1.0, max), 1.0);
}

#[test]
fn a_car_brakes_for_a_walker_at_a_zebra_without_a_hard_brake_and_stops_before_it() {
    // The kinematics of the case: 30 km/h, a walker on the zebra 25 m ahead (stop 1 m
    // before it), the brake applied as the sim does.
    let (mut speed, mut gap) = (30.0 / 3.6, 25.0 - 1.0);
    let (mut accel, mut worst) = (0.0, 0.0_f64);
    for _ in 0..400 {
        let wanted = -needed_brake(speed, 0.0, gap).max(0.0);
        accel = brake_build_up(accel, wanted.max(-8.0), 8.0);
        worst = worst.max(-accel);
        speed = (speed + accel * DT).max(0.0);
        gap -= speed * DT;
    }
    assert!(speed < 0.05, "stopped: {speed}");
    assert!(gap > -0.5, "short of the zebra: {gap}");
    assert!(worst < HARD_BRAKE, "braked at {worst} m/s²");
}

#[test]
fn careful_drivers_at_30_kmh_rarely_brake_seriously_for_walkers_and_never_hit_them() {
    let mut serious = 0;
    let level = level();
    for seed in 1..=4 {
        let options = RunOptions {
            seed,
            population: Population::Careful,
            demand_scale: 1.0,
        };
        let mut run = StreetRun::new(&level, &Design::new(&level.id), options).expect("builds");
        while !run.finished() {
            run.step();
        }
        for entry in run.events() {
            match &entry.event {
                StreetEvent::Conflict {
                    kind: ConflictKind::HardBrake,
                    a,
                    b,
                    measure,
                    ..
                } if (is_walker(*a) || is_walker(*b)) && *measure >= SERIOUS_BRAKE => {
                    serious += 1;
                }
                StreetEvent::Contact { .. } => panic!("seed {seed}: a contact"),
                _ => {}
            }
        }
    }
    // A walker who steps out right in front of a creeping car is a real near miss; the
    // old one-tick measure logged 3 to 8 of them per run (about one in two walkers).
    println!("serious brakes for walkers: {serious} in 4 runs");
    assert!(
        serious <= 4,
        "{serious} serious brakes for walkers in 4 runs"
    );
}

#[test]
fn the_maximum_brake_is_drawn_per_vehicle_in_its_range() {
    let (mut low, mut high) = (f64::INFINITY, 0.0_f64);
    for id in 1..200 {
        let car = draw_max_brake(Mode::Car, 7, AgentId(id));
        assert!((7.5..=9.5).contains(&car), "{car}");
        let van = draw_max_brake(Mode::Van, 7, AgentId(id));
        assert!((6.5..=8.0).contains(&van), "{van}");
        low = low.min(car);
        high = high.max(car);
    }
    assert!(high - low > 1.0, "the draws differ: {low}..{high}");
    assert_eq!(
        draw_max_brake(Mode::Car, 7, AgentId(3)),
        draw_max_brake(Mode::Car, 7, AgentId(3))
    );
}
