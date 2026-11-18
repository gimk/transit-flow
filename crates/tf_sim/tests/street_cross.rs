//! Crossing streets (pivot plan Phase 3, stage B): where pedestrians cross (a zebra or
//! the desire line), waiting at the kerb for a gap, and pedestrian lights. Drivers do
//! not react to walkers yet (stage C), so these tests look only at what walkers decide.

use tf_sim::street::Design;
use tf_sim::street::lanes::Turn;
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

/// A careful walker never steps onto the zebra in front of a car that can neither pass
/// before it is across nor stop for it: every approaching car either reaches the zebra
/// after the walker's crossing time, at the speed it has, or can still stop before it
/// braking at a comfortable 3 m/s² (its driver has seen the walker at the kerb). Stage
/// C: drivers yield at a zebra, so walkers no longer wait for a full gap there; in
/// stage B this test asked for the full gap.
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
                        position,
                        ..
                    } => Some((e.tick, agent, crossing, waited_s, position)),
                    _ => None,
                })
                .collect();
            for &(tick, agent, crossing, waited, position) in &events[seen..] {
                assert_eq!(tick + 1, run.tick);
                let speed = run.walkers[&agent].desired_speed;
                assert_eq!(crossing, Some(1), "careful walkers use the zebra");
                starts += 1;
                waited_some += usize::from(waited > 0.5);
                // The band the walker crosses in: its body and a little room each side
                // of where it steps off. The carriageway is 6 m across.
                let (low, high) = (position.x - 1.0, position.x + 1.0);
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
                    // In the walker's band: only a car standing before the zebra (it
                    // yields) is allowed there.
                    let on_zebra = front.x > 58.5 && front.x < 61.5
                        || rear.x > 58.5 && rear.x < 61.5
                        || (front.x - 60.0) * (rear.x - 60.0) < 0.0;
                    assert!(
                        to_zebra > 0.0 || (car.speed < 0.3 && !on_zebra),
                        "seed {seed}, tick {tick}: car {:?} at {:?} ({} m/s) is in the way \
                         of walker {agent:?} at {position:?}",
                        car.id,
                        car.pos,
                        car.speed
                    );
                    if to_zebra <= 0.0 {
                        continue;
                    }
                    if car.speed > 0.3 {
                        let time = f64::from(to_zebra) / car.speed;
                        let v = car.speed;
                        let can_stop = f64::from(to_zebra) >= v * v / 6.0;
                        if !can_stop {
                            closest = closest.min(time);
                        }
                        assert!(
                            time >= crossing_s || can_stop,
                            "seed {seed}, tick {tick}: car {:?} reaches the zebra in \
                             {time:.2} s and cannot stop",
                            car.id
                        );
                    }
                }
            }
            seen = events.len();
        }
    }
    println!(
        "{starts} crossings, {waited_some} after a wait, closest car that could not stop \
         {closest:.2} s away"
    );
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

/// The approaches whose traffic drives over a crossing area, from the lanes' geometry:
/// (approaches whose own lane or straight movement passes through it, approaches whose
/// turning movement leaves through it).
fn approaches_over(run: &StreetRun, area: usize) -> (Vec<i32>, Vec<i32>) {
    let polygon = &run.walk_space.areas[area].polygon;
    let crosses = |points: &[glam::Vec2]| {
        points
            .windows(2)
            .any(|pair| (0..=10).any(|k| polygon.contains(pair[0].lerp(pair[1], k as f32 / 10.0))))
    };
    let (mut straight, mut turning) = (Vec::new(), Vec::new());
    for movement in run.street.movements(0) {
        let feeder = &run.street.lanes[&movement.feeder.expect("a feeder")];
        let out = &run.street.lanes[&movement.outgoing[0]];
        let approach = feeder.segment_id.expect("a road lane");
        let list = if crosses(&feeder.polyline())
            || (movement.turn == Turn::Straight && crosses(&out.polyline()))
        {
            &mut straight
        } else if crosses(&out.polyline()) {
            &mut turning
        } else {
            continue;
        };
        if !list.contains(&approach) {
            list.push(approach);
        }
    }
    (straight, turning)
}

