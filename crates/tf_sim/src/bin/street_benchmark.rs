//! Sim benchmark for the street game (pivot plan Phase 5): how fast one level runs
//! headless, at several demand scales. For each scale it runs the full level with the
//! empty design, once per seed, and prints one line: the vehicles and the walkers alive
//! per tick (mean and max, sampled after each tick), the ticks, the wall-clock time of
//! the stepping, the mean and worst cost of one tick, and the speed against real time
//! (one tick is 1 / `TICK_RATE` s). Building the run is not timed. The wall clock is
//! read here, in the bin only: the sim itself never reads time.
//! How to read it: the cost per tick grows with the agents alive, so the scale of the
//! line is the load; a speed below 1x means the level cannot run in real time on this
//! machine. The worst tick shows the hitches a frame would see if the sim ran on the
//! main thread.
//! Run: `cargo run -p tf_sim --release --bin street_benchmark -- [level-id] [--seeds <n>]
//! [--scales <x,y,...>]`. Defaults: `carrefour-ecole`, 3 seeds, scales 1, 2.5, 4, 8.
//! Seeds are the level seed plus 0, 1, 2 ... (`--seeds` of them).

use std::process::ExitCode;
use std::time::{Duration, Instant};

use tf_sim::street::design::Design;
use tf_sim::street::level::{LevelError, LevelFile, level_ids};
use tf_sim::street::run::{RunOptions, StreetRun, TICK_RATE};

const USAGE: &str = "usage: street_benchmark [level-id] [--seeds <n>] [--scales <x,y,...>]";
const DEFAULT_LEVEL: &str = "carrefour-ecole";
const DEFAULT_SEEDS: u32 = 3;
const DEFAULT_SCALES: [f64; 4] = [1.0, 2.5, 4.0, 8.0];

struct Args {
    level: String,
    seeds: u32,
    scales: Vec<f64>,
}

/// What one demand scale adds up to, over all its seeds.
#[derive(Default)]
struct Totals {
    /// Ticks run (one sample of the alive counts per tick).
    samples: u64,
    vehicles_sum: u64,
    vehicles_max: u64,
    walkers_sum: u64,
    walkers_max: u64,
    /// Time spent inside `StreetRun::step`, summed.
    step: Duration,
    /// Slowest single `step`.
    worst: Duration,
    /// Stepping time of the whole runs (includes the per-tick clock reads).
    wall: Duration,
}

fn parse_seeds(text: &str) -> Result<u32, String> {
    match text.parse::<u32>() {
        Ok(seeds) if seeds >= 1 => Ok(seeds),
        _ => Err(format!("seeds {text} must be a whole number from 1")),
    }
}

fn parse_scales(text: &str) -> Result<Vec<f64>, String> {
    let mut scales = Vec::new();
    for part in text.split(',') {
        match part.trim().parse::<f64>() {
            Ok(scale) if scale.is_finite() && scale >= 0.0 => scales.push(scale),
            _ => return Err(format!("scale {part} must be a number of 0 or more")),
        }
    }
    Ok(scales)
}

fn parse(raw: &[String]) -> Result<Args, String> {
    let mut args = Args {
        level: DEFAULT_LEVEL.to_string(),
        seeds: DEFAULT_SEEDS,
        scales: DEFAULT_SCALES.to_vec(),
    };
    let mut level_given = false;
    let mut rest = raw.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--seeds" => {
                let text = rest.next().ok_or("--seeds needs a value")?;
                args.seeds = parse_seeds(text)?;
            }
            "--scales" => {
                let text = rest.next().ok_or("--scales needs a value")?;
                args.scales = parse_scales(text)?;
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            id => {
                if level_given {
                    return Err("give one level id only".to_string());
                }
                args.level = id.to_string();
                level_given = true;
            }
        }
    }
    if args.scales.is_empty() {
        return Err("--scales needs at least one scale".to_string());
    }
    Ok(args)
}

/// Runs the level once per seed at one demand scale and adds up the samples.
fn measure(level: &LevelFile, design: &Design, scale: f64, seeds: u32) -> Result<Totals, String> {
    let mut totals = Totals::default();
    for k in 0..seeds {
        let mut options = RunOptions::for_level(level);
        options.demand_scale = scale;
        options.seed = level.seed.wrapping_add(u64::from(k));
        let mut run = StreetRun::new(level, design, options)
            .map_err(|error| format!("the design does not build: {error:?}"))?;

        let start = Instant::now();
        while !run.finished() {
            let tick = Instant::now();
            run.step();
            let cost = tick.elapsed();
            totals.step += cost;
            totals.worst = totals.worst.max(cost);

            let vehicles = run.agents.len() as u64;
            let walkers = run.walkers.len() as u64;
            totals.samples += 1;
            totals.vehicles_sum += vehicles;
            totals.vehicles_max = totals.vehicles_max.max(vehicles);
            totals.walkers_sum += walkers;
            totals.walkers_max = totals.walkers_max.max(walkers);
        }
        totals.wall += start.elapsed();
    }
    Ok(totals)
}

fn print_line(scale: f64, totals: &Totals, seeds: u32) {
    let samples = totals.samples.max(1) as f64;
    let mean_cost_us = totals.step.as_secs_f64() * 1e6 / samples;
    let worst_us = totals.worst.as_secs_f64() * 1e6;
    let wall_s = totals.wall.as_secs_f64();
    let sim_s = totals.samples as f64 / f64::from(TICK_RATE);
    let speed = if wall_s > 0.0 {
        sim_s / wall_s
    } else {
        f64::INFINITY
    };
    println!(
        "scale {scale}: vehicles mean {:.1} max {} | walkers mean {:.1} max {} | \
         seeds {seeds}, ticks {} | wall {:.1} ms | mean {mean_cost_us:.1} us per tick, \
         worst {worst_us:.0} us | {speed:.1}x real time",
        totals.vehicles_sum as f64 / samples,
        totals.vehicles_max,
        totals.walkers_sum as f64 / samples,
        totals.walkers_max,
        totals.samples,
        wall_s * 1000.0,
    );
}

fn run(args: &Args) -> Result<(), String> {
    let level = LevelFile::load(&args.level).map_err(|error| match error {
        LevelError::Unknown(id) => format!(
            "unknown level {id}; known levels: {}",
            level_ids().collect::<Vec<_>>().join(", ")
        ),
        other => format!("level {}: {other}", args.level),
    })?;
    let design = Design::new(&level.id);
    println!(
        "level {} | empty design | mixed population | tick {:.2} s",
        level.id,
        1.0 / f64::from(TICK_RATE)
    );
    for &scale in &args.scales {
        let totals = measure(&level, &design, scale, args.seeds)?;
        print_line(scale, &totals, args.seeds);
    }
    Ok(())
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse(&raw) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("street_benchmark: {error}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("street_benchmark: {error}");
            ExitCode::from(2)
        }
    }
}
