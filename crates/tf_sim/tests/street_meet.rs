//! Cars against people on foot (pivot plan Phase 3, stage C): drivers see walkers (or do
//! not, behind a van), yield at zebras, approach a hidden zebra slowly; contacts have a
//! severity and a cause chain, and a death ends the run.

use glam::Vec2;
use tf_sim::rng::SimRng;
use tf_sim::street::Design;
use tf_sim::street::level::{JunctionControl, LevelFile, Mode};
use tf_sim::street::run::events::{
    CauseKind, EndReason, Outcome, PEDESTRIAN_SEVERITY, draw_outcome, severity_at,
};
use tf_sim::street::run::walk::AreaKind;
use tf_sim::street::run::{Population, RunOptions, StreetEvent, StreetRun, is_walker};

/// A straight street 160 m long: north sidewalk (y 0 to 3), a parking strip (3 to 5), an
/// eastbound lane (5 to 8), a westbound lane (8 to 11), south sidewalk (11 to 14). Doors
/// at x = 60 on both sidewalks; a zebra centred at x = 60; `obstacles` and `flows` as
/// given; a speed limit of `kmh`.
fn street(kmh: f32, zebra: bool, obstacles: &str, flows: &str, slice_s: f32) -> LevelFile {
    let crossings = if zebra {
        "(kind: Zebra, segment: 1, at: 60.0, width: 3.0)"
    } else {
        ""
    };
    let text = format!(
        r#"(
        id: "meet-test", name: "Meet test", place: "test", difficulty: 1,
        bounds: (min: (0.0, 0.0), max: (160.0, 14.0)),
        nodes: [(id: 1, position: (0.0, 7.0)), (id: 2, position: (160.0, 7.0))],
        segments: [(id: 1, from: 1, to: 2, speed_limit_kmh: {kmh:?}, profile: [
            (use: Sidewalk, width: 3.0, right: Kerb),
            (use: Parking, width: 2.0, left: Kerb),
            (use: CarLane, width: 3.0, direction: Forward),
            (use: CarLane, width: 3.0, direction: Backward, right: Kerb),
            (use: Sidewalk, width: 3.0, left: Kerb),
        ])],
        junctions: [],
        obstacles: [{obstacles}],
        crossings: [{crossings}],
        gates: [
            (id: "west", position: (0.0, 7.0), segment: 1, access: 4),
            (id: "east", position: (160.0, 7.0), segment: 1, access: 4),
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

fn run(level: &LevelFile, seed: u64, population: Population, scale: f64) -> StreetRun {
    let options = RunOptions {
        seed,
        population,
        demand_scale: scale,
    };
    StreetRun::new(level, &Design::new(&level.id), options).expect("the level builds")
}

/// Contacts between a vehicle and a walker: (causes, outcome).
fn walker_contacts(run: &StreetRun) -> Vec<(Vec<CauseKind>, Outcome)> {
    run.events()
        .iter()
        .filter_map(|e| match &e.event {
            StreetEvent::Contact {
                b, causes, outcome, ..
            } if is_walker(*b) => Some((causes.iter().map(|c| c.what).collect(), *outcome)),
            _ => None,
        })
        .collect()
}

/// No walker's centre is inside a vehicle body unless a contact between the two was
/// logged (stage C review, finding 3).
fn assert_nobody_inside_a_vehicle(run: &StreetRun, what: &str) {
    for car in run.agents.values() {
        for walker in run.walkers.values() {
            let local = walker.pos - car.pos;
            let inside = local.dot(car.dir).abs() < (car.length as f32) / 2.0
                && local.dot(car.dir.perp()).abs() < (car.width as f32) / 2.0;
            if !inside {
                continue;
            }
            let touched = run.events().iter().any(|e| {
                matches!(e.event, StreetEvent::Contact { a, b, .. }
                    if a == car.id && b == walker.id)
            });
            assert!(
                touched,
                "{what}, tick {}: {:?} is inside {:?} with no contact",
                run.tick, walker.id, car.id
            );
        }
    }
}

/// Steps a run to its end, checking `assert_nobody_inside_a_vehicle` every tick.
fn run_checked(mut run: StreetRun, what: &str) -> StreetRun {
    while !run.finished() {
        run.step();
        assert_nobody_inside_a_vehicle(&run, what);
    }
    run
}

/// A van parked in the parking strip just before the zebra, for eastbound drivers: it
/// hides the north kerb of the zebra, where people wait to cross.
const VAN: &str =
    "(kind: ParkedVan, shape: Polygon([(52.0, 3.2), (57.8, 3.2), (57.8, 4.8), (52.0, 4.8)])),";

fn hidden_zebra(kmh: f32) -> LevelFile {
    let flows = flow("Pedestrian", "north", "south", 900)
        + &flow("Car", "west", "east", 600)
        + &flow("Car", "east", "west", 300);
    street(kmh, true, VAN, &flows, 150.0)
}

/// The "done when" of Phase 3: a hidden pedestrian can be hit by an inattentive driver
/// and never by a careful one at 30 km/h. People (children among the mixed population)
/// step out onto the zebra from behind a parked van.
#[test]
fn a_hidden_pedestrian_is_never_hit_by_a_careful_driver_at_30() {
    let level = hidden_zebra(30.0);
    let mut crossings = 0;
    for seed in 1..=20 {
        let run = run_checked(run(&level, seed, Population::Careful, 1.0), "careful");
        let s = run.summary();
        crossings += s.walkers.crossings;
        assert_eq!(s.walkers.contacts, 0, "seed {seed}: {:?}", s.walkers);
        assert_eq!(s.contacts, 0, "seed {seed}");
    }
    println!("careful: {crossings} crossings behind the van, no contact");
    assert!(crossings >= 500);
}

#[test]
fn a_hidden_pedestrian_can_be_hit_by_an_inattentive_driver() {
    let level = hidden_zebra(30.0);
    let mut contacts = 0;
    for seed in 1..=20 {
        let run = run_checked(run(&level, seed, Population::Careless, 1.0), "careless");
        for (causes, _) in walker_contacts(&run) {
            contacts += 1;
            // Every contact names what the hidden zebra is about: the walker hidden from
            // the driver, the look not made (at the kerb, or away: a distraction), the
            // speed too high for a hidden kerb, or the yield not made (stage C review,
            // finding 5).
            assert!(
                causes.iter().any(|c| matches!(
                    c,
                    CauseKind::HiddenWalker { .. }
                        | CauseKind::DidNotLook { .. }
                        | CauseKind::Distracted
                        | CauseKind::TooFastForSight { .. }
                        | CauseKind::DidNotYieldToWalker { .. }
                )),
                "seed {seed}: a contact names neither the hidden walker nor the look: \
                 {causes:?}"
            );
        }
    }
    println!("careless: {contacts} contacts, all naming the hidden walker or the look");
    assert!(contacts > 0, "careless drivers hit someone over 20 seeds");
}

/// A careful driver stops for people on the zebra or waiting at it and goes when it is
/// free: nobody waits for ever (no deadlock between a waiting walker and a waiting
/// driver), even with a queue of cars at the zebra.
#[test]
fn a_careful_driver_stops_at_a_zebra_and_nobody_waits_for_ever() {
    let flows = flow("Pedestrian", "north", "south", 600)
        + &flow("Pedestrian", "south", "north", 600)
        + &flow("Car", "west", "east", 700)
        + &flow("Car", "east", "west", 700);
    let level = street(30.0, true, "", &flows, 150.0);
    let (mut yielded, mut queued) = (0, 0);
    for seed in 1..=6 {
        let mut run = run(&level, seed, Population::Careful, 1.0);
        let zebra = (0..run.walk_space.areas.len())
            .find(|a| matches!(run.walk_space.areas[*a].kind, AreaKind::Crossing { .. }))
            .expect("the zebra");
        while !run.finished() {
            run.step();
            assert_nobody_inside_a_vehicle(&run, "zebra queue");
            let polygon = &run.walk_space.areas[zebra].polygon;
            let on_zebra = run.walkers.values().any(|w| polygon.contains(w.pos));
            let standing: Vec<_> = run
                .agents
                .values()
                .filter(|car| car.speed < 0.3)
                .filter(|car| {
                    let front = car.pos + car.dir * (car.length as f32 / 2.0);
                    (front.x - 60.0).abs() < 6.0
                })
                .collect();
            if on_zebra && !standing.is_empty() {
                yielded += 1;
            }
            if run.agents.values().filter(|car| car.speed < 0.3).count() >= 3 {
                queued += 1;
            }
        }
        let s = run.summary();
        assert_eq!(s.contacts + s.walkers.contacts, 0, "seed {seed}");
        assert_eq!(s.gave_up, 0, "seed {seed}: {s:?}");
        assert_eq!(s.walkers.gave_up, 0, "seed {seed}: {:?}", s.walkers);
    }
    println!("{yielded} ticks of cars standing for people on the zebra, {queued} with a queue");
    assert!(yielded > 100, "cars stop for people on the zebra");
    assert!(queued > 100, "queues form at the zebra");
}

fn signal_level(signals: bool) -> LevelFile {
    let mut level = LevelFile::load("carrefour-ecole").expect("the test level loads");
    if signals {
        level.junctions[0].control = JunctionControl::Signals;
    }
    level
}

/// Full compliance and attention on both sides: no contact (between a vehicle and a
/// walker, or two vehicles) over 50 seeds, at the level's demand and at four times it,
/// with and without signals. Drivers who yield to steady streams of people at the zebras
/// past the junction exits, on top of priority to the right, can starve an approach:
/// at most `GIVE_UP_BOUND` of the cars, and of the walkers, give up (the user's decision
/// of 2026-11-16; the Phase 2 tests of drivers among themselves run without walkers).
#[test]
fn careful_drivers_and_walkers_never_touch() {
    const GIVE_UP_BOUND: f64 = 0.01;
    for signals in [false, true] {
        for scale in [1.0, 4.0] {
            let level = signal_level(signals);
            let (mut cars, mut cars_gave_up, mut walkers, mut walkers_gave_up) = (0, 0, 0, 0);
            for seed in 1..=50 {
                let mut run = run(&level, seed, Population::Careful, scale);
                run.run_to_end();
                let s = run.summary();
                assert_eq!(
                    s.walkers.contacts, 0,
                    "signals {signals}, x{scale}, seed {seed}: {:?}",
                    s.walkers
                );
                assert_eq!(s.contacts, 0, "signals {signals}, x{scale}, seed {seed}");
                cars += s.spawned;
                cars_gave_up += s.gave_up;
                walkers += s.walkers.spawned;
                walkers_gave_up += s.walkers.gave_up;
            }
            println!(
                "signals {signals}, x{scale}: cars gave up {cars_gave_up} of {cars}, walkers \
                 {walkers_gave_up} of {walkers}"
            );
            if !signals && scale > 1.0 {
                // Priority to the right at four times the demand starves the north street
                // with or without walkers (the Phase 2 finding the user accepted): no bound
                // on the cars there.
            } else {
                assert!(
                    f64::from(cars_gave_up) <= GIVE_UP_BOUND * f64::from(cars),
                    "signals {signals}, x{scale}: {cars_gave_up} of {cars} cars gave up"
                );
            }
            assert!(
                f64::from(walkers_gave_up) <= GIVE_UP_BOUND * f64::from(walkers),
                "signals {signals}, x{scale}: {walkers_gave_up} of {walkers} walkers gave up"
            );
        }
    }
}

/// No vehicle is on a zebra while a walker is in its path (in front of it, within its
/// width), with careful drivers and walkers (stage B review, finding 2: the movements
/// that cross a zebra come from the geometry, here the bodies themselves).
#[test]
fn careful_drivers_are_never_on_a_zebra_with_a_walker_in_their_path() {
    let mut checked = 0;
    for signals in [false, true] {
        let level = signal_level(signals);
        for seed in 1..=4 {
            let mut run = run(&level, seed, Population::Careful, 2.0);
            let zebras: Vec<usize> = (0..run.walk_space.areas.len())
                .filter(|a| matches!(run.walk_space.areas[*a].kind, AreaKind::Crossing { .. }))
                .collect();
            while !run.finished() {
                run.step();
                for &zebra in &zebras {
                    let polygon = &run.walk_space.areas[zebra].polygon;
                    for car in run.agents.values() {
                        let half = car.dir * (car.length as f32 / 2.0);
                        let on = [car.pos - half, car.pos, car.pos + half]
                            .iter()
                            .any(|p| polygon.contains(*p));
                        if !on || car.speed < 0.3 {
                            continue;
                        }
                        for walker in run.walkers.values() {
                            if !polygon.contains(walker.pos) {
                                continue;
                            }
                            checked += 1;
                            let local: Vec2 = walker.pos - car.pos;
                            let along = local.dot(car.dir);
                            let side = local.dot(car.dir.perp()).abs();
                            let in_path =
                                along > 0.0 && side < (car.width as f32) / 2.0 + walker.radius;
                            assert!(
                                !in_path,
                                "signals {signals}, seed {seed}, tick {}: car {:?} drives \
                                 on a zebra with {:?} in front of it",
                                run.tick, car.id, walker.id
                            );
                        }
                    }
                }
            }
        }
    }
    println!("{checked} car-walker pairs on a zebra checked");
}

/// Careless drivers and walkers meet: conflicts and contacts, and every contact has a
/// cause chain.
#[test]
fn careless_people_have_conflicts_and_contacts_with_causes() {
    let level = signal_level(false);
    let (mut contacts, mut conflicts) = (0, 0);
    for seed in 1..=10 {
        let run = run_checked(run(&level, seed, Population::Careless, 2.0), "careless");
        for (causes, _) in walker_contacts(&run) {
            contacts += 1;
            assert!(!causes.is_empty(), "seed {seed}: a contact without causes");
        }
        conflicts += run
            .events()
            .iter()
            .filter(|e| {
                matches!(&e.event, StreetEvent::Conflict { a, b, .. }
                    if is_walker(*a) || is_walker(*b))
            })
            .count();
    }
    println!("careless: {contacts} car-walker contacts, {conflicts} car-walker conflicts");
    assert!(contacts > 0);
    assert!(conflicts > contacts);
}

/// The outcomes drawn for a person on foot follow the curves (Rosén & Sander 2009 for
/// death, Tefft 2011 for serious injury).
#[test]
fn walker_injuries_follow_the_severity_curves() {
    for kmh in [20.0, 30.0, 50.0] {
        let (light, serious, death) = severity_at(PEDESTRIAN_SEVERITY, kmh);
        let n = 40_000;
        let mut counts = [0u32; 4];
        for k in 0..n {
            let mut rng = SimRng::new(1000 + k as u64);
            counts[match draw_outcome(PEDESTRIAN_SEVERITY, kmh, &mut rng) {
                Outcome::NoInjury => 0,
                Outcome::LightInjury => 1,
                Outcome::SeriousInjury => 2,
                Outcome::Death => 3,
            }] += 1;
        }
        let share = |c: u32| f64::from(c) / f64::from(n);
        let at_least_light = share(counts[1] + counts[2] + counts[3]);
        let at_least_serious = share(counts[2] + counts[3]);
        let dead = share(counts[3]);
        println!(
            "{kmh} km/h: light+ {at_least_light:.3} ({light:.3}), serious+ \
             {at_least_serious:.3} ({serious:.3}), death {dead:.4} ({death:.4})"
        );
        assert!((at_least_light - light).abs() < 0.01);
        assert!((at_least_serious - serious).abs() < 0.01);
        assert!((dead - death).abs() < 0.005);
    }
}

/// A death ends the run, with the death as its reason, and nothing happens after it.
#[test]
fn a_death_ends_the_run() {
    let flows = flow("Pedestrian", "north", "south", 1800)
        + &flow("Car", "west", "east", 900)
        + &flow("Car", "east", "west", 900);
    let level = street(50.0, false, "", &flows, 300.0);
    let mut found = false;
    for seed in 1..=40 {
        let mut run = run(&level, seed, Population::Careless, 1.0);
        run.run_to_end();
        if run.ended() != Some(EndReason::Death) {
            continue;
        }
        found = true;
        let events = run.events();
        let death = events
            .iter()
            .position(|e| {
                matches!(e.event, StreetEvent::Contact { b, outcome: Outcome::Death, .. }
                    if is_walker(b))
            })
            .expect("a walker died");
        let last = events.last().expect("events");
        assert_eq!(
            last.event,
            StreetEvent::RunEnded {
                reason: EndReason::Death
            }
        );
        // Nothing after the tick of the death, but the end of the run (logged at the
        // next tick, as for a death between vehicles).
        let tick = events[death].tick;
        assert!(
            events[death..events.len() - 1]
                .iter()
                .all(|e| e.tick == tick)
        );
        assert_eq!(last.tick, tick + 1);
        println!("seed {seed}: a death at tick {} ended the run", last.tick);
        break;
    }
    assert!(
        found,
        "careless drivers at 50 km/h kill someone within 40 seeds"
    );
}

#[test]
fn meetings_are_deterministic() {
    let level = hidden_zebra(30.0);
    let mut a = run(&level, 3, Population::Careless, 1.0);
    let mut b = run(&level, 3, Population::Careless, 1.0);
    a.run_to_end();
    b.run_to_end();
    assert_eq!(a.events(), b.events());
}

/// A careless driver who drives onto a zebra while someone has been on it for a while and
/// is not past its path (a careful driver would have seen it and stopped before the
/// zebra) is blamed for not
/// yielding or not looking: the yield a careful driver would have made stays a pending
/// cause until the front enters the zebra (stage C review, finding 1).
#[test]
fn not_yielding_at_a_zebra_is_named_when_the_car_enters_it() {
    let flows = flow("Pedestrian", "north", "south", 1800)
        + &flow("Pedestrian", "south", "north", 1800)
        + &flow("Car", "west", "east", 700)
        + &flow("Car", "east", "west", 700);
    let level = street(50.0, true, "", &flows, 150.0);
    let mut checked = 0;
    for seed in 1..=20 {
        let mut run = run(&level, seed, Population::Careless, 1.0);
        let zebra = (0..run.walk_space.areas.len())
            .find(|a| matches!(run.walk_space.areas[*a].kind, AreaKind::Crossing { .. }))
            .expect("the zebra");
        // (tick to check at, car). A point 1 m behind the front: the run finds where a
        // route enters the zebra every 0.5 m, so the route's front is in by then, and the
        // next decision records the causes.
        let mut due: Vec<(u64, tf_sim::street::AgentId)> = Vec::new();
        while !run.finished() {
            run.step();
            let polygon = run.walk_space.areas[zebra].polygon.clone();
            let now = run.tick;
            for car in run.agents.values() {
                let reach = car.length as f32 / 2.0 - 1.0;
                let front = car.pos + car.dir * reach;
                let before = car.prev_pos + car.prev_dir * reach;
                if !polygon.contains(front) || polygon.contains(before) || car.speed < 1.0 {
                    continue;
                }
                let on_for_long = run.walkers.values().any(|w| {
                    let on_for = run.events().iter().rev().find_map(|e| match e.event {
                        StreetEvent::CrossingStarted { agent, .. } if agent == w.id => {
                            Some((now - e.tick) as f64 * 0.05)
                        }
                        _ => None,
                    });
                    // Not past the car's path: within its reach (half a car, a body, a
                    // margin, and `PASSED_MARGIN`), or coming toward its line.
                    let side = (w.pos - front).dot(car.dir.perp());
                    let coming = w.vel.dot(car.dir.perp()) * side < 0.0;
                    let not_past = side.abs() < 0.9 + w.radius + 0.3 + 1.0 || coming;
                    polygon.contains(w.pos) && not_past && on_for.is_some_and(|s| s >= 2.0)
                });
                if on_for_long {
                    due.push((now + 2, car.id));
                }
            }
            for &(_, id) in due.iter().filter(|(at, _)| *at == now) {
                let Some(car) = run.agents.get(&id) else {
                    continue;
                };
                checked += 1;
                assert!(
                    car.recent.iter().any(|(_, c)| matches!(
                        c,
                        CauseKind::DidNotYieldToWalker { .. }
                            | CauseKind::DidNotLook { .. }
                            | CauseKind::Distracted
                    )),
                    "seed {seed}, tick {now}: {id:?} drove onto the zebra in front of someone \
                     with no cause: {:?}",
                    car.recent
                );
            }
        }
    }
    println!("{checked} cars drove onto a zebra with someone on it");
    assert!(checked > 0);
}

/// A careful driver never stops for ever before a zebra whose kerb stays hidden: it
/// creeps until it sees the kerb (stage C review, finding 2). Two scenes: a parked van
/// right at the zebra's edge, and a van standing in the next lane (eastbound traffic
/// only) that hides the far kerb.
#[test]
fn a_careful_driver_creeps_past_a_kerb_that_stays_hidden() {
    let at_the_edge =
        "(kind: ParkedVan, shape: Polygon([(52.7, 3.2), (58.5, 3.2), (58.5, 4.8), (52.7, 4.8)])),";
    let in_the_next_lane = "(kind: ParkedVan, shape: Polygon([(52.7, 8.2), (58.5, 8.2), (58.5, 10.8), (52.7, 10.8)])),";
    for (scene, van, walkers) in [
        ("van at the zebra's edge", at_the_edge, "north"),
        ("van in the next lane", in_the_next_lane, "south"),
    ] {
        let to = if walkers == "north" { "south" } else { "north" };
        let flows = flow("Pedestrian", walkers, to, 900) + &flow("Car", "west", "east", 600);
        let level = street(30.0, true, van, &flows, 150.0);
        let mut arrived = 0;
        for seed in 1..=6 {
            let run = run_checked(run(&level, seed, Population::Careful, 1.0), scene);
            let s = run.summary();
            assert_eq!(s.gave_up, 0, "{scene}, seed {seed}: {s:?}");
            assert_eq!(
                s.walkers.gave_up, 0,
                "{scene}, seed {seed}: {:?}",
                s.walkers
            );
            assert_eq!(s.contacts + s.walkers.contacts, 0, "{scene}, seed {seed}");
            arrived += s.arrived;
        }
        println!("{scene}: {arrived} cars got past");
        assert!(arrived >= 50, "{scene}: {arrived}");
    }
}

/// A car standing at its junction line for people on the zebra before the line is held
/// by them: the drivers who give way to it do not wait for it (stage C review, finding
/// 4).
#[test]
fn a_car_held_by_people_on_the_zebra_before_its_line_says_so() {
    use tf_sim::street::run::drive::AT_LINE;
    let mut checked = 0;
    for signals in [false, true] {
        let level = signal_level(signals);
        for seed in 1..=4 {
            let mut run = run(&level, seed, Population::Careful, 2.0);
            let zebras: Vec<usize> = (0..run.walk_space.areas.len())
                .filter(|a| matches!(run.walk_space.areas[*a].kind, AreaKind::Crossing { .. }))
                .collect();
            while !run.finished() {
                run.step();
                for car in run.agents.values() {
                    let Some(approach) = &car.approach else {
                        continue;
                    };
                    // Cars: a rider held by people on foot is not flagged (open point, Phase 6).
                    if car.mode == Mode::Bicycle
                        || approach.committed
                        || car.speed > 0.05
                        || approach.line_d - car.d >= AT_LINE
                    {
                        continue;
                    }
                    // Someone on a zebra right in front of it, in its lane.
                    let front = car.pos + car.dir * (car.length as f32 / 2.0);
                    let blocked = run.walkers.values().any(|w| {
                        let local = w.pos - front;
                        let ahead = local.dot(car.dir);
                        zebras
                            .iter()
                            .any(|z| run.walk_space.areas[*z].polygon.contains(w.pos))
                            && ahead > 0.0
                            && ahead < 4.0
                            && local.dot(car.dir.perp()).abs() < 1.5
                    });
                    if blocked {
                        checked += 1;
                        assert!(
                            car.held_by_walkers,
                            "signals {signals}, seed {seed}, tick {}: {:?} stands for people \
                             on the zebra but is not held by them",
                            run.tick, car.id
                        );
                    }
                }
            }
        }
    }
    println!("{checked} cars standing for people on the zebra before their line");
    assert!(checked > 0);
}
