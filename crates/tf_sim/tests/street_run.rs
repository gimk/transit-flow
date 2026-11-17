//! The run sim for cars (pivot plan Phase 2, "Done when") on the test level "Carrefour
//! école": careful drivers never touch, careless ones do and every contact says why, a run
//! is a pure function of `level + design + seed + options`, driving styles differ, and the
//! junction rules (signals, stop sign, priority to the right) show in the traffic.

use std::collections::{HashMap, HashSet};

use tf_sim::street::level::{JunctionControl, LevelFile, Mode, Sign};
use tf_sim::street::run::detect::{Body, overlap};
use tf_sim::street::run::{ConflictKind, Population, RunOptions, StreetEvent, StreetRun, Summary};
use tf_sim::street::signals::Light;
use tf_sim::street::{AgentId, Design, DesignCommand};

const LEVEL: &str = "carrefour-ecole";

fn level() -> LevelFile {
    LevelFile::load(LEVEL).expect("the test level loads")
}

fn signal_level() -> LevelFile {
    let mut level = level();
    level.junctions[0].control = JunctionControl::Signals;
    level
}

fn options(seed: u64, population: Population, demand_scale: f64) -> RunOptions {
    RunOptions {
        seed,
        population,
        demand_scale,
    }
}

fn new_run(
    level: &LevelFile,
    design: &Design,
    seed: u64,
    population: Population,
    scale: f64,
) -> StreetRun {
    StreetRun::new(level, design, options(seed, population, scale)).expect("the level builds")
}

fn run_to_end(
    level: &LevelFile,
    design: &Design,
    seed: u64,
    population: Population,
    scale: f64,
) -> StreetRun {
    let mut run = new_run(level, design, seed, population, scale);
    run.run_to_end();
    run
}

fn summaries(
    level: &LevelFile,
    seeds: std::ops::RangeInclusive<u64>,
    population: Population,
    scale: f64,
) -> Vec<(u64, Summary)> {
    let design = Design::new(&level.id);
    seeds
        .map(|seed| {
            (
                seed,
                run_to_end(level, &design, seed, population, scale).summary(),
            )
        })
        .collect()
}

fn conflicts(summary: &Summary) -> u32 {
    summary.conflicts_light + summary.conflicts_serious
}

fn body(agent: &tf_sim::street::run::Agent) -> Body {
    Body {
        center: agent.pos,
        dir: agent.dir,
        half_length: (agent.length / 2.0) as f32,
        half_width: (agent.width / 2.0) as f32,
    }
}

// --- (a) careful drivers -------------------------------------------------------------

/// The level without its pedestrian flows. Since Phase 3 stage C drivers yield to people
/// on foot, and steady streams of them at the zebras past the junction exits can starve
/// an approach (the user's decision of 2026-11-16): the tests whose subject is drivers
/// among themselves run without walkers. Cars and walkers together are covered by the
/// stage C tests in `street_meet.rs` (`careful_drivers_and_walkers_never_touch`: no
/// contact, and a stated bound on those who give up).
fn without_walkers(mut level: LevelFile) -> LevelFile {
    for slice in &mut level.demand {
        slice.flows.retain(|flow| flow.mode != Mode::Pedestrian);
    }
    level
}

#[test]
fn careful_drivers_never_touch_and_everybody_arrives() {
    for (seed, s) in summaries(&without_walkers(level()), 1..=50, Population::Careful, 1.0) {
        assert_eq!(s.contacts, 0, "seed {seed}: {s:?}");
        assert_eq!(s.arrived, s.spawned, "seed {seed}: {s:?}");
        assert_eq!(s.gave_up, 0, "seed {seed}: {s:?}");
        assert!(s.spawned > 5, "seed {seed}: {s:?}");
    }
}

#[test]
fn careful_drivers_never_touch_on_a_busy_street() {
    for (seed, s) in summaries(&level(), 1..=20, Population::Careful, 4.0) {
        assert_eq!(s.contacts, 0, "seed {seed}: {s:?}");
    }
}

