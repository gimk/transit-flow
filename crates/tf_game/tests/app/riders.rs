//! Phase 6, W6b: the riders of a run in the views, the bicycle light in a run, and the
//! riders in the report. A rider is an agent of `Mode::Bicycle`: it is in the snapshot with
//! its side position, it is drawn as its own shape, the small bicycle light of a junction
//! with a leading green follows the run's plan, and the report counts it like the other
//! modes. The tests that open play mode share the design folder, so they hold `save_lock`.

use crate::common::{Harness, save_lock};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use tf_game::play::ui::report_tail_text;
use tf_game::play::{PlayPhase, PlayState};
use tf_game::street_runner::{AgentPose, SignalColor, StreetRunner, StreetSnapshot};
use tf_game::views::agents::{AgentViews, Drawn, draw_agents};
use tf_game::views::batch::Batch;
use tf_sim::street::design::SignalTemplate;
use tf_sim::street::ids::AgentId;
use tf_sim::street::level::{JunctionControl, Mode};
use tf_sim::street::run::{RunOptions, StreetRun};
use tf_sim::street::{Design, DesignCommand, LevelFile};

const LEVEL: &str = "carrefour-ecole";
/// The junction of the level the light tests signal.
const NODE: i32 = 0;
/// The leading bicycle green of the light tests, seconds.
const LEAD_S: f32 = 4.0;
/// The archetypes of the riders (`tf_sim::street::run::agent`).
const RIDER_ARCHETYPES: [&str; 6] = [
    "careful cyclist",
    "velib tourist",
    "bike commuter",
    "delivery rider",
    "cargo bike",
    "careless cyclist",
];

fn level() -> LevelFile {
    LevelFile::load(LEVEL).expect("loads")
}

fn new_run() -> StreetRun {
    let level = level();
    let options = RunOptions::for_level(&level);
    StreetRun::new(&level, &Design::new(&level.id), options).expect("builds")
}

/// A design with signals on `NODE` from a template, and a leading bicycle green of
/// `lead_s` seconds (0: none).
fn signalled_design(lead_s: f32) -> Design {
    let level = level();
    let mut design = Design::new(&level.id);
    design.commands.push(DesignCommand::SetJunctionControl {
        node: NODE,
        control: JunctionControl::Signals,
        template: Some(SignalTemplate::TwoPhase { green_s: 20.0 }),
    });
    if lead_s > 0.0 {
        design
            .commands
            .push(DesignCommand::SetBicycleLead { node: NODE, lead_s });
    }
    design
}

fn signalled_run(lead_s: f32) -> StreetRun {
    let level = level();
    let options = RunOptions::for_level(&level);
    StreetRun::new(&level, &signalled_design(lead_s), options).expect("builds")
}

/// A pose of a vehicle of `mode`, at `pos`, going east.
fn pose(id: i32, mode: Mode, archetype: &'static str, pos: Vec2) -> AgentPose {
    let (length, width) = match mode {
        Mode::Bicycle => (1.8, 0.6),
        _ => (4.3, 1.8),
    };
    AgentPose {
        id: AgentId(id),
        mode,
        archetype,
        pos,
        dir: Vec2::X,
        prev_pos: pos,
        prev_dir: Vec2::X,
        length,
        width,
        lateral: 0.0,
        speed: 5.0,
        frustration: 0.0,
        waiting: false,
        crashed: false,
        distracted: false,
        held_by_walkers: false,
    }
}

fn snapshot(agents: Vec<AgentPose>) -> StreetSnapshot {
    StreetSnapshot {
        agents,
        ..Default::default()
    }
}

#[test]
fn riders_are_in_the_snapshot_with_their_side_position() {
    let mut run = new_run();
    let mut steps = 0;
    while !run.agents.values().any(|agent| agent.mode == Mode::Bicycle) {
        run.step();
        steps += 1;
        assert!(steps < 40_000, "no rider ever came in");
    }
    let snapshot = StreetSnapshot::capture(&run);
    let riders: Vec<&AgentPose> = snapshot
        .agents
        .iter()
        .filter(|pose| pose.mode == Mode::Bicycle)
        .collect();
    assert!(!riders.is_empty(), "the riders are in the snapshot");
    for pose in riders {
        let agent = &run.agents[&pose.id];
        assert_eq!(pose.pos, agent.pos);
        assert_eq!(pose.lateral, agent.lateral as f32);
        // The body the run gives a bicycle: 1.8 m by 0.6 m.
        assert_eq!((pose.length, pose.width), (1.8, 0.6));
        assert!(
            RIDER_ARCHETYPES.contains(&pose.archetype),
            "{}",
            pose.archetype
        );
    }
}

#[test]
fn a_rider_is_drawn_as_a_body_with_a_rider_on_it() {
    let car = snapshot(vec![pose(1, Mode::Car, "careful", Vec2::ZERO)]);
    let rider = snapshot(vec![pose(2, Mode::Bicycle, "bike commuter", Vec2::ZERO)]);
    let mut batch = Batch::default();
    let car_drawn = draw_agents(&mut batch, &car, &car, 0.0);
    let car_vertices = batch.vertex_count();
    let rider_drawn = draw_agents(&mut batch, &rider, &rider, 0.0);
    let rider_vertices = batch.vertex_count();
    assert_eq!(
        car_drawn,
        Drawn {
            agents: 1,
            riders: 0
        }
    );
    assert_eq!(
        rider_drawn,
        Drawn {
            agents: 1,
            riders: 1
        }
    );
    // A car is its body and its front mark; a rider adds the rider on the bicycle.
    assert_eq!(car_vertices, 8, "two quads");
    assert!(
        rider_vertices > car_vertices,
        "a rider takes more than a car: {rider_vertices} / {car_vertices}"
    );
}