/// At signals, no careful walker is still on a zebra when an approach whose traffic
/// drives straight over it gets green: walkers step off only when their own crossing
/// fits before that green (stage B review, finding 1). The approaches come from the
/// lanes, not from the run's own table.
#[test]
fn at_signals_careful_walkers_are_across_before_the_traffic_gets_green() {
    let level = signal_level();
    let mut checks = 0;
    for seed in 1..=3 {
        let mut run = run(&level, seed, Population::Careful, 2.0);
        let zebras: Vec<(usize, Vec<i32>)> = (0..run.walk_space.areas.len())
            .filter(|a| matches!(run.walk_space.areas[*a].kind, AreaKind::Crossing { .. }))
            .map(|a| (a, approaches_over(&run, a).0))
            .collect();
        let green = |run: &StreetRun, approach: i32| {
            matches!(run.approach_light(0, approach), Some(Light::Green(_)))
        };
        while !run.finished() {
            let before: Vec<Vec<bool>> = zebras
                .iter()
                .map(|(_, approaches)| approaches.iter().map(|a| green(&run, *a)).collect())
                .collect();
            run.step();
            for ((area, approaches), was) in zebras.iter().zip(&before) {
                for (approach, was_green) in approaches.iter().zip(was) {
                    if *was_green || !green(&run, *approach) {
                        continue;
                    }
                    checks += 1;
                    let polygon = &run.walk_space.areas[*area].polygon;
                    // A walker held still by cars queued over the zebra (cyclists slow the
                    // traffic and make such queues) did not misjudge the crossing.
                    for walker in run.walkers.values().filter(|w| w.vel.length() > 0.05) {
                        assert!(
                            !polygon.contains(walker.pos),
                            "seed {seed}, tick {}: {:?} is on zebra area {area} as approach \
                             {approach} turns green",
                            run.tick,
                            walker.id
                        );
                    }
                }
            }
        }
    }
    assert!(checks >= 10, "{checks} greens checked");
}

/// Crossing out of a zebra with traffic (stage B review, finding 3): the gap is measured
/// on the walker's own crossing line, so careful walkers never step in front of a car.
/// They wait long at 700 cars an hour each way: a careful walker needs about 6 s free in
/// both directions (the textbook delay for such a street is near a minute).
#[test]
fn careful_walkers_cross_a_busy_street_out_of_a_zebra_safely() {
    let flows = flow("Pedestrian", "north", "south", 600)
        + &flow("Car", "west", "east", 700)
        + &flow("Car", "east", "west", 700);
    let level = street(None, &flows, 150.0);
    let (mut starts, mut waited, mut arrived, mut spawned) = (0, 0, 0, 0);
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
                        position,
                        waited_s,
                        ..
                    } => Some((agent, crossing, position, waited_s)),
                    _ => None,
                })
                .collect();
            for &(agent, crossing, position, wait) in &events[seen..] {
                assert_eq!(crossing, None);
                starts += 1;
                waited += usize::from(wait > 0.5);
                let walker = &run.walkers[&agent];
                let crossing_s = 6.0 / walker.desired_speed;
                let (low, high) = (position.x - 0.6, position.x + 0.6);
                for car in run.agents.values() {
                    let half = car.dir * (car.length as f32 / 2.0);
                    let (front, rear) = (car.pos + half, car.pos - half);
                    let (to_line, past) = if car.dir.x > 0.0 {
                        (low - front.x, rear.x > high)
                    } else {
                        (front.x - high, rear.x < low)
                    };
                    if past {
                        continue;
                    }
                    assert!(to_line > 0.0, "seed {seed}: car {:?} is in the way", car.id);
                    if car.speed > 0.3 {
                        let time = f64::from(to_line) / car.speed;
                        assert!(
                            time >= crossing_s,
                            "seed {seed}: car {:?} is {time:.2} s away",
                            car.id
                        );
                    }
                }
            }
            seen = events.len();
        }
        let s = run.summary().walkers;
        arrived += s.arrived;
        spawned += s.spawned;
    }
    println!("{starts} crossings, {waited} after a wait, {arrived} of {spawned} arrived");
    assert!(waited >= 5, "traffic makes them wait");
    assert!(starts >= 40, "careful walkers do get across: {starts}");
}

