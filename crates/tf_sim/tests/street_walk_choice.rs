//! Pedestrian decisions (pivot plan Phase 4, WP9): one decision per rule and per crossing,
//! from what the walker sees: red lights, zebra or free way, the gap, corner cutting.
//! Habits are separate per rule.

use tf_sim::rng::SimRng;
use tf_sim::street::Design;
use tf_sim::street::level::{JunctionControl, LevelFile};
use tf_sim::street::run::walker::{
    WALK_CAREFUL, WALK_CHILD, WALK_COMMUTER, WALK_ELDERLY, WALK_HURRIED, WALK_RESIDENT,
    WalkerTraits,
};
use tf_sim::street::run::{Population, RunOptions, StreetEvent, StreetRun};

/// A straight two-way street 160 m long and 12 m wide (3 m sidewalks), doors at x = 60
/// ("north") and at `south_x` ("south"), a zebra centred at `zebra_at` (`None`: no zebra,
/// 3 m wide) and the given flows in one slice.
fn street(zebra_at: Option<f32>, south_x: f32, flows: &str, slice_s: f32) -> LevelFile {
    let crossings = zebra_at
        .map(|at| format!("(kind: Zebra, segment: 1, at: {at:?}, width: 3.0)"))
        .unwrap_or_default();
    let text = format!(
        r#"(
        id: "choice-test", name: "Choice test", place: "test", difficulty: 1,
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
            (id: "south", position: ({south_x:?}, 12.0), segment: 1, access: 1),
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

fn walkers_flow() -> String {
    flow("Pedestrian", "north", "south", 900) + &flow("Pedestrian", "south", "north", 900)
}

fn cars_flow() -> String {
    flow("Car", "west", "east", 700) + &flow("Car", "east", "west", 700)
}

fn run(level: &LevelFile, seed: u64, population: Population, scale: f64) -> StreetRun {
    let options = RunOptions {
        seed,
        population,
        demand_scale: scale,
    };
    StreetRun::new(level, &Design::new(&level.id), options).expect("the level builds")
}

/// The signalled test junction with only the flows kept that `keep` accepts.
fn signal_level(keep_cars: bool) -> LevelFile {
    let mut level = LevelFile::load("carrefour-ecole").expect("the test level loads");
    level.junctions[0].control = JunctionControl::Signals;
    if !keep_cars {
        for slice in &mut level.demand {
            slice
                .flows
                .retain(|f| format!("{:?}", f.mode).contains("Pedestrian"));
        }
    }
    level
}

/// Steps off a kerb: (x, crossing, on red, gap).
fn starts(run: &StreetRun) -> Vec<(f32, Option<i32>, bool, Option<f64>)> {
    run.events()
        .iter()
        .filter_map(|e| match e.event {
            StreetEvent::CrossingStarted {
                crossing,
                position,
                on_red,
                gap_s,
                ..
            } => Some((position.x, crossing, on_red, gap_s)),
            _ => None,
        })
        .collect()
}

fn run_with(level: &LevelFile, seed: u64, population: Population, scale: f64) -> StreetRun {
    let mut run = run(level, seed, population, scale);
    run.run_to_end();
    run
}

#[test]
fn a_careful_population_still_has_no_contact() {
    let level = LevelFile::load("carrefour-ecole").expect("the test level loads");
    for seed in 1..=4 {
        let run = run_with(&level, seed, Population::Careful, 3.0);
        let s = run.summary();
        assert_eq!(s.contacts, 0, "seed {seed}");
        assert_eq!(s.walkers.contacts, 0, "seed {seed}");
        assert_eq!(s.walkers.on_red, 0, "seed {seed}");
    }
}

/// The share of the people stepping off on red among all who step off, mixed walkers.
fn red_share(level: &LevelFile) -> (f64, usize) {
    let (mut red, mut all) = (0, 0);
    for seed in 1..=8 {
        let run = run_with(level, seed, Population::Mixed, 2.0);
        for (_, _, on_red, _) in starts(&run) {
            all += 1;
            red += usize::from(on_red);
        }
    }
    (red as f64 / all as f64, all)
}

/// People cross on red when no car is in view and hardly when cars come: same level, same
/// seeds and walkers, with and without cars.
#[test]
fn on_red_people_go_when_no_car_comes_and_wait_when_cars_come() {
    let (empty, n) = red_share(&signal_level(false));
    let (busy, m) = red_share(&signal_level(true));
    println!("on red: no cars {empty:.3} of {n}, with cars {busy:.3} of {m}");
    assert!(n >= 40 && m >= 40, "{n} {m}");
    assert!(empty >= 0.15, "no cars: {empty:.3}");
    assert!(
        busy * 2.0 <= empty,
        "cars {busy:.3} against none {empty:.3}"
    );
}

/// A walker who looks (attention 1) never steps off in a gap below its crossing time,
/// even the most hurried one: risk only takes a share of the margin.
#[test]
fn a_walker_who_looks_never_steps_off_in_a_gap_shorter_than_its_crossing_time() {
    let flows = walkers_flow() + &cars_flow();
    let level = street(None, 60.0, &flows, 120.0);
    let (mut with_gap, mut shortest) = (0, f64::INFINITY);
    for seed in 1..=4 {
        let mut run = run(&level, seed, Population::Mixed, 1.0);
        let mut seen = 0;
        while !run.finished() {
            for w in run.walkers.values_mut() {
                w.traits.attention = 1.0;
                w.traits.risk = 1.0;
            }
            run.step();
            let found = starts(&run);
            for &(_, _, _, gap) in &found[seen..] {
                if let Some(gap) = gap {
                    with_gap += 1;
                    // The road is 6 m wide; the crossing time of the quickest walker.
                    let fastest = run
                        .walkers
                        .values()
                        .map(|w| w.desired_speed)
                        .fold(0.0, f64::max);
                    shortest = shortest.min(gap * fastest);
                }
            }
            seen = found.len();
        }
    }
    println!("{with_gap} steps off with a car in view; shortest gap x speed {shortest:.2} m");
    assert!(with_gap >= 10, "{with_gap}");
    assert!(shortest >= 6.0, "a gap of {shortest:.2} m of walking");
}

/// Walkers who do not look are the ones who step in front of cars: more conflicts and
/// contacts between cars and walkers when nobody looks than when everybody does.
#[test]
fn walkers_who_do_not_look_meet_cars_more_often() {
    let flows = walkers_flow() + &cars_flow();
    let level = street(None, 60.0, &flows, 120.0);
    let meetings = |attention: f64, look: f32| {
        let mut total = 0;
        for seed in 1..=6 {
            let mut run = run(&level, seed, Population::Mixed, 1.0);
            while !run.finished() {
                for w in run.walkers.values_mut() {
                    w.traits.attention = attention;
                    for d in &mut w.kerb_draws {
                        d.look = look;
                    }
                }
                run.step();
            }
            let s = run.summary();
            total += s.walkers.contacts + s.conflicts_light + s.conflicts_serious;
        }
        total
    };
    let blind = meetings(0.0, 0.0);
    let looking = meetings(1.0, 1.0);
    println!("walker meetings with cars: nobody looks {blind}, everybody looks {looking}");
    assert!(blind > looking, "{blind} against {looking}");
}

/// The share of street crossings made out of a crossing over seeds 1 to 6 (zebra 40 m
/// from the doors).
fn off_crossing_share(flows: &str) -> (f64, usize) {
    let level = street(Some(100.0), 60.0, flows, 120.0);
    let (mut off, mut all) = (0, 0);
    for seed in 1..=6 {
        let run = run_with(&level, seed, Population::Mixed, 1.0);
        for (_, crossing, _, _) in starts(&run) {
            all += 1;
            off += usize::from(crossing.is_none());
        }
    }
    (off as f64 / all as f64, all)
}

#[test]
fn with_traffic_fewer_people_cross_away_from_the_zebra() {
    let (empty, n) = off_crossing_share(&walkers_flow());
    let (busy, m) = off_crossing_share(&(walkers_flow() + &cars_flow()));
    println!("off the zebra: empty street {empty:.2} of {n}, with cars {busy:.2} of {m}");
    assert!(n >= 80 && m >= 80, "{n} {m}");
    assert!(empty > 0.1, "{empty:.2}");
    assert!(busy < empty, "{busy:.2} against {empty:.2}");
}

/// Walkers cut the corner of a zebra (step onto the road beside it, on a diagonal) and
/// still count as crossing at it; careful drivers still yield to them.
#[test]
fn some_walkers_cut_the_corner_at_a_zebra_and_drivers_still_yield() {
    // The south door is 6 m east of the north one: the goal pulls to the east.
    let flows = walkers_flow() + &cars_flow();
    let level = street(Some(60.0), 66.0, &flows, 120.0);
    let (mut at_zebra, mut beside) = (0, 0);
    for seed in 1..=3 {
        let mut run = run(&level, seed, Population::Careful, 1.0);
        while !run.finished() {
            for w in run.walkers.values_mut() {
                w.traits.corner_habit = 0.0;
                for d in &mut w.kerb_draws {
                    d.corner = 0.0;
                }
            }
            run.step();
        }
        for (x, crossing, _, _) in starts(&run) {
            assert!(crossing.is_some(), "a cut corner still counts as the zebra");
            if (58.5..=61.5).contains(&x) {
                at_zebra += 1;
            } else {
                beside += 1;
                println!("beside x = {x} seed {seed}");
            }
        }
        let s = run.summary();
        assert_eq!(s.contacts, 0, "seed {seed}");
        assert_eq!(s.walkers.contacts, 0, "seed {seed}");
        assert_eq!(s.walkers.off_crossing, 0, "seed {seed}");
    }
    println!("{at_zebra} steps off on the zebra, {beside} beside it");
    assert!(beside >= 3, "{beside} cut the corner");
}

/// A person can ignore red lights and still use the zebras: with the red habit at zero
/// and the zebra habit at one, people cross on red and (almost) never away from a zebra
/// 40 m off their way; with the zebra habit at zero, many cross away from it.
#[test]
fn a_person_can_break_one_rule_and_follow_another() {
    let signals = signal_level(false);
    let far_zebra = street(Some(100.0), 60.0, &walkers_flow(), 120.0);
    let totals = |zebra_habit: f64| {
        let (mut red, mut off) = (0, 0);
        for (level, scale) in [(&signals, 2.0), (&far_zebra, 1.0)] {
            for seed in 1..=4 {
                let mut run = run(level, seed, Population::Mixed, scale);
                while !run.finished() {
                    for w in run.walkers.values_mut() {
                        w.traits.red_habit = 0.0;
                        w.traits.zebra_habit = zebra_habit;
                    }
                    run.step();
                }
                let s = run.summary();
                red += s.walkers.on_red;
                off += s.walkers.off_crossing;
            }
        }
        (red, off)
    };
    let (red, off_follow) = totals(1.0);
    let (_, off_break) = totals(0.0);
    println!("{red} on red; off the zebras: habit 1 {off_follow}, habit 0 {off_break}");
    assert!(red > 0);
    assert!(off_follow <= 5, "{off_follow}");
    assert!(
        off_break >= 3 * off_follow.max(1),
        "{off_break} against {off_follow}"
    );
}

#[test]
fn habits_follow_compliance_but_are_not_one_toggle() {
    // Fully compliant people have fully compliant habits.
    for seed in 0..50 {
        let mut rng = SimRng::new(seed);
        let t = WalkerTraits::draw(&WALK_CAREFUL, &mut rng);
        let mut t2 = t;
        t2.draw_habits(&WALK_CAREFUL, &mut rng);
        assert_eq!(
            (t2.red_habit, t2.zebra_habit, t2.corner_habit),
            (1.0, 1.0, 1.0)
        );
    }
    // Others: the habits differ within one person, in both directions, and stay in 0..1.
    let (mut red_lower, mut zebra_lower) = (0, 0);
    let mut related = 0;
    for archetype in [
        WALK_COMMUTER,
        WALK_RESIDENT,
        WALK_CHILD,
        WALK_ELDERLY,
        WALK_HURRIED,
    ] {
        for seed in 0..200 {
            let mut rng = SimRng::new(seed);
            let mut t = WalkerTraits::draw(&archetype, &mut rng);
            t.draw_habits(&archetype, &mut rng);
            for h in [t.red_habit, t.zebra_habit, t.corner_habit] {
                assert!((0.0..=1.0).contains(&h));
            }
            red_lower += usize::from(t.red_habit + 0.1 < t.zebra_habit);
            zebra_lower += usize::from(t.zebra_habit + 0.1 < t.red_habit);
            related += usize::from((t.red_habit - t.compliance).abs() <= 0.5);
        }
    }
    assert!(
        red_lower > 50 && zebra_lower > 5,
        "{red_lower} {zebra_lower}"
    );
    assert_eq!(related, 1000, "habits stay near compliance");
}
