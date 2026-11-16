//! The street run on the runner (pivot plan Phase 2): uneven frames, speed changes and a
//! worker thread must not change the run (same event log hash as a headless run), and the
//! agents view draws what the runner shows.

use crate::common::Harness;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use tf_game::street_runner::{StreetRunner, StreetSnapshot};
use tf_game::views::agents::AgentViews;
use tf_game::views::walkers::WalkerView;
use tf_sim::street::ids::AgentId;
use tf_sim::street::run::{RunOptions, StreetRun};
use tf_sim::street::{Design, LevelFile};

const LEVEL: &str = "carrefour-ecole";
/// Frame lengths (real seconds) and speed indexes (x1, x4, pause, x2) that repeat.
const DELTAS: [f64; 4] = [0.016, 0.033, 0.007, 0.1];
const SPEEDS: [usize; 4] = [1, 3, 0, 2];
/// `advance` never waits for the worker, so a frame count says nothing about a threaded
/// run: the guard is real time.
const MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(300);

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
    let started = std::time::Instant::now();
    while !runner.view_to().finished {
        assert!(started.elapsed() < MAX_WAIT, "the run never finished");
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

fn walkers_drawn(harness: &Harness) -> usize {
    harness.app.world().resource::<WalkerView>().drawn
}

/// The vertices in the walkers' mesh: three placeholders when it is empty (a mesh without
/// vertices is skipped by Bevy), a body and its mark per walker otherwise.
fn walker_vertices(harness: &Harness) -> usize {
    let world = harness.app.world();
    let mesh = world.resource::<WalkerView>().mesh.clone();
    let meshes = world.resource::<Assets<Mesh>>();
    match meshes
        .get(&mesh)
        .and_then(|mesh| mesh.attribute(Mesh::ATTRIBUTE_POSITION))
    {
        Some(VertexAttributeValues::Float32x3(positions)) => positions.len(),
        _ => 0,
    }
}

#[test]
fn a_snapshot_copies_the_walkers_in_id_order() {
    let mut run = new_run();
    let mut steps = 0;
    while run.walkers.is_empty() {
        run.step();
        steps += 1;
        assert!(steps < 40_000, "no walker ever came in");
    }
    let snapshot = StreetSnapshot::capture(&run);
    assert_eq!(snapshot.tick, run.tick);
    assert_eq!(snapshot.walkers.len(), run.walkers.len());
    assert!(snapshot.walkers.windows(2).all(|w| w[0].id < w[1].id));
    for walker in run.walkers.values() {
        let pose = &snapshot.walkers[snapshot
            .walker_index_of(walker.id)
            .expect("every walker has a pose")];
        assert_eq!(pose.pos, walker.pos);
        assert_eq!(pose.prev_pos, walker.prev_pos);
        assert_eq!(pose.archetype, walker.archetype);
        assert_eq!(pose.group, walker.group);
        assert_eq!(pose.radius, walker.radius);
        assert_eq!(pose.waiting, walker.kerb_since.is_some());
        let area = run.walk_paths[walker.path].areas[walker.area_index];
        assert_eq!(
            pose.on_carriageway,
            run.walk_space.areas[area].kind.is_road()
        );
    }
    assert_eq!(snapshot.walker_index_of(AgentId(-1)), None);
    // The cars keep their own list.
    assert!(snapshot.agents.windows(2).all(|w| w[0].id < w[1].id));
}

#[test]
fn the_walkers_are_drawn_while_a_run_exists() {
    let mut harness = Harness::new();
    harness.frames(2);
    assert_eq!(walkers_drawn(&harness), 0, "no run, nothing drawn");
    assert_eq!(
        walker_vertices(&harness),
        3,
        "only the placeholder triangle"
    );

    harness
        .app
        .world_mut()
        .resource_mut::<StreetRunner>()
        .start(new_run(), false);
    let mut frames = 0;
    while walkers_drawn(&harness) == 0 {
        assert!(frames < 20_000, "no walker was ever drawn");
        harness
            .app
            .world_mut()
            .resource_mut::<StreetRunner>()
            .advance(0.05);
        harness.frames(1);
        frames += 1;
    }
    assert!(walker_vertices(&harness) > 3, "a body and its mark");

    harness
        .app
        .world_mut()
        .resource_mut::<StreetRunner>()
        .stop();
    harness.frames(2);
    assert_eq!(walkers_drawn(&harness), 0, "stopped, nothing drawn");
    assert_eq!(
        walker_vertices(&harness),
        3,
        "only the placeholder triangle"
    );
}