/// Wanted behaviour (the user's call, 2026-11-16): with priority to the right, a
/// saturated crossroads starves its minor street. At 4 times the traffic the west-to-east
/// flow alone (880 an hour) leaves no gap in which the north street can cross, so some of
/// its drivers wait out the limit and give up. It is the player's problem to solve:
/// under signals nobody gives up (`careful_drivers_cope_with_signals_on_a_busy_street`).
#[test]
fn a_saturated_crossroads_without_signals_starves_its_minor_street() {
    let starved = summaries(&level(), 1..=20, Population::Careful, 4.0)
        .into_iter()
        .any(|(_, s)| s.gave_up > 0);
    assert!(starved, "nobody gave up without signals");
}

#[test]
fn careful_drivers_cope_with_signals() {
    for (seed, s) in summaries(&signal_level(), 1..=20, Population::Careful, 1.0) {
        assert_eq!(s.contacts, 0, "seed {seed}: {s:?}");
        assert_eq!(s.gave_up, 0, "seed {seed}: {s:?}");
        assert_eq!(s.arrived, s.spawned, "seed {seed}: {s:?}");
    }
}

/// Without walkers: see `without_walkers`.
#[test]
fn careful_drivers_cope_with_signals_on_a_busy_street() {
    for (seed, s) in summaries(
        &without_walkers(signal_level()),
        1..=20,
        Population::Careful,
        4.0,
    ) {
        assert_eq!(s.contacts, 0, "seed {seed}: {s:?}");
        assert_eq!(s.gave_up, 0, "seed {seed}: {s:?}");
    }
}

#[test]
fn a_new_car_does_not_brake_hard_at_the_tick_it_comes_in() {
    let level = level();
    let design = Design::new(&level.id);
    let run = run_to_end(&level, &design, 1, Population::Careful, 1.0);
    let mut spawned_at: HashMap<AgentId, u64> = HashMap::new();
    for logged in run.events() {
        match &logged.event {
            StreetEvent::Spawned { agent, .. } => {
                spawned_at.insert(*agent, logged.tick);
            }
            StreetEvent::Conflict {
                kind: ConflictKind::HardBrake,
                a,
                ..
            } => assert_ne!(
                spawned_at.get(a),
                Some(&logged.tick),
                "agent {a:?} braked hard on its first tick"
            ),
            _ => {}
        }
    }
}

// --- (b) careless drivers ------------------------------------------------------------

#[test]
fn careless_drivers_have_conflicts_and_contacts_and_every_contact_has_a_cause() {
    let level = level();
    let design = Design::new(&level.id);
    let (mut conflicts_seen, mut contacts_seen) = (0, 0);
    for seed in 1..=30 {
        let run = run_to_end(&level, &design, seed, Population::Careless, 3.0);
        for logged in run.events() {
            match &logged.event {
                StreetEvent::Conflict { .. } => conflicts_seen += 1,
                StreetEvent::Contact { a, b, causes, .. } => {
                    contacts_seen += 1;
                    assert!(
                        !causes.is_empty(),
                        "seed {seed}, tick {}: contact {a:?} / {b:?} has no cause",
                        logged.tick
                    );
                }
                _ => {}
            }
        }
    }
    assert!(conflicts_seen > 0, "no conflict at all");
    assert!(contacts_seen > 0, "no contact at all");
}

#[test]
fn careless_drivers_have_more_conflicts_than_careful_ones() {
    let level = level();
    let total = |population| -> u32 {
        summaries(&level, 1..=30, population, 3.0)
            .iter()
            .map(|(_, s)| conflicts(s))
            .sum()
    };
    let (careful, careless) = (total(Population::Careful), total(Population::Careless));
    assert!(
        careless > careful,
        "careless {careless} against careful {careful}"
    );
}

// --- (c) determinism and replay hashes -----------------------------------------------

#[test]
fn the_same_inputs_give_the_same_log_hash() {
    let level = level();
    let design = Design::new(&level.id);
    for population in [Population::Careful, Population::Mixed, Population::Careless] {
        let a = run_to_end(&level, &design, 7, population, 2.0);
        let b = run_to_end(&level, &design, 7, population, 2.0);
        assert_eq!(a.log_hash(), b.log_hash(), "{population:?}");
        assert_eq!(a.events(), b.events(), "{population:?}");
        let other = run_to_end(&level, &design, 8, population, 2.0);
        assert_ne!(a.log_hash(), other.log_hash(), "{population:?}");
    }
}

