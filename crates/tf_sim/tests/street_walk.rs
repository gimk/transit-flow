//! Pedestrians (pivot plan Phase 3, stage A): the walkable space, the walk graph, the
//! crowd model and the pedestrian spawner. Micro-levels are built here as RON text; the
//! test level "Carrefour école" checks whole trips across junction arms.

use std::collections::BTreeMap;

use glam::Vec2;
use tf_sim::street::AgentId;
use tf_sim::street::Design;
use tf_sim::street::level::{LevelFile, Mode};
use tf_sim::street::run::drive::DT;
use tf_sim::street::run::{
    Population, RunOptions, StreetEvent, StreetRun, WalkSpawn, Walker, is_walker,
};

/// Two bodies may overlap by this much at the end of a tick, metres.
const OVERLAP_TOLERANCE: f32 = 0.02;
/// A body may reach this far into an obstacle or a wall, metres.
const WALL_TOLERANCE: f32 = 0.02;
/// The forces alone (before the position constraints) may let two bodies this far into
/// each other in the counterflow, metres: under half the smallest body radius.
const FORCE_OVERLAP_BOUND: f32 = 0.09;
/// A lone walker on a free street keeps its desired speed and its free walk time
/// within this share.
const LONE_PACE_TOLERANCE: f64 = 0.02;

/// A straight sidewalk from x = 0 to x = `length`, `width` wide, with a gate at each end
/// ("west", "east"), the given obstacles and pedestrian flows in one slice.
fn sidewalk(length: f32, width: f32, obstacles: &str, flows: &str, slice_s: f32) -> LevelFile {
    let y = width / 2.0;
    let text = format!(
        r#"(
        id: "walk-test", name: "Walk test", place: "test", difficulty: 1,
        bounds: (min: (0.0, 0.0), max: ({length:?}, {width:?})),
        nodes: [(id: 1, position: (0.0, {y:?})), (id: 2, position: ({length:?}, {y:?}))],
        segments: [(id: 1, from: 1, to: 2, speed_limit_kmh: 30.0,
            profile: [(use: Sidewalk, width: {width:?})])],
        junctions: [],
        obstacles: [{obstacles}],
        gates: [
            (id: "west", position: (0.0, {y:?}), segment: 1, access: 1),
            (id: "east", position: ({length:?}, {y:?}), segment: 1, access: 1),
        ],
        demand: [(name: "test", duration_s: {slice_s:?}, flows: [{flows}])],
        seed: 1,
    )"#
    );
    LevelFile::from_ron(&text).expect("the micro-level parses")
}

