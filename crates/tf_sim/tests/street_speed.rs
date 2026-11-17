//! Speed from the look of the street (pivot plan §5.7, addition 1) on the test level
//! "Carrefour école": bollards at the lane edges and a narrower lane slow drivers down, a
//! careful population never wants more than the limit, and careless drivers on a wide
//! street with a 30 km/h sign go faster than the sign.

use glam::Vec2;
use tf_sim::geom::Circle;
use tf_sim::street::level::{LevelFile, ObstacleDef, ObstacleKind, Shape};
use tf_sim::street::run::{Population, RunOptions, StreetRun};
use tf_sim::street::space::StripUse;
use tf_sim::street::speed::street_speeds;
use tf_sim::street::{Design, DesignCommand, Street};

const LEVEL: &str = "carrefour-ecole";
const LIMIT: f64 = 30.0 / 3.6;
const SEEDS: std::ops::RangeInclusive<u64> = 1..=4;

fn level() -> LevelFile {
    LevelFile::load(LEVEL).expect("the test level loads")
}

/// Bollards every 3 m along the three edges of the two car lanes of segment 2 (east of
/// the junction), from x 90 to 145.
fn with_bollards(mut level: LevelFile) -> LevelFile {
    for edge_y in [71.4_f32, 74.5, 77.6] {
        let mut x = 90.0_f32;
        while x <= 145.0 {
            level.obstacles.push(ObstacleDef {
                kind: ObstacleKind::Bollard,
                shape: Shape::Circle(Circle {
                    center: Vec2::new(x, edge_y),
                    radius: 0.15,
                }),
            });
            x += 3.0;
        }
    }
    level
}

/// Mean speed of the vehicles on the stretch of segment 2 between x 100 and 140, over
/// the whole run and every seed, m/s.
fn mean_speed(level: &LevelFile, design: &Design, population: Population) -> f64 {
    let (mut sum, mut count) = (0.0, 0_u64);
    for seed in SEEDS {
        let options = RunOptions {
            seed,
            population,
            demand_scale: 0.4,
        };
        let mut run = StreetRun::new(level, design, options).expect("the level builds");
        while !run.finished() {
            run.step();
            for agent in run.agents.values() {
                if (100.0..140.0).contains(&agent.pos.x) && (agent.pos.y - 75.0).abs() < 6.0 {
                    sum += agent.speed;
                    count += 1;
                }
            }
        }
    }
    assert!(count > 100, "enough samples: {count}");
    sum / count as f64
}

/// A 50 km/h sign on segment 2: with the 30 km/h sign every compliant driver is held to the
/// limit and the look of the street shows little.
fn fifty() -> Design {
    let mut design = Design::new(LEVEL);
    design.commands.push(DesignCommand::SetSpeedLimit {
        segment: 2,
        kmh: 50.0,
    });
    design
}

fn lane_speeds_of(street: &Street, segment: i32) -> Vec<f64> {
    let speeds = street_speeds(street);
    street
        .segment_lanes(segment)
        .filter(|lane| lane.access.intersects(tf_sim::street::space::Access::CAR))
        .map(|lane| speeds[&lane.id])
        .collect()
}

#[test]
fn bollards_at_the_edge_slow_the_street_and_the_drivers() {
    let plain = level();
    let bollards = with_bollards(level());
    let design = fifty();
    let before = lane_speeds_of(&Street::from_level(&plain).unwrap(), 2);
    let after = lane_speeds_of(&Street::from_level(&bollards).unwrap(), 2);
    for (b, a) in before.iter().zip(&after) {
        assert!(a < b, "the street speed drops: {b} -> {a}");
    }
    let mean_before = mean_speed(&plain, &design, Population::Mixed);
    let mean_after = mean_speed(&bollards, &design, Population::Mixed);
    assert!(
        mean_after < mean_before - 0.3,
        "mean speed {mean_before:.2} -> {mean_after:.2} m/s"
    );
}

#[test]
fn a_narrower_lane_slows_the_drivers() {
    let plain = level();
    let design = fifty();
    let mut narrow = fifty();
    // Widen the north sidewalk of segment 2 by 0.5 m: the lane next to it gets narrower.
    narrow.commands.push(DesignCommand::MoveBoundary {
        segment: 2,
        boundary: 1,
        delta: 0.5,
    });
    let street = Street::from_level_and_design(&plain, &narrow).unwrap();
    assert!(
        lane_speeds_of(&street, 2)
            .iter()
            .zip(lane_speeds_of(&Street::from_level(&plain).unwrap(), 2))
            .any(|(a, b)| *a < b)
    );
    let before = mean_speed(&plain, &design, Population::Mixed);
    let after = mean_speed(&plain, &narrow, Population::Mixed);
    assert!(
        after < before,
        "mean speed {before:.2} -> {after:.2} m/s with a narrower lane"
    );
}

#[test]
fn a_careful_population_never_goes_over_the_limit() {
    let level = level();
    let design = Design::new(LEVEL);
    for seed in SEEDS {
        let options = RunOptions {
            seed,
            population: Population::Careful,
            demand_scale: 1.0,
        };
        let mut run = StreetRun::new(&level, &design, options).unwrap();
        let mut top = 0.0_f64;
        while !run.finished() {
            run.step();
            for agent in run.agents.values() {
                top = top.max(agent.speed);
            }
        }
        assert!(top <= LIMIT + 0.1, "seed {seed}: top speed {top}");
    }
}

#[test]
fn careless_drivers_on_a_wide_street_go_over_the_sign() {
    let mut level = level();
    // Segment 2 without parking: two 4.5 m lanes and 2 m sidewalks.
    let strips = &mut level.segments[1].profile.strips;
    strips.retain(|s| s.use_ != StripUse::Parking);
    for strip in strips.iter_mut() {
        strip.width = if strip.use_ == StripUse::CarLane {
            4.5
        } else {
            2.0
        };
    }
    let street = Street::from_level(&level).expect("the wide street builds");
    for speed in lane_speeds_of(&street, 2) {
        assert!(speed > LIMIT * 1.5, "the street asks for {speed}");
    }
    let design = Design::new(LEVEL);
    let mean = mean_speed(&level, &design, Population::Careless);
    assert!(mean > LIMIT, "mean speed {mean:.2} m/s against {LIMIT:.2}");
}