/// Replay hashes: `level + design + seed + options` -> the hash of the event log. A
/// changed hash means the behaviour of the run changed. That may be deliberate (a retuned
/// constant, a new rule), and then the commit must say so (CLAUDE.md, "Golden and trace
/// tests"); if it is not, the change is a bug. Never edit a hash to make a test pass
/// without knowing why it moved.
// Moved on purpose with the braking model (needed deceleration over the distance left,
// per-vehicle maximum brake, brake build-up): old c59d2a872df0c53f, 0148f27719f690e1,
// ac1abcfab98750b6, 6cd5807b2a438da1.
// Moved on purpose with the pedestrian decisions (WP9: red lights, zebra or free way,
// gap, looking, corner cutting, one habit per rule): old b03860f66190c142,
// c998a0ed29cbb4b7, 29d40c18a8e440e3, 515b33745471b714.
const REPLAY_HASHES: [(u64, Population, f64, u64); 4] = [
    (1, Population::Mixed, 1.0, 0x7bcf_23c0_03b1_36af),
    (2, Population::Mixed, 1.0, 0xf83b_4be3_e0ec_9a3b),
    (1, Population::Careless, 4.0, 0xcafc_6169_817a_3b71),
    (2, Population::Careless, 4.0, 0x7bf9_4ec0_ae48_7899),
];

#[test]
fn replay_hashes_have_not_moved() {
    let level = level();
    let design = Design::new(&level.id);
    for (seed, population, scale, hash) in REPLAY_HASHES {
        let run = run_to_end(&level, &design, seed, population, scale);
        assert_eq!(
            run.log_hash(),
            hash,
            "seed {seed}, {population:?}, x{scale}: the hash is now {:#018x}",
            run.log_hash()
        );
    }
}

// --- (d) driving styles --------------------------------------------------------------

#[test]
fn drivers_have_different_top_speeds() {
    let level = level();
    let design = Design::new(&level.id);
    let mut run = new_run(&level, &design, 1, Population::Mixed, 3.0);
    let mut top: HashMap<AgentId, f64> = HashMap::new();
    while !run.finished() {
        run.step();
        for agent in run.agents.values() {
            let best = top.entry(agent.id).or_insert(0.0);
            *best = best.max(agent.speed);
        }
    }
    let fastest = top.values().copied().fold(0.0, f64::max);
    let slowest = top.values().copied().fold(f64::INFINITY, f64::min);
    assert!(
        fastest - slowest > 1.0,
        "top speeds from {slowest:.2} to {fastest:.2} m/s"
    );
}

#[test]
fn careful_drivers_keep_to_the_speed_limit() {
    let level = level();
    let design = Design::new(&level.id);
    for seed in 1..=5 {
        let mut run = new_run(&level, &design, seed, Population::Careful, 3.0);
        while !run.finished() {
            run.step();
            for agent in run.agents.values() {
                let limit = run.limit_for(agent);
                assert!(
                    agent.speed <= limit + 0.01,
                    "seed {seed}, tick {}: agent {:?} at {:.2} m/s, limit {limit:.2}",
                    run.tick,
                    agent.id,
                    agent.speed
                );
            }
        }
    }
}

// --- micro-scenarios -----------------------------------------------------------------

#[test]
fn careful_drivers_do_not_cross_the_stop_line_on_red() {
    let level = signal_level();
    let design = Design::new(&level.id);
    let (mut crossings, mut waited_on_red) = (0, 0);
    for seed in 1..=6 {
        let mut run = new_run(&level, &design, seed, Population::Careful, 2.0);
        while !run.finished() {
            // The light a car is shown during this step is the one at the step's start.
            let before: Vec<(AgentId, f64, f64, Option<Light>)> = run
                .agents
                .values()
                .filter_map(|a| {
                    let approach = a.approach.as_ref()?;
                    Some((a.id, a.d, approach.line_d, run.light_for(a)))
                })
                .collect();
            run.step();
            for (id, d, line, light) in before {
                let Some(agent) = run.agents.get(&id) else {
                    continue;
                };
                if d <= line && agent.d > line {
                    crossings += 1;
                    assert!(
                        !matches!(light, Some(Light::Red(_))),
                        "seed {seed}, tick {}: agent {id:?} crossed its line on {light:?}",
                        run.tick
                    );
                }
                if matches!(light, Some(Light::Red(_))) && agent.speed < 0.3 {
                    waited_on_red += 1;
                }
            }
        }
    }
    assert!(crossings > 20, "only {crossings} crossings: a weak test");
    assert!(waited_on_red > 0, "nobody ever waited at a red light");
}