fn walk_flow(from: &str, to: &str, per_hour: u32) -> String {
    format!(r#"(mode: Pedestrian, from: "{from}", to: "{to}", per_hour: {per_hour}),"#)
}

fn new_run(level: &LevelFile, seed: u64, population: Population) -> StreetRun {
    let options = RunOptions {
        seed,
        population,
        demand_scale: 1.0,
    };
    StreetRun::new(level, &Design::new(&level.id), options).expect("the level builds")
}

/// The test level with no demand but a long empty slice, so a run lasts until the
/// walkers added by hand arrive.
fn quiet_school(slice_s: f32) -> LevelFile {
    let mut level = LevelFile::load("carrefour-ecole").expect("the test level loads");
    level.demand.truncate(1);
    level.demand[0].flows.clear();
    level.demand[0].duration_s = slice_s;
    level
}

/// +1 for a walker heading to larger x, -1 otherwise.
fn heading_east(run: &StreetRun, walker: &Walker) -> bool {
    let goal = run.walk_paths[walker.path].goal;
    goal.0.x > walker.pos.x
}

/// The deepest overlap between two walkers now.
fn max_overlap(run: &StreetRun) -> f32 {
    let walkers: Vec<&Walker> = run.walkers.values().collect();
    let mut deepest: f32 = 0.0;
    for (i, a) in walkers.iter().enumerate() {
        for b in &walkers[i + 1..] {
            deepest = deepest.max(a.radius + b.radius - a.pos.distance(b.pos));
        }
    }
    deepest
}

/// Every walker is inside the walkable space, clear of obstacles and walls.
fn assert_inside(run: &StreetRun, what: &str) {
    for walker in run.walkers.values() {
        assert!(
            run.walk_space.contains(walker.pos),
            "{what}, tick {}: {:?} at {:?} is outside the walkable space",
            run.tick,
            walker.id,
            walker.pos
        );
        let depth = run.walk_space.obstacle_depth(walker.pos, walker.radius);
        assert!(
            depth <= WALL_TOLERANCE,
            "{what}, tick {}: {:?} is {depth} m into an obstacle",
            run.tick,
            walker.id
        );
        for wall in &run.walk_space.walls {
            let q = tf_sim::geom::segment::closest_point_to_segment(walker.pos, wall.a, wall.b);
            let into = walker.radius - q.distance(walker.pos);
            assert!(
                into <= WALL_TOLERANCE,
                "{what}, tick {}: {:?} is {into} m into a wall",
                run.tick,
                walker.id
            );
        }
    }
}

/// Remembers when each walker came in and its free walk time, so a test can see a
/// walker that is stuck without giving up (it would only count as "still walking").
#[derive(Default)]
struct Arrivals {
    seen: BTreeMap<AgentId, (f64, f64)>,
}

impl Arrivals {
    /// Call after every step.
    fn watch(&mut self, run: &StreetRun) {
        for walker in run.walkers.values() {
            self.seen
                .entry(walker.id)
                .or_insert((walker.spawn_tick as f64 * DT, walker.free_time_s));
        }
    }

    /// Every walker that came in at least twice its free walk time plus 20 s before the
    /// end of the run has arrived. Returns (walkers due, walkers arrived).
    fn check(&self, run: &StreetRun, what: &str) -> (u32, u32) {
        let end_s = run.tick as f64 * DT;
        let arrived: Vec<AgentId> = run
            .events()
            .iter()
            .filter_map(|e| match e.event {
                StreetEvent::WalkEnded { agent, .. } => Some(agent),
                _ => None,
            })
            .collect();
        let mut due = 0;
        for (id, (spawn_s, free_s)) in &self.seen {
            if spawn_s + 2.0 * free_s + 20.0 <= end_s {
                due += 1;
                assert!(
                    arrived.contains(id),
                    "{what}: {id:?} came in at {spawn_s} s (free walk {free_s:.1} s) and has \
                     not arrived by {end_s} s"
                );
            }
        }
        (due, arrived.len() as u32)
    }
}

struct CrowdMeasures {
    /// Correlation between the lateral position and the walking direction.
    lane_correlation: f64,
    /// Mean speed along the sidewalk against the walkers' free speed.
    speed_share: f64,
    max_overlap: f32,
    spawned: u32,
    arrived: u32,
    gave_up: u32,
    /// Still walking when the run ended (the grace time ran out).
    walking: u32,
    /// Walkers that had time to arrive (`Arrivals::check`); all of them did.
    due: u32,
}

/// Runs a counterflow on a sidewalk and measures it in the middle third.
fn counterflow(width: f32, per_hour: u32, seed: u64) -> (StreetRun, CrowdMeasures) {
    let length = 80.0;
    let flows = walk_flow("west", "east", per_hour) + &walk_flow("east", "west", per_hour);
    let level = sidewalk(length, width, "", &flows, 40.0);
    let mut run = new_run(&level, seed, Population::Careful);
    let (mut n, mut sum_y, mut sum_s, mut sum_yy, mut sum_ss, mut sum_ys) =
        (0.0f64, 0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut speed, mut free) = (0.0f64, 0.0f64);
    let mut deepest: f32 = 0.0;
    let mut arrivals = Arrivals::default();
    while !run.finished() {
        run.step();
        arrivals.watch(&run);
        deepest = deepest.max(max_overlap(&run));
        assert_inside(&run, "counterflow");
        for walker in run.walkers.values() {
            if walker.pos.x < length / 3.0 || walker.pos.x > 2.0 * length / 3.0 {
                continue;
            }
            let s = if heading_east(&run, walker) {
                1.0
            } else {
                -1.0
            };
            let y = f64::from(walker.pos.y - width / 2.0);
            n += 1.0;
            sum_y += y;
            sum_s += s;
            sum_yy += y * y;
            sum_ss += s * s;
            sum_ys += y * s;
            speed += f64::from(walker.vel.x) * s;
            free += walker.desired_speed;
        }
    }
    let cov = sum_ys / n - (sum_y / n) * (sum_s / n);
    let var_y = sum_yy / n - (sum_y / n).powi(2);
    let var_s = sum_ss / n - (sum_s / n).powi(2);
    let summary = run.summary();
    let (due, _) = arrivals.check(&run, "counterflow");
    let measures = CrowdMeasures {
        lane_correlation: cov / (var_y * var_s).sqrt(),
        speed_share: speed / free,
        max_overlap: deepest,
        spawned: summary.walkers.spawned,
        arrived: summary.walkers.arrived,
        gave_up: summary.walkers.gave_up,
        walking: run.walkers.len() as u32,
        due,
    };
    (run, measures)
}

#[test]
fn opposite_crowds_on_a_two_metre_sidewalk_form_lanes_and_slow_down_without_overlap() {
    for seed in 1..=3 {
        let (run, m) = counterflow(2.0, 5000, seed);
        let stats = run.walk_stats;
        println!(
            "seed {seed}: lanes {:.2}, speed share {:.2}, overlap {:.4} m, spawned {}, \
             arrived {}, walking {}, due {}, force overlap {:.3} m, held back {}, \
             force flips {}/{}, velocity flips {}/{}",
            m.lane_correlation,
            m.speed_share,
            m.max_overlap,
            m.spawned,
            m.arrived,
            m.walking,
            m.due,
            stats.max_force_overlap,
            stats.held_back,
            stats.force_flips,
            stats.force_samples,
            stats.velocity_flips,
            stats.velocity_samples
        );
        assert!(m.spawned >= 50, "seed {seed}: a crowd needs people");
        assert_eq!(m.gave_up, 0, "seed {seed}: nobody is stuck");
        assert_eq!(m.walking, 0, "seed {seed}: walkers still on the sidewalk");
        assert_eq!(m.arrived, m.spawned, "seed {seed}: walkers are missing");
        // The 20 Hz measures (stage A report): bounded by the constraints, no walker kept
        // back by a wall, no reversal of motion.
        assert_eq!(stats.held_back, 0, "seed {seed}");
        assert_eq!(stats.velocity_flips, 0, "seed {seed}");
        assert!(
            stats.max_force_overlap <= FORCE_OVERLAP_BOUND,
            "seed {seed}: the forces let bodies {} m into each other",
            stats.max_force_overlap
        );
        assert!(
            m.lane_correlation > 0.6,
            "seed {seed}: no lanes ({:.2})",
            m.lane_correlation
        );
        assert!(
            m.speed_share < 0.92,
            "seed {seed}: the crowd does not slow down ({:.2})",
            m.speed_share
        );
        assert!(
            m.max_overlap <= OVERLAP_TOLERANCE,
            "seed {seed}: bodies overlap by {} m",
            m.max_overlap
        );
    }
}

#[test]
fn a_lone_walker_keeps_its_pace_and_arrives() {
    // Along one sidewalk, then across junction arms to the school door.
    for (from, to) in [("west", "school"), ("south", "school"), ("north", "south")] {
        let level = quiet_school(200.0);
        let mut run = new_run(&level, 1, Population::Careful);
        let WalkSpawn::Spawned(ids) = run.add_walkers(from, to, Some(1)) else {
            panic!("{from} -> {to}: the walker comes in");
        };
        let desired = run.walkers[&ids[0]].desired_speed;
        let free = run.walkers[&ids[0]].free_time_s;
        let (mut speed, mut ticks) = (0.0, 0.0);
        while !run.finished() {
            run.step();
            assert_inside(&run, from);
            if let Some(walker) = run.walkers.get(&ids[0]) {
                speed += walker.speed();
                ticks += 1.0;
            }
        }
        let mean = speed / ticks;
        let ended = run.events().iter().find_map(|e| match e.event {
            StreetEvent::WalkEnded {
                time_s, delay_s, ..
            } => Some((time_s, delay_s)),
            _ => None,
        });
        let (time_s, delay_s) = ended.expect("the walker arrives");
        println!(
            "{from} -> {to}: {time_s} s for a free time of {free:.1} s, mean speed {mean:.2} \
             of {desired:.2} m/s"
        );
        assert!(
            delay_s <= LONE_PACE_TOLERANCE * free,
            "{from} -> {to}: {delay_s} s late"
        );
        assert!(
            (mean - desired).abs() <= LONE_PACE_TOLERANCE * desired,
            "{from} -> {to}: mean speed {mean} for {desired}"
        );
    }
}

#[test]
fn walkers_go_around_obstacles_and_never_enter_them() {
    let obstacles = r#"
        (kind: Tree, shape: Circle((center: (20.0, 1.5), radius: 0.6))),
        (kind: Bench, shape: Polygon([(30.0, 0.0), (32.0, 0.0), (32.0, 0.6), (30.0, 0.6)])),
        (kind: Planter, shape: Polygon([(40.0, 1.0), (41.0, 1.0), (41.0, 2.2), (40.0, 2.2)])),
        (kind: Bollard, shape: Circle((center: (48.0, 0.8), radius: 0.15))),
        (kind: Bollard, shape: Circle((center: (48.0, 2.2), radius: 0.15))),
    "#;
    let flows = walk_flow("west", "east", 1800) + &walk_flow("east", "west", 1800);
    let level = sidewalk(60.0, 3.0, obstacles, &flows, 40.0);
    for seed in 1..=3 {
        let mut run = new_run(&level, seed, Population::Mixed);
        let mut deepest: f32 = 0.0;
        let mut arrivals = Arrivals::default();
        while !run.finished() {
            run.step();
            arrivals.watch(&run);
            assert_inside(&run, "obstacles");
            deepest = deepest.max(max_overlap(&run));
        }
        let s = run.summary().walkers;
        let (due, arrived) = arrivals.check(&run, "obstacles");
        println!("seed {seed}: {s:?}, due {due}, arrived {arrived}, overlap {deepest:.4}");
        assert!(s.spawned >= 20, "seed {seed}: {s:?}");
        assert_eq!(s.gave_up, 0, "seed {seed}: {s:?}");
        assert_eq!(s.arrived, s.spawned, "seed {seed}: walkers are missing");
        assert!(
            deepest <= OVERLAP_TOLERANCE,
            "seed {seed}: overlap {deepest}"
        );
    }
}

#[test]
fn a_group_stays_together() {
    for (from, to) in [("south", "school"), ("west", "east")] {
        let level = quiet_school(200.0);
        let mut run = new_run(&level, 4, Population::Mixed);
        let WalkSpawn::Spawned(ids) = run.add_walkers(from, to, Some(3)) else {
            panic!("the group comes in");
        };
        assert_eq!(ids.len(), 3);
        // The furthest a member gets from the group's middle.
        let mut widest: f32 = 0.0;
        while !run.finished() {
            run.step();
            let members: Vec<Vec2> = ids
                .iter()
                .filter_map(|id| run.walkers.get(id))
                .map(|w| w.pos)
                .collect();
            if members.len() < 3 {
                continue;
            }
            let middle = members.iter().copied().sum::<Vec2>() / 3.0;
            for member in &members {
                widest = widest.max(member.distance(middle));
            }
        }
        let arrivals: Vec<f64> = run
            .events()
            .iter()
            .filter_map(|e| match e.event {
                StreetEvent::WalkEnded { time_s, .. } => Some(time_s),
                _ => None,
            })
            .collect();
        println!("{from} -> {to}: widest {widest:.2} m, arrivals {arrivals:?}");
        assert_eq!(arrivals.len(), 3, "every member arrives");
        assert!(
            widest <= 2.0,
            "{from} -> {to}: a member got {widest} m from the middle"
        );
        let spread = arrivals.iter().cloned().fold(f64::MIN, f64::max)
            - arrivals.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread <= 3.0, "{from} -> {to}: arrivals {arrivals:?}");
    }
}

