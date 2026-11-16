//! The street run on the runner (pivot plan Phase 2): uneven frames, speed changes and a
//! worker thread must not change the run (same event log hash as a headless run), and the
//! agents view draws what the runner shows.

use crate::common::Harness;
use tf_game::street_runner::StreetRunner;
use tf_game::views::agents::AgentViews;
use tf_sim::street::run::{RunOptions, StreetRun};
use tf_sim::street::{Design, LevelFile};

const LEVEL: &str = "carrefour-ecole";
/// Frame lengths (real seconds) and speed indexes (x1, x4, pause, x2) that repeat.
const DELTAS: [f64; 4] = [0.016, 0.033, 0.007, 0.1];
const SPEEDS: [usize; 4] = [1, 3, 0, 2];
const MAX_FRAMES: usize = 5_000_000;

fn new_run() -> StreetRun {
    let level = LevelFile::load(LEVEL).expect("loads");
    // The level's seed and the mixed population.
    let options = RunOptions::for_level(&level);
    StreetRun::new(&level, &Design::new(&level.id), options).expect("builds")
}

fn headless_hash() -> u64 {
    let mut run = new_run();
    run.run_to_end();
    run.log_hash()
}

fn hash_through_runner(threaded: bool) -> u64 {
    let mut runner = StreetRunner::default();
    runner.start(new_run(), threaded);
    let mut frame = 0;
    while !runner.view_to().finished {
        assert!(frame < MAX_FRAMES, "the run never finished");
        if frame % 40 == 0 {
            runner.set_speed(SPEEDS[(frame / 40) % SPEEDS.len()]);
        }
        runner.advance(DELTAS[frame % DELTAS.len()]);
        frame += 1;
    }
    let run = runner.get_mut().expect("a run");
    assert!(run.finished());
    run.log_hash()
}

#[test]
fn an_inline_runner_gives_the_headless_hash() {
    assert_eq!(hash_through_runner(false), headless_hash());
}

#[test]
fn a_threaded_runner_gives_the_headless_hash() {
    assert_eq!(hash_through_runner(true), headless_hash());
}

fn drawn(harness: &Harness) -> usize {
    harness.app.world().resource::<AgentViews>().drawn
}

#[test]
fn the_agents_are_drawn_while_a_run_exists() {
    let mut harness = Harness::new();
    harness.frames(2);
    assert_eq!(drawn(&harness), 0, "no run, nothing drawn");

    harness
        .app
        .world_mut()
        .resource_mut::<StreetRunner>()
        .start(new_run(), false);
    let mut frames = 0;
    while drawn(&harness) == 0 {
        assert!(frames < 20_000, "no agent was ever drawn");
        harness
            .app
            .world_mut()
            .resource_mut::<StreetRunner>()
            .advance(0.05);
        harness.frames(1);
        frames += 1;
    }
    assert!(drawn(&harness) > 0);

    harness
        .app
        .world_mut()
        .resource_mut::<StreetRunner>()
        .stop();
    harness.frames(2);
    assert_eq!(drawn(&harness), 0, "stopped, nothing drawn");
}