/// The vertices in the agents' mesh: three placeholders when it is empty (a mesh without
/// vertices is skipped by Bevy).
fn agent_vertices(harness: &Harness) -> usize {
    let world = harness.app.world();
    let mesh = world.resource::<AgentViews>().mesh.clone();
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
fn the_agents_view_draws_the_riders_of_a_run() {
    let mut harness = Harness::new();
    harness.frames(2);
    assert_eq!(harness.app.world().resource::<AgentViews>().riders, 0);

    harness
        .app
        .world_mut()
        .resource_mut::<StreetRunner>()
        .start(new_run(), false);
    let mut frames = 0;
    while harness.app.world().resource::<AgentViews>().riders == 0 {
        assert!(frames < 20_000, "no rider was ever drawn");
        harness
            .app
            .world_mut()
            .resource_mut::<StreetRunner>()
            .advance(0.05);
        harness.frames(1);
        frames += 1;
    }
    let views = harness.app.world().resource::<AgentViews>();
    assert!(views.riders > 0 && views.drawn >= views.riders);
    // A rider takes more vertices than a car's two quads, so the mesh is bigger than
    // eight vertices per agent.
    assert!(
        agent_vertices(&harness) > 8 * views.drawn,
        "{} vertices for {} agents",
        agent_vertices(&harness),
        views.drawn
    );

    harness
        .app
        .world_mut()
        .resource_mut::<StreetRunner>()
        .stop();
    harness.frames(2);
    assert_eq!(harness.app.world().resource::<AgentViews>().riders, 0);
}

/// The lights of every approach of `NODE` at each tick of the first `ticks` ticks.
fn lights_over(run: &mut StreetRun, ticks: u64) -> Vec<Vec<(SignalColor, SignalColor)>> {
    let mut seen = Vec::new();
    for _ in 0..ticks {
        run.step();
        let snapshot = StreetSnapshot::capture(run);
        seen.push(
            snapshot
                .lights
                .iter()
                .filter(|light| light.node == NODE)
                .map(|light| (light.color, light.bicycle))
                .collect(),
        );
    }
    seen
}

#[test]
fn the_bicycle_light_leads_the_cars_light_in_a_run() {
    let mut run = signalled_run(LEAD_S);
    // Two cycles of the plan (20 s green, amber and all red) are enough.
    let seen = lights_over(&mut run, 20 * 60);
    assert!(
        seen.iter().all(|lights| !lights.is_empty()),
        "every approach of the junction has a light"
    );
    // The leading green: the bicycle light is green while the cars' light is still red.
    assert!(
        seen.iter()
            .flatten()
            .any(|(car, bicycle)| *car == SignalColor::Red && *bicycle == SignalColor::Green),
        "the bicycle light leads"
    );
    // And it changes during the run, like the cars' light.
    let bicycle_colors: Vec<SignalColor> = seen.iter().flatten().map(|(_, b)| *b).collect();
    assert!(
        bicycle_colors.contains(&SignalColor::Green) && bicycle_colors.contains(&SignalColor::Red),
        "the bicycle light changes"
    );
}

#[test]
fn without_a_lead_the_bicycle_light_is_the_cars_light() {
    let mut run = signalled_run(0.0);
    let seen = lights_over(&mut run, 20 * 60);
    assert!(
        seen.iter().flatten().all(|(car, bicycle)| car == bicycle),
        "no lead: the same light for the bicycles"
    );
}

fn phase(harness: &Harness) -> PlayPhase {
    *harness.app.world().resource::<State<PlayPhase>>().get()
}

fn state(harness: &Harness) -> &PlayState {
    harness.app.world().resource::<PlayState>()
}

#[test]
fn riders_appear_in_the_report() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    harness.key(KeyCode::Enter);
    harness.frames(2);
    assert_eq!(phase(&harness), PlayPhase::Run);
    // Play the day out, then read the report.
    harness
        .app
        .world_mut()
        .resource_mut::<StreetRunner>()
        .get_mut()
        .expect("a run")
        .run_to_end();
    harness.frames(3);
    assert_eq!(phase(&harness), PlayPhase::Report);
    let report = state(&harness).report.clone().expect("a report");
    let summary = report.summary.as_ref().expect("a run summary");
    assert!(
        summary.cyclists.spawned > 0,
        "level 1 has bicycle flows: {:?}",
        summary.cyclists
    );
    // The riders have their own line under the walkers' line.
    let text = report_tail_text(state(&harness), &report);
    assert!(text.contains("Riders:"), "{text}");
    assert!(text.contains("close passes"), "{text}");
    // And the per-mode lines of the stage report count them.
    assert!(
        report
            .stage
            .per_mode
            .iter()
            .any(|stats| stats.mode == Mode::Bicycle && stats.spawned > 0),
        "{:?}",
        report.stage.per_mode
    );
}