#[test]
fn walks_are_deterministic_and_one_more_walker_does_not_change_the_others_draws() {
    let (a, _) = counterflow(2.0, 2400, 5);
    let (b, _) = counterflow(2.0, 2400, 5);
    assert_eq!(a.log_hash(), b.log_hash());
    assert_eq!(a.events(), b.events());
    let (c, _) = counterflow(2.0, 2400, 6);
    assert_ne!(a.log_hash(), c.log_hash());

    let level = quiet_school(60.0);
    let spawn = |extra: bool| {
        let mut run = new_run(&level, 9, Population::Mixed);
        let mut ids = Vec::new();
        for (from, to) in [("west", "east"), ("east", "west"), ("north", "south")] {
            if let WalkSpawn::Spawned(new) = run.add_walkers(from, to, None) {
                ids.extend(new);
            }
        }
        if extra {
            assert!(matches!(
                run.add_walkers("south", "school", Some(1)),
                WalkSpawn::Spawned(_)
            ));
        }
        (run, ids)
    };
    let (without, ids) = spawn(false);
    let (with, _) = spawn(true);
    assert_eq!(with.walkers.len(), without.walkers.len() + 1);
    for id in ids {
        let (x, y) = (&without.walkers[&id], &with.walkers[&id]);
        assert_eq!(x.traits, y.traits, "{id:?}");
        assert_eq!(x.archetype, y.archetype, "{id:?}");
        assert_eq!(x.pos, y.pos, "{id:?}");
    }
}

