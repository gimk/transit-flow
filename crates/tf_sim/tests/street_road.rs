//! Walkers on the carriageway keep looking at the vehicles (pivot plan Phase 4, WP12):
//! they stop, step back or hurry for a car that does not slow down, and a rare assertive
//! walker keeps its pace for a car that can still stop.

use tf_sim::street::Design;
use tf_sim::street::level::LevelFile;
use tf_sim::street::run::walker::RoadAct;
use tf_sim::street::run::{Population, RunOptions, StreetRun};

/// A straight two-way street 160 m long, 12 m wide, doors at x = 60 on both sidewalks,
/// with a zebra at x = 60 when `zebra` is set.
fn street(kmh: f32, zebra: bool, flows: &str) -> LevelFile {
    let crossings = if zebra {
        "(kind: Zebra, segment: 1, at: 60.0, width: 3.0)"
    } else {
        ""
    };
    let text = format!(
        r#"(
        id: "road-test", name: "Road test", place: "test", difficulty: 1,
        bounds: (min: (0.0, 0.0), max: (160.0, 12.0)),
        nodes: [(id: 1, position: (0.0, 6.0)), (id: 2, position: (160.0, 6.0))],
        segments: [(id: 1, from: 1, to: 2, speed_limit_kmh: {kmh:?}, profile: [
            (use: Sidewalk, width: 3.0, right: Kerb),
            (use: CarLane, width: 3.0, direction: Forward, left: Kerb),
            (use: CarLane, width: 3.0, direction: Backward, right: Kerb),
            (use: Sidewalk, width: 3.0, left: Kerb),
        ])],
        junctions: [],
        crossings: [{crossings}],
        gates: [
            (id: "west", position: (0.0, 6.0), segment: 1, access: 4),
            (id: "east", position: (160.0, 6.0), segment: 1, access: 4),
            (id: "north", position: (60.0, 0.0), segment: 1, access: 1),
            (id: "south", position: (60.0, 12.0), segment: 1, access: 1),
        ],
        demand: [(name: "test", duration_s: 150.0, flows: [{flows}])],
        seed: 1,
    )"#
    );
    LevelFile::from_ron(&text).expect("the micro-level parses")
}

