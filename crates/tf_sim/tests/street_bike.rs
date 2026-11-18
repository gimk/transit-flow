//! Cyclists (pivot plan Phase 6, package 1) on the test level "Carrefour école".

use tf_sim::street::Design;
use tf_sim::street::level::{LevelFile, Mode};
use tf_sim::street::run::{Population, RunOptions, StreetRun};

fn new_run(seed: u64, population: Population) -> StreetRun {
    let level = LevelFile::load("carrefour-ecole").expect("the level loads");
    let options = RunOptions {
        seed,
        population,
        demand_scale: 1.0,
    };
    StreetRun::new(&level, &Design::new(&level.id), options).expect("builds")
}

fn finished(seed: u64, population: Population) -> StreetRun {
    let mut run = new_run(seed, population);
    run.run_to_end();
    run
}

#[test]
fn cyclists_spawn_and_most_of_them_arrive() {
    let (mut spawned, mut arrived) = (0, 0);
    for seed in [1, 2, 4, 5, 6] {
        let c = finished(seed, Population::Careful).summary().cyclists;
        spawned += c.spawned;
        arrived += c.arrived;
    }
    assert!(spawned >= 20, "cyclists spawn: {spawned}");
    assert!(
        arrived * 10 >= spawned * 9,
        "{arrived} of {spawned} arrived"
    );
}

#[test]
fn a_run_with_cyclists_is_deterministic() {
    let a = finished(3, Population::Mixed);
    let b = finished(3, Population::Mixed);
    assert_eq!(a.log_hash(), b.log_hash());
    assert_ne!(a.log_hash(), finished(4, Population::Mixed).log_hash());
}

#[test]
fn careless_cyclists_make_more_serious_conflicts_than_careful_ones() {
    let total = |population| -> u32 {
        (1..=6)
            .map(|seed| {
                let c = finished(seed, population).summary().cyclists;
                c.conflicts_serious + c.contacts_with_cars + c.contacts_with_walkers
            })
            .sum()
    };
    let careful = total(Population::Careful);
    let careless = total(Population::Careless);
    assert!(careless > careful, "careless {careless}, careful {careful}");
}

/// A cyclist rides off the middle of the lane (near the right edge in a shared lane).
#[test]
fn cyclists_ride_off_the_middle_of_their_lane() {
    let mut off_centre = false;
    for seed in 1..=3 {
        let mut run = new_run(seed, Population::Mixed);
        while !run.finished() {
            run.step();
            off_centre |= run
                .agents
                .values()
                .any(|a| a.mode == Mode::Bicycle && a.lateral.abs() > 0.3);
        }
    }
    assert!(off_centre, "no cyclist rode off the middle of its lane");
}

/// Passing a cyclist swings a car into the lane of the oncoming traffic; two cars did that
/// from opposite sides and met (seed 13 of the careful runs). A car starts no pass near a
/// junction, and looks far enough up the road for what comes at speed.
#[test]
fn careful_drivers_that_pass_cyclists_do_not_meet_head_on() {
    let level = LevelFile::load("carrefour-ecole").expect("the level loads");
    for seed in 1..=20 {
        let options = RunOptions {
            seed,
            population: Population::Careful,
            demand_scale: 1.0,
        };
        let mut run = StreetRun::new(&level, &Design::new(&level.id), options).expect("builds");
        run.run_to_end();
        assert_eq!(run.summary().contacts, 0, "seed {seed}");
    }
}
