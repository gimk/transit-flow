//! Crossing streets (pivot plan Phase 3, stage B): where pedestrians cross (a zebra or
//! the desire line), waiting at the kerb for a gap, and pedestrian lights. Drivers do
//! not react to walkers yet (stage C), so these tests look only at what walkers decide.

use tf_sim::street::Design;
use tf_sim::street::level::{JunctionControl, LevelFile};
use tf_sim::street::run::walk::AreaKind;
use tf_sim::street::run::{Population, RunOptions, StreetEvent, StreetRun};
use tf_sim::street::signals::Light;

/// A straight two-way street 160 m long and 12 m wide (3 m sidewalks), with doors on
/// both sidewalks at x = 60 ("north", "south"), car gates at the ends, a zebra centred at
/// `zebra_at` (`None`: no zebra) and the given flows in one slice of `slice_s`.
fn street(zebra_at: Option<f32>, flows: &str, slice_s: f32) -> LevelFile {
    let crossings = zebra_at
        .map(|at| format!("(kind: Zebra, segment: 1, at: {at:?}, width: 3.0)"))
        .unwrap_or_default();
    let text = format!(
        r#"(
        id: "cross-test", name: "Cross test", place: "test", difficulty: 1,
        bounds: (min: (0.0, 0.0), max: (160.0, 12.0)),
        nodes: [(id: 1, position: (0.0, 6.0)), (id: 2, position: (160.0, 6.0))],
        segments: [(id: 1, from: 1, to: 2, speed_limit_kmh: 30.0, profile: [
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
        demand: [(name: "test", duration_s: {slice_s:?}, flows: [{flows}])],
        seed: 1,
    )"#
    );
    LevelFile::from_ron(&text).expect("the micro-level parses")
}

fn flow(mode: &str, from: &str, to: &str, per_hour: u32) -> String {
    format!(r#"(mode: {mode}, from: "{from}", to: "{to}", per_hour: {per_hour}),"#)
}

fn run(level: &LevelFile, seed: u64, population: Population, scale: f64) -> StreetRun {
    let options = RunOptions {
        seed,
        population,
        demand_scale: scale,
    };
    StreetRun::new(level, &Design::new(&level.id), options).expect("the level builds")
}

/// The steps off a kerb: (tick, position x, crossing, on red, waited).
fn crossings(run: &StreetRun) -> Vec<(u64, f32, Option<i32>, bool, f64)> {
    run.events()
        .iter()
        .filter_map(|e| match e.event {
            StreetEvent::CrossingStarted {
                crossing,
                position,
                on_red,
                waited_s,
                ..
            } => Some((e.tick, position.x, crossing, on_red, waited_s)),
            _ => None,
        })
        .collect()
}

/// The share of street crossings made out of a crossing, over seeds 1 to 6.
fn off_crossing_share(zebra_at: f32, population: Population) -> (f64, usize) {
    let walkers =
        flow("Pedestrian", "north", "south", 900) + &flow("Pedestrian", "south", "north", 900);
    let level = street(Some(zebra_at), &walkers, 120.0);
    let (mut off, mut all) = (0, 0);
    for seed in 1..=6 {
        let mut run = run(&level, seed, population, 1.0);
        run.run_to_end();
        for (_, _, crossing, _, _) in crossings(&run) {
            all += 1;
            off += usize::from(crossing.is_none());
        }
    }
    (off as f64 / all as f64, all)
}

/// The "done when" of Phase 3: a zebra 40 m from the desire line is ignored by a
/// measurable part of the people. Careful walkers (full compliance) use it, careless
/// ones mostly do not, and nobody ignores a zebra on the desire line.
#[test]
fn a_zebra_forty_metres_away_is_ignored_by_part_of_the_people() {
    let (mixed, n) = off_crossing_share(100.0, Population::Mixed);
    let (careful, _) = off_crossing_share(100.0, Population::Careful);
    let (careless, _) = off_crossing_share(100.0, Population::Careless);
    let (on_the_line, _) = off_crossing_share(60.0, Population::Mixed);
    println!(
        "off the zebra: mixed {mixed:.2} of {n}, careful {careful:.2}, careless {careless:.2}, \
         mixed with the zebra on the desire line {on_the_line:.2}"
    );
    assert!(n >= 100, "enough people cross: {n}");
    assert!((0.15..=0.45).contains(&mixed), "mixed {mixed:.2}");
    assert_eq!(careful, 0.0);
    assert!(careless >= 0.7, "careless {careless:.2}");
    assert_eq!(on_the_line, 0.0);
}

/// With no crossing within 50 m, crossing anywhere is legal: careful walkers cross on
/// the desire line too.
#[test]
fn without_a_crossing_nearby_everybody_crosses_where_they_are() {
    let walkers = flow("Pedestrian", "north", "south", 900);
    let level = street(None, &walkers, 60.0);
    let mut run = run(&level, 1, Population::Careful, 1.0);
    run.run_to_end();
    let starts = crossings(&run);
    assert!(starts.len() >= 5);
    assert!(
        starts
            .iter()
            .all(|(_, x, crossing, ..)| crossing.is_none() && (55.0..=65.0).contains(x))
    );
    assert_eq!(run.summary().walkers.arrived, run.summary().walkers.spawned);
}

#[test]
fn with_no_traffic_a_walker_does_not_wait_at_the_kerb() {
    let walkers = flow("Pedestrian", "north", "south", 600);
    let level = street(Some(60.0), &walkers, 60.0);
    let mut run = run(&level, 2, Population::Careful, 1.0);
    run.run_to_end();
    let starts = crossings(&run);
    assert!(starts.len() >= 5);
    for (_, _, crossing, on_red, waited) in starts {
        assert_eq!(crossing, Some(1));
        assert!(!on_red);
        assert_eq!(waited, 0.0, "nobody waits on an empty street");
    }
}

/// A careful walker waits for a gap and never steps off when a vehicle, at the speed it
/// has, would reach the crossing before the walker is across.
#[test]
fn a_careful_walker_waits_for_a_gap_and_never_starts_into_a_car() {
    let flows = flow("Pedestrian", "north", "south", 900)
        + &flow("Pedestrian", "south", "north", 900)
        + &flow("Car", "west", "east", 700)
        + &flow("Car", "east", "west", 700);
    let level = street(Some(60.0), &flows, 150.0);
    let (mut waited_some, mut starts) = (0, 0);
    let mut closest = f64::INFINITY;
    for seed in 1..=4 {
        let mut run = run(&level, seed, Population::Careful, 1.0);
        let mut seen = 0;
        while !run.finished() {
            run.step();
            let events: Vec<_> = run
                .events()
                .iter()
                .filter_map(|e| match e.event {
                    StreetEvent::CrossingStarted {
                        agent,
                        crossing,
                        waited_s,
                        ..
                    } => Some((e.tick, agent, crossing, waited_s)),
                    _ => None,
                })
                .collect();
            for &(tick, agent, crossing, waited) in &events[seen..] {
                assert_eq!(tick + 1, run.tick);
                let speed = run.walkers[&agent].desired_speed;
                assert_eq!(crossing, Some(1), "careful walkers use the zebra");
                starts += 1;
                waited_some += usize::from(waited > 0.5);
                // The zebra spans x 58.5 to 61.5; a walker's body adds a little. The
                // carriageway is 6 m across.
                let (low, high) = (58.5 - 0.3, 61.5 + 0.3);
                let crossing_s = 6.0 / speed;
                for car in run.agents.values() {
                    let half = car.dir * (car.length as f32 / 2.0);
                    let (front, rear) = (car.pos + half, car.pos - half);
                    let (to_zebra, past) = if car.dir.x > 0.0 {
                        (low - front.x, rear.x > high)
                    } else {
                        (front.x - high, rear.x < low)
                    };
                    if past {
                        continue;
                    }
                    assert!(
                        to_zebra > 0.0,
                        "seed {seed}, tick {tick}: car {:?} is on the zebra",
                        car.id
                    );
                    if car.speed > 0.3 {
                        let time = f64::from(to_zebra) / car.speed;
                        closest = closest.min(time);
                        assert!(
                            time >= crossing_s,
                            "seed {seed}, tick {tick}: car {:?} reaches the zebra in \
                             {time:.2} s",
                            car.id
                        );
                    }
                }
            }
            seen = events.len();
        }
    }
    println!("{starts} crossings, {waited_some} after a wait, closest car {closest:.2} s away");
    assert!(starts >= 40, "{starts}");
    assert!(waited_some >= 5, "traffic makes some wait: {waited_some}");
}

fn signal_level() -> LevelFile {
    let mut level = LevelFile::load("carrefour-ecole").expect("the test level loads");
    level.junctions[0].control = JunctionControl::Signals;
    level
}

/// At signals, careful walkers step off only on their walk light, and a walk light is
/// never on while an approach whose traffic drives over the crossing has green or amber.
#[test]
fn at_signals_careful_walkers_cross_on_their_green_only() {
    let level = signal_level();
    for seed in 1..=3 {
        let mut run = run(&level, seed, Population::Careful, 2.0);
        let signal_areas: Vec<(usize, i32)> = run
            .walk_space
            .areas
            .iter()
            .enumerate()
            .filter_map(|(index, area)| match area.kind {
                AreaKind::Crossing {
                    id,
                    signal: Some(_),
                    ..
                } => Some((index, id)),
                _ => None,
            })
            .collect();
        assert_eq!(signal_areas.len(), 4, "every arm's zebra has lights");
        let (mut walks, mut seen) = (0, 0);
        while !run.finished() {
            // The lights of this tick, before it is stepped.
            let lights: Vec<(i32, bool)> = signal_areas
                .iter()
                .map(|&(area, id)| (id, run.walk_light(area).expect("a light")))
                .collect();
            for &(area, _) in &signal_areas {
                if run.walk_light(area) == Some(true) {
                    let (node, conflicting) = run.walk_conflicts(area).expect("a light");
                    for approach in conflicting {
                        let light = run.approach_light(node, *approach).expect("signals");
                        assert!(
                            matches!(light, Light::Red(_)),
                            "seed {seed}, tick {}: walk while approach {approach} shows \
                             {light:?}",
                            run.tick
                        );
                    }
                }
            }
            run.step();
            let events = crossings(&run);
            for &(_, _, crossing, on_red, _) in &events[seen..] {
                let id = crossing.expect("careful walkers use the zebras");
                assert!(!on_red, "seed {seed}: a careful walker crossed on red");
                let walk = lights.iter().find(|(c, _)| *c == id).expect("lights").1;
                assert!(walk, "seed {seed}: crossing {id} stepped on without walk");
                walks += 1;
            }
            seen = events.len();
        }
        println!("seed {seed}: {walks} crossings on walk");
        assert!(walks >= 10, "seed {seed}: {walks}");
    }
}

#[test]
fn some_careless_walkers_cross_on_red() {
    let level = signal_level();
    let on_red: u32 = (1..=3)
        .map(|seed| {
            let mut run = run(&level, seed, Population::Careless, 2.0);
            run.run_to_end();
            run.summary().walkers.on_red
        })
        .sum();
    println!("{on_red} careless crossings on red");
    assert!(on_red > 0);
}

#[test]
fn crossings_are_deterministic() {
    let flows = flow("Pedestrian", "north", "south", 900)
        + &flow("Car", "west", "east", 700)
        + &flow("Car", "east", "west", 700);
    let level = street(Some(100.0), &flows, 60.0);
    let mut a = run(&level, 3, Population::Mixed, 1.0);
    let mut b = run(&level, 3, Population::Mixed, 1.0);
    a.run_to_end();
    b.run_to_end();
    assert_eq!(a.log_hash(), b.log_hash());
    assert_eq!(a.events(), b.events());
}