fn flow(mode: &str, from: &str, to: &str, per_hour: u32) -> String {
    format!(r#"(mode: {mode}, from: "{from}", to: "{to}", per_hour: {per_hour}),"#)
}

fn busy(kmh: f32, zebra: bool) -> LevelFile {
    let flows = flow("Pedestrian", "north", "south", 900)
        + &flow("Pedestrian", "south", "north", 900)
        + &flow("Car", "west", "east", 600)
        + &flow("Car", "east", "west", 600);
    street(kmh, zebra, &flows)
}

/// How many walker-ticks each reaction was seen in, and the run's summary.
#[derive(Default, Debug)]
struct Seen {
    stop: u32,
    back: u32,
    hurry: u32,
    hold: u32,
    /// The fastest a hurrying walker went, as a share of its desired speed.
    fastest: f64,
}

fn watch(
    level: &LevelFile,
    seed: u64,
    population: Population,
    assertive: bool,
) -> (Seen, StreetRun) {
    let options = RunOptions {
        seed,
        population,
        demand_scale: 1.0,
    };
    let mut run = StreetRun::new(level, &Design::new(&level.id), options).expect("it builds");
    let mut seen = Seen::default();
    while !run.finished() {
        if assertive {
            for walker in run.walkers.values_mut() {
                for draw in &mut walker.kerb_draws {
                    draw.assertive = 0.0;
                }
            }
        }
        run.step();
        for walker in run.walkers.values() {
            match walker.road_act {
                RoadAct::Free => {}
                RoadAct::Stop => seen.stop += 1,
                RoadAct::Back => seen.back += 1,
                RoadAct::Hold => seen.hold += 1,
                RoadAct::Hurry => {
                    seen.hurry += 1;
                    seen.fastest = seen.fastest.max(walker.speed() / walker.desired_speed);
                }
            }
        }
    }
    (seen, run)
}

#[test]
fn walkers_on_the_road_react_to_cars_that_do_not_slow_down() {
    let level = busy(50.0, false);
    let mut total = Seen::default();
    for seed in 1..=8 {
        let (seen, _) = watch(&level, seed, Population::Careless, false);
        total.stop += seen.stop;
        total.back += seen.back;
        total.hurry += seen.hurry;
        total.fastest = total.fastest.max(seen.fastest);
    }
    println!("careless drivers, free crossing: {total:?}");
    assert!(total.stop + total.back + total.hurry > 0, "{total:?}");
    assert!(total.hurry == 0 || total.fastest <= 1.5 * 1.15, "{total:?}");
}

#[test]
fn careful_drivers_never_make_walkers_run_or_touch_them() {
    let level = busy(30.0, true);
    let mut hurried = 0;
    for seed in 1..=6 {
        let (seen, run) = watch(&level, seed, Population::Careful, false);
        hurried += seen.hurry;
        let s = run.summary();
        assert_eq!(s.walkers.contacts + s.contacts, 0, "seed {seed}");
        assert_eq!(s.walkers.gave_up, 0, "seed {seed}");
    }
    assert_eq!(hurried, 0, "a car that yields does not make anyone run");
}

#[test]
fn an_assertive_walker_makes_a_yielding_car_wait() {
    // Mixed drivers: some brake early and some late, so a car comes that has not slowed
    // down yet but can still stop. Cars that yield wait for the walker on the zebra.
    let level = busy(50.0, true);
    let mut holds = 0;
    for seed in 1..=8 {
        let (seen, run) = watch(&level, seed, Population::Mixed, true);
        holds += seen.hold;
        let s = run.summary();
        assert_eq!(s.contacts + s.walkers.contacts, 0, "seed {seed}: {s:?}");
    }
    println!("assertive walkers kept their pace for {holds} walker-ticks");
    assert!(
        holds > 0,
        "somebody kept its pace for a car that could stop"
    );
}

#[test]
fn an_assertive_walker_still_reacts_to_a_car_that_cannot_stop() {
    let level = busy(50.0, false);
    let (mut reacted, mut holds) = (0, 0);
    for seed in 1..=8 {
        let (seen, _) = watch(&level, seed, Population::Careless, true);
        reacted += seen.stop + seen.back + seen.hurry;
        holds += seen.hold;
    }
    println!("assertive walkers, careless drivers: {holds} hold, {reacted} reacted");
    assert!(reacted > 0, "nobody walks into a sure hit on purpose");
}

#[test]
fn assertive_walkers_are_rare_among_those_who_react() {
    use std::collections::HashSet;
    let level = busy(50.0, false);
    let mut reacted: HashSet<(u64, i32)> = HashSet::new();
    let mut kept: HashSet<(u64, i32)> = HashSet::new();
    for seed in 1..=8 {
        let options = RunOptions {
            seed,
            population: Population::Careless,
            demand_scale: 1.0,
        };
        let mut run = StreetRun::new(&level, &Design::new(&level.id), options).expect("it builds");
        while !run.finished() {
            run.step();
            for walker in run.walkers.values() {
                let key = (seed, walker.id.0);
                match walker.road_act {
                    RoadAct::Free => {}
                    RoadAct::Hold => {
                        kept.insert(key);
                    }
                    _ => {
                        reacted.insert(key);
                    }
                }
            }
        }
    }
    let share = kept.len() as f64 / (kept.len() + reacted.len()).max(1) as f64;
    println!(
        "{} kept their pace, {} reacted: {:.1} %",
        kept.len(),
        reacted.len(),
        share * 100.0
    );
    assert!(share < 0.2, "assertive walkers are rare: {share}");
}