#[test]
fn pedestrians_spawn_from_the_level_demand_and_do_not_change_the_car_traffic() {
    let level = LevelFile::load("carrefour-ecole").expect("the test level loads");
    let mut cars_only = level.clone();
    for slice in &mut cars_only.demand {
        slice.flows.retain(|flow| flow.mode != Mode::Pedestrian);
    }
    for (seed, population, scale) in [
        (1, Population::Mixed, 1.0),
        (2, Population::Careless, 1.0),
        (1, Population::Careless, 4.0),
        (2, Population::Careless, 4.0),
    ] {
        let options = RunOptions {
            seed,
            population,
            demand_scale: scale,
        };
        let mut with = StreetRun::new(&level, &Design::new(&level.id), options).expect("builds");
        let mut deepest: f32 = 0.0;
        let mut arrivals = Arrivals::default();
        while !with.finished() {
            with.step();
            arrivals.watch(&with);
            assert_inside(&with, "carrefour");
            deepest = deepest.max(max_overlap(&with));
        }
        let (due, arrived) = arrivals.check(&with, "carrefour");
        let mut without =
            StreetRun::new(&cars_only, &Design::new(&level.id), options).expect("builds");
        without.run_to_end();
        let vehicle_events = |run: &StreetRun| -> Vec<StreetEvent> {
            run.events()
                .iter()
                .map(|e| e.event.clone())
                .filter(|e| {
                    !matches!(
                        e,
                        StreetEvent::WalkerSpawned { .. }
                            | StreetEvent::WalkEnded { .. }
                            | StreetEvent::Bump { .. }
                            | StreetEvent::RunEnded { .. }
                    ) && !matches!(e, StreetEvent::GaveUp { agent, .. } if is_walker(*agent))
                })
                .collect()
        };
        assert_eq!(
            vehicle_events(&with),
            vehicle_events(&without),
            "seed {seed} {population:?} x{scale}"
        );
        let s = with.summary();
        println!(
            "seed {seed} {population:?} x{scale}: {:?}, due {due}, arrived {arrived}, \
             overlap {deepest:.4}",
            s.walkers
        );
        assert!(s.walkers.spawned >= 10, "seed {seed}: {:?}", s.walkers);
        assert_eq!(s.walkers.gave_up, 0, "seed {seed}: {:?}", s.walkers);
        assert!(
            deepest <= OVERLAP_TOLERANCE,
            "seed {seed}: overlap {deepest}"
        );
        // Groups come from the spawner too.
        let grouped = with.events().iter().any(
            |e| matches!(e.event, StreetEvent::WalkerSpawned { group_size, .. } if group_size > 1),
        );
        assert!(grouped, "seed {seed}: no group spawned");
    }
}