#[test]
fn careful_drivers_stop_at_a_stop_sign() {
    let level = level();
    let mut design = Design::new(&level.id);
    design.commands.push(DesignCommand::SetSign {
        node: 0,
        segment: 3,
        sign: Sign::Stop,
    });
    let mut checked = 0;
    for seed in 1..=5 {
        let mut run = new_run(&level, &design, seed, Population::Careful, 1.0);
        let mut from_north: HashSet<AgentId> = HashSet::new();
        let mut stopped: HashSet<AgentId> = HashSet::new();
        while !run.finished() {
            run.step();
            for logged in run
                .events()
                .iter()
                .rev()
                .take_while(|l| l.tick == run.tick - 1)
            {
                if let StreetEvent::Spawned { agent, from, .. } = &logged.event
                    && from == "north"
                {
                    from_north.insert(*agent);
                }
            }
            for agent in run.agents.values() {
                let Some(approach) = &agent.approach else {
                    continue;
                };
                let to_line = approach.line_d - agent.d;
                if (0.0..=4.0).contains(&to_line) && agent.speed < 0.3 {
                    stopped.insert(agent.id);
                }
            }
        }
        // Every north car that passed the junction stopped near its line.
        let mut ended: HashSet<AgentId> = HashSet::new();
        for logged in run.events() {
            if let StreetEvent::TripEnded { agent, .. } = &logged.event {
                ended.insert(*agent);
            }
        }
        for agent in &from_north {
            assert!(
                ended.contains(agent),
                "seed {seed}: {agent:?} never arrived"
            );
            assert!(
                stopped.contains(agent),
                "seed {seed}: {agent:?} from the north did not stop at the sign"
            );
            checked += 1;
        }
    }
    assert!(checked >= 5, "only {checked} cars from the north");
}

#[test]
fn signals_cost_more_time_than_priority_to_the_right() {
    let mean_delay = |level: &LevelFile| {
        let (mut total, mut arrived) = (0.0, 0.0);
        for (_, s) in summaries(level, 1..=10, Population::Careful, 1.0) {
            total += s.mean_delay_s * f64::from(s.arrived);
            arrived += f64::from(s.arrived);
        }
        total / arrived
    };
    let priority = mean_delay(&level());
    let signals = mean_delay(&signal_level());
    assert!(
        signals > priority,
        "mean delay: signals {signals:.2} s, priority to the right {priority:.2} s"
    );
}

// --- invariants ----------------------------------------------------------------------

#[test]
fn speeds_and_positions_stay_sane_and_careful_bodies_never_overlap() {
    let level = level();
    let design = Design::new(&level.id);
    for (population, scale, check_overlap) in [
        (Population::Careful, 4.0, true),
        (Population::Careless, 3.0, false),
    ] {
        for seed in 1..=3 {
            let mut run = new_run(&level, &design, seed, population, scale);
            while !run.finished() {
                run.step();
                let agents: Vec<_> = run.agents.values().collect();
                for agent in &agents {
                    if agent.crashed() {
                        continue;
                    }
                    assert!(
                        agent.speed.is_finite() && agent.speed >= 0.0,
                        "{population:?} seed {seed}, tick {}: speed {}",
                        run.tick,
                        agent.speed
                    );
                    assert!(
                        agent.pos.is_finite() && agent.dir.is_finite(),
                        "{population:?} seed {seed}, tick {}: pose {:?}",
                        run.tick,
                        agent.pos
                    );
                }
                if !check_overlap {
                    continue;
                }
                for (i, a) in agents.iter().enumerate() {
                    for b in &agents[i + 1..] {
                        if a.crashed() || b.crashed() {
                            continue;
                        }
                        assert!(
                            !overlap(&body(a), &body(b)),
                            "seed {seed}, tick {}: {:?} and {:?} overlap",
                            run.tick,
                            a.id,
                            b.id
                        );
                    }
                }
            }
        }
    }
}