/// People (the everyday mix) get across a street of 700 cars an hour each way out of a
/// zebra: a stated bound on those who give up.
#[test]
fn people_get_across_a_busy_street_out_of_a_zebra() {
    let flows = flow("Pedestrian", "north", "south", 600)
        + &flow("Car", "west", "east", 700)
        + &flow("Car", "east", "west", 700);
    let level = street(None, &flows, 150.0);
    let (mut arrived, mut spawned, mut gave_up, mut wait) = (0, 0, 0, 0.0);
    for seed in 1..=4 {
        let mut run = run(&level, seed, Population::Mixed, 1.0);
        run.run_to_end();
        let s = run.summary().walkers;
        arrived += s.arrived;
        spawned += s.spawned;
        gave_up += s.gave_up;
        wait += s.mean_kerb_wait_s * f64::from(s.arrived);
    }
    let wait = wait / f64::from(arrived);
    println!("mixed: {arrived} of {spawned} arrived, {gave_up} gave up, mean wait {wait:.1} s");
    assert!(gave_up * 10 <= spawned, "{gave_up} of {spawned} gave up");
}

/// The 50 m rule counts crossings of the same street on other segments, by position
/// (stage B review, finding 6): a zebra just past a junction, on the next segment of the
/// street, is 10 m from the desire line, so careful walkers use it.
#[test]
fn a_zebra_on_the_next_segment_of_the_street_counts_for_the_fifty_metre_rule() {
    let profile = r#"[
        (use: Sidewalk, width: 3.0, right: Kerb),
        (use: CarLane, width: 3.0, direction: Forward, left: Kerb),
        (use: CarLane, width: 3.0, direction: Backward, right: Kerb),
        (use: Sidewalk, width: 3.0, left: Kerb),
    ]"#;
    let text = format!(
        r#"(
        id: "two-segments", name: "Two segments", place: "test", difficulty: 1,
        bounds: (min: (0.0, 0.0), max: (160.0, 12.0)),
        nodes: [(id: 1, position: (0.0, 6.0)), (id: 2, position: (80.0, 6.0)),
                (id: 3, position: (160.0, 6.0))],
        segments: [
            (id: 1, name: "Rue", from: 1, to: 2, speed_limit_kmh: 30.0, profile: {profile}),
            (id: 2, name: "Rue", from: 2, to: 3, speed_limit_kmh: 30.0, profile: {profile}),
        ],
        junctions: [(node: 2, polygon: [(78.0, 3.0), (82.0, 3.0), (82.0, 9.0), (78.0, 9.0)])],
        plazas: [
            [(78.0, 0.0), (82.0, 0.0), (82.0, 3.0), (78.0, 3.0)],
            [(78.0, 9.0), (82.0, 9.0), (82.0, 12.0), (78.0, 12.0)],
        ],
        crossings: [(kind: Zebra, segment: 2, at: 6.0, width: 3.0)],
        gates: [
            (id: "north", position: (74.0, 0.0), segment: 1, access: 1),
            (id: "south", position: (74.0, 12.0), segment: 1, access: 1),
        ],
        demand: [(name: "test", duration_s: 60.0, flows: [
            (mode: Pedestrian, from: "north", to: "south", per_hour: 900),
        ])],
        seed: 1,
    )"#
    );
    let level = LevelFile::from_ron(&text).expect("the level parses");
    let mut run = run(&level, 1, Population::Careful, 1.0);
    run.run_to_end();
    let starts = crossings(&run);
    assert!(starts.len() >= 5, "{}", starts.len());
    assert!(
        starts
            .iter()
            .all(|(_, _, crossing, ..)| *crossing == Some(1)),
        "careful walkers use the zebra 10 m away: {starts:?}"
    );
}
