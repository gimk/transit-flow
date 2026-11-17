//! Headless run of one street level (pivot plan §5): the day's traffic on the level's
//! street, with the empty design or a design file, stepped at 20 Hz. Prints the summary
//! (vehicles, then pedestrians under `walkers`), the crowd measures and the replay
//! hash; `--events` prints the event log first. Use it to check a design or a
//! population without the window.
//! Run: `cargo run -p tf_sim --release --bin run_level -- <level-id> [--design <file.json>]
//! [--seed <n>] [--population careful|mixed|careless] [--demand-scale <x>] [--stage <n>]
//! [--events]`. `--stage` sets the demand scale of that star stage (from 1) and prints the
//! stage report; `--demand-scale` overrides the scale.

use std::process::ExitCode;
use std::time::Instant;

use tf_sim::street::design::Design;
use tf_sim::street::level::{LevelError, LevelFile, level_ids};
use tf_sim::street::run::agent::Population;
use tf_sim::street::run::{RunOptions, StreetRun, TICK_RATE};
use tf_sim::street::score::{StageReport, stage_options, stage_report};

const USAGE: &str = "usage: run_level <level-id> [--design <file.json>] [--seed <n>] \
[--population careful|mixed|careless] [--demand-scale <x>] [--stage <n>] [--events]";

struct Args {
    level: String,
    design: Option<String>,
    seed: Option<u64>,
    population: Population,
    demand_scale: Option<f64>,
    stage: Option<usize>,
    events: bool,
}

fn value<'a>(rest: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<String, String> {
    rest.next()
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

fn population(text: &str) -> Result<Population, String> {
    match text {
        "careful" => Ok(Population::Careful),
        "mixed" => Ok(Population::Mixed),
        "careless" => Ok(Population::Careless),
        other => Err(format!("unknown population {other}")),
    }
}

fn parse(raw: &[String]) -> Result<Args, String> {
    let mut level = None;
    let mut args = Args {
        level: String::new(),
        design: None,
        seed: None,
        population: Population::Mixed,
        demand_scale: None,
        stage: None,
        events: false,
    };
    let mut rest = raw.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--design" => args.design = Some(value(&mut rest, arg)?),
            "--seed" => {
                let text = value(&mut rest, arg)?;
                args.seed = Some(
                    text.parse::<u64>()
                        .map_err(|_| format!("seed {text} is not a whole number"))?,
                );
            }
            "--population" => args.population = population(&value(&mut rest, arg)?)?,
            "--demand-scale" => {
                let text = value(&mut rest, arg)?;
                args.demand_scale = Some(match text.parse::<f64>() {
                    Ok(scale) if scale.is_finite() && scale >= 0.0 => scale,
                    _ => return Err(format!("demand scale {text} must be a number of 0 or more")),
                });
            }
            "--stage" => {
                let text = value(&mut rest, arg)?;
                args.stage = match text.parse::<usize>() {
                    Ok(stage) if stage >= 1 => Some(stage),
                    _ => return Err(format!("stage {text} must be a whole number from 1")),
                };
            }
            "--events" => args.events = true,
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            id => {
                if level.replace(id.to_string()).is_some() {
                    return Err("give one level id only".to_string());
                }
            }
        }
    }
    args.level = level.ok_or_else(|| "missing level id".to_string())?;
    Ok(args)
}

fn print_report(report: &StageReport) {
    let verdict = if report.passed { "PASSED" } else { "FAILED" };
    println!(
        "stage {}: {verdict}, score {:.1} (needs {:.0}); safety -{:.1}, serenity -{:.1}, life +{:.1}",
        report.stage,
        report.score,
        report.pass_score,
        report.parts.safety,
        report.parts.serenity,
        report.parts.life
    );
    if let Some(reason) = &report.hard_fail {
        println!("hard fail: {reason:?}");
    }
    for mode in &report.per_mode {
        println!(
            "  {:?}: spawned {}, arrived {}, gave up {}, mean delay {:.1} s, mean annoyance {:.3}",
            mode.mode,
            mode.spawned,
            mode.arrived,
            mode.gave_up,
            mode.mean_delay_s,
            mode.mean_annoyance
        );
    }
    for reason in &report.top_reasons {
        println!("  why: {reason}");
    }
    for incident in &report.incidents {
        println!("  [{:.1} s] {}", incident.time_s, incident.text);
    }
}

fn run(args: &Args) -> Result<(), String> {
    let level = LevelFile::load(&args.level).map_err(|error| match error {
        LevelError::Unknown(id) => format!(
            "unknown level {id}; known levels: {}",
            level_ids().collect::<Vec<_>>().join(", ")
        ),
        other => format!("level {}: {other}", args.level),
    })?;
    let design = match &args.design {
        None => Design::new(&level.id),
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("cannot read design {path}: {error}"))?;
            let design =
                Design::from_json(&text).map_err(|error| format!("design {path}: {error}"))?;
            if design.level != level.id {
                return Err(format!(
                    "design {path} is for level {}, not {}",
                    design.level, level.id
                ));
            }
            design
        }
    };

    let mut options = RunOptions::for_level(&level);
    options.population = args.population;
    if let Some(stage) = args.stage {
        options = stage_options(&level, stage).ok_or_else(|| {
            format!(
                "level {} has {} stages, not {stage}",
                level.id,
                level.stages.len()
            )
        })?;
        options.population = args.population;
    }
    if let Some(scale) = args.demand_scale {
        options.demand_scale = scale;
    }
    if let Some(seed) = args.seed {
        options.seed = seed;
    }
    let (seed, population, scale) = (options.seed, args.population, options.demand_scale);
    let mut run = StreetRun::new(&level, &design, options)
        .map_err(|error| format!("the design does not build: {error:?}"))?;

    println!(
        "level {} seed {seed} population {} scale {}",
        level.id,
        format!("{population:?}").to_lowercase(),
        scale
    );

    let start = Instant::now();
    run.run_to_end();
    let wall_s = start.elapsed().as_secs_f64();

    if args.events {
        for logged in run.events() {
            println!(
                "{}",
                serde_json::to_string(logged).expect("an event always serializes")
            );
        }
    }
    let summary = run.summary();
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).expect("a summary always serializes")
    );
    let walk = run.walk_stats;
    println!(
        "walkers: deepest overlap {:.3} m (from the forces alone {:.3} m), held back {}, \
         force flips {}/{}, still walking {}",
        walk.max_overlap,
        walk.max_force_overlap,
        walk.held_back,
        walk.force_flips,
        walk.force_samples,
        run.walkers.len()
    );
    println!("log hash: {:016x}", run.log_hash());
    if let Some(stage) = args.stage {
        print_report(&stage_report(&run, &level, stage));
    }

    let sim_s = summary.ticks as f64 / f64::from(TICK_RATE);
    let speed = if wall_s > 0.0 {
        sim_s / wall_s
    } else {
        f64::INFINITY
    };
    println!(
        "ran {:.1} ms wall-clock for {sim_s:.1} sim s: {speed:.1}x real time",
        wall_s * 1000.0
    );
    Ok(())
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse(&raw) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("run_level: {error}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("run_level: {error}");
            ExitCode::from(2)
        }
    }
}