#[test]
fn a_dense_crowd_annoys_more_than_a_light_one() {
    let annoyance = |per_hour: u32| {
        let (run, _) = counterflow(2.0, per_hour, 2);
        let s = run.summary().walkers;
        (s.total_annoyance / f64::from(s.arrived), s.mean_crowded_s)
    };
    let (light, light_crowded) = annoyance(600);
    let (dense, dense_crowded) = annoyance(4000);
    println!(
        "annoyance per walker: light {light:.3} ({light_crowded:.1} s crowded), dense {dense:.3} ({dense_crowded:.1} s)"
    );
    assert!(dense > light);
    assert!(dense_crowded > light_crowded);
}

#[test]
fn a_pedestrian_flow_counts_people_not_groups() {
    // 600 people an hour for 10 minutes: 100 people a run, in groups or alone.
    let level = sidewalk(30.0, 3.0, "", &walk_flow("west", "east", 600), 600.0);
    let (mut people, mut groups) = (0, 0);
    for seed in 1..=10 {
        let mut run = new_run(&level, seed, Population::Mixed);
        run.run_to_end();
        for e in run.events() {
            if let StreetEvent::WalkerSpawned { agent, group, .. } = e.event {
                people += 1;
                if agent == group {
                    groups += 1;
                }
            }
        }
    }
    println!("{people} people in {groups} spawns over 10 runs (1 000 expected)");
    assert!((900..=1100).contains(&people), "{people} people");
    assert!(groups < people, "groups come in too");
}

