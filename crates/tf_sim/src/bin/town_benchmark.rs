//! R3 benchmark: the Town for a full game day, then the Town again with 2 000 stress
//! vehicles on top for two sim minutes; average and worst cost of a tick. Same runs as
//! `migration/probes/town_benchmark.gd` (the GDScript side).
//! Run: `cargo run -p tf_sim --release --bin town_benchmark`.

use std::time::Instant;

use tf_sim::Simulation;
use tf_sim::clock::SimClock;
use tf_sim::debug::scenarios;

fn run(label: &str, stress: usize, ticks: usize) {
    let mut sim = Simulation::new();
    scenarios::build_town(&mut sim);
    sim.stress.target_count = stress;
    let (mut total, mut worst, mut peak) = (0.0f64, 0.0f64, 0);
    for _ in 0..ticks {
        let start = Instant::now();
        sim.step();
        let cost = start.elapsed().as_secs_f64() * 1000.0;
        total += cost;
        worst = worst.max(cost);
        peak = peak.max(sim.vehicles.vehicles.len());
        sim.take_view_events();
    }
    println!(
        "{label}: {ticks} ticks, {:.3} ms/tick average, {worst:.2} ms worst, peak {peak} vehicles, {} citizens",
        total / ticks as f64,
        sim.citizens.citizens.len()
    );
}

fn main() {
    let day = (24.0 * 60.0 / SimClock::GAME_MINUTES_PER_SIM_SECOND
        * f64::from(Simulation::TICK_RATE)) as usize;
    run("Town, one day", 0, day);
    run(
        "Town + 2000 stress, 2 min",
        2000,
        Simulation::TICK_RATE as usize * 120,
    );
}
