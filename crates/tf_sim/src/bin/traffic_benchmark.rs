//! Port of `benchmarks/traffic_benchmark.gd`: builds a 20 x 20 street grid, fills it with
//! N stress vehicles and reports the average and worst cost of one simulation tick.
//! Run: `cargo run -p tf_sim --release --bin traffic_benchmark`.
//! (The GDScript version also timed filling the renderer's MultiMesh; that belongs to the
//! game crate.)

use std::time::Instant;

use glam::Vec2;
use tf_sim::Simulation;
use tf_sim::events::SimEvent;
use tf_sim::grid::CELL_SIZE;
use tf_sim::roads::{RoadNetwork, RoadType};

const COUNTS: [usize; 4] = [500, 1000, 2000, 5000];
const GRID_ROADS: i32 = 20;
const GRID_SPACING: i32 = 3;
const MAX_RAMP_TICKS: usize = 1500;
const MEASURE_TICKS: usize = 200;

fn build_grid_roads() -> serde_json::Value {
    let mut roads = RoadNetwork::new();
    let local = RoadType::load_by_id("local_2lane").unwrap();
    let g = CELL_SIZE as f32;
    let extent = ((GRID_ROADS - 1) * GRID_SPACING) as f32;
    let mut out = Vec::new();
    for i in 0..GRID_ROADS {
        let offset = (2 + i * GRID_SPACING) as f32;
        roads.add_road(
            Vec2::new(2.0, offset) * g,
            Vec2::new(2.0 + extent, offset) * g,
            local,
            0,
            &mut out,
        );
        roads.add_road(
            Vec2::new(offset, 2.0) * g,
            Vec2::new(offset, 2.0 + extent) * g,
            local,
            0,
            &mut out,
        );
    }
    roads.to_dict()
}

fn run(road_data: &serde_json::Value, target: usize) {
    let mut sim = Simulation::with_roads(RoadNetwork::from_dict(road_data));
    sim.stress.target_count = target;
    for _ in 0..MAX_RAMP_TICKS {
        sim.step();
        if sim.vehicles.vehicles.len() >= target {
            break;
        }
    }
    sim.take_view_events();
    let mut arrivals = 0;
    let mut total = 0.0;
    let mut worst = 0.0f64;
    for _ in 0..MEASURE_TICKS {
        let start = Instant::now();
        sim.step();
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        total += elapsed;
        worst = worst.max(elapsed);
        arrivals += sim
            .take_view_events()
            .iter()
            .filter(|e| matches!(e, SimEvent::VehicleArrived(_)))
            .count();
    }
    println!(
        "| {target} | {} | {:.2} | {:.2} | {} | {:.1} |",
        sim.vehicles.vehicles.len(),
        total / MEASURE_TICKS as f64,
        worst,
        sim.vehicles.stuck_count(30.0),
        arrivals as f64 / (MEASURE_TICKS as f64 * Simulation::TICK_DT),
    );
}

fn main() {
    let road_data = build_grid_roads();
    println!("Grid: {GRID_ROADS} x {GRID_ROADS} roads, {GRID_SPACING} cells apart");
    println!("| target | vehicles | avg tick ms | worst tick ms | stuck >30s | arrivals/s |");
    println!("|---|---|---|---|---|---|");
    for target in COUNTS {
        run(&road_data, target);
    }
}