#[test]
fn a_thin_barrier_is_never_walked_through() {
    // A 0.2 m barrier across most of the sidewalk, open at the south side.
    let barrier = r#"
        (kind: Barrier, shape: Polygon([(30.0, 0.0), (30.2, 0.0), (30.2, 2.2), (30.0, 2.2)])),
    "#;
    let flows = walk_flow("west", "east", 1200) + &walk_flow("east", "west", 1200);
    let level = sidewalk(60.0, 3.0, barrier, &flows, 40.0);
    let edges = [
        (Vec2::new(30.0, 0.0), Vec2::new(30.2, 0.0)),
        (Vec2::new(30.2, 0.0), Vec2::new(30.2, 2.2)),
        (Vec2::new(30.2, 2.2), Vec2::new(30.0, 2.2)),
        (Vec2::new(30.0, 2.2), Vec2::new(30.0, 0.0)),
    ];
    for seed in 1..=3 {
        let mut run = new_run(&level, seed, Population::Mixed);
        let mut arrivals = Arrivals::default();
        let mut through = 0;
        while !run.finished() {
            run.step();
            arrivals.watch(&run);
            assert_inside(&run, "barrier");
            for walker in run.walkers.values() {
                let crossed = edges.iter().any(|(a, b)| {
                    tf_sim::geom::polygon::segment_intersection(walker.prev_pos, walker.pos, *a, *b)
                        .is_some()
                });
                through += usize::from(crossed);
            }
        }
        let (due, arrived) = arrivals.check(&run, "barrier");
        println!("seed {seed}: due {due}, arrived {arrived}");
        assert_eq!(through, 0, "seed {seed}: a walker went through the barrier");
        let s = run.summary().walkers;
        assert!(s.spawned >= 20, "seed {seed}: people walk past the barrier");
        assert_eq!(s.arrived, s.spawned, "seed {seed}: walkers are missing");
    }
}
