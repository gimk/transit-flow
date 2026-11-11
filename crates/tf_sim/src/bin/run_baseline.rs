//! Port of `migration/run_baseline.gd`: runs the metric scenarios of the migration plan
//! (§3) on the Rust sim and prints each metric next to Godot's value (from
//! `migration/baseline.md`) and its tolerance band, with a verdict.
//! Run: `cargo run -p tf_sim --release --bin run_baseline`. Exits 1 if a metric is
//! outside its band. The scenarios and measurements copy the GUT tests they come from.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use glam::{IVec2, Vec2};
use tf_sim::Simulation;
use tf_sim::city::citizen::{Mode, State};
use tf_sim::debug::checks::{connector_conflict_violations, min_queue_gap};
use tf_sim::debug::scenarios;
use tf_sim::events::SimEvent;
use tf_sim::grid::CELL_SIZE;
use tf_sim::ids::{CitizenId, SegmentId};
use tf_sim::roads::{JunctionControl, RoadType};
use tf_sim::traffic::vehicle::Kind;
use tf_sim::traffic::vehicle_system::MIN_GAP;

const G: f32 = CELL_SIZE as f32;
const BOTTLENECK_ORIGIN: IVec2 = IVec2::new(30, 30);
const DAY_TICKS: u32 = 5760;
/// The whole morning: every commuter is at work after ~90 s in the mornings.
const MORNING_TICKS: u32 = Simulation::TICK_RATE * 120;

/// How a metric is compared with Godot's.
#[derive(Clone, Copy)]
enum Band {
    Exact,
    Points(f64),
    Percent(f64),
    /// At most Godot + n.
    AtMost(f64),
    /// At least this value.
    AtLeast(f64),
}

impl Band {
    fn text(self) -> String {
        match self {
            Band::Exact => "exact".into(),
            Band::Points(n) => format!("± {n}"),
            Band::Percent(n) => format!("± {n}%"),
            Band::AtMost(n) => format!("≤ Godot + {n}"),
            Band::AtLeast(n) => format!("≥ {n}"),
        }
    }

    fn holds(self, rust: f64, godot: f64) -> bool {
        match self {
            Band::Exact => (rust - godot).abs() < 1e-9,
            Band::Points(n) => (rust - godot).abs() <= n,
            Band::Percent(n) => (rust - godot).abs() <= godot.abs() * n / 100.0,
            Band::AtMost(n) => rust <= godot + n,
            Band::AtLeast(n) => rust >= n,
        }
    }
}

struct Report {
    failures: Vec<String>,
}

impl Report {
    fn row(&mut self, metric: &str, rust: f64, godot: f64, decimals: usize, band: Band) {
        let ok = band.holds(rust, godot);
        println!(
            "| {metric} | {rust:.decimals$} | {godot:.decimals$} | {} | {} |",
            band.text(),
            if ok { "ok" } else { "OUT" }
        );
        if !ok {
            self.failures.push(metric.to_string());
        }
    }
}

fn road_type(id: &str) -> &'static RoadType {
    RoadType::load_by_id(id).expect("a built-in road type")
}

fn add_road(sim: &mut Simulation, from: Vec2, to: Vec2, type_id: &str) {
    let road_type = road_type(type_id);
    sim.call(|sim, out| {
        sim.roads.add_road(from * G, to * G, road_type, 0, out);
    });
}

fn main() {
    let started = Instant::now();
    let mut report = Report {
        failures: Vec::new(),
    };
    println!("# Rust baseline\n");
    println!(
        "The metrics of `migration/baseline.md` on the Rust sim, next to Godot's, by `cargo run -p tf_sim --release --bin run_baseline`.\n"
    );
    town_day(&mut report);
    bottleneck_mornings(&mut report);
    bottleneck_flow(&mut report);
    stress_grids(&mut report);
    println!("\nRecorded in {:.0} s.", started.elapsed().as_secs_f64());
    if !report.failures.is_empty() {
        eprintln!("outside the band: {}", report.failures.join(", "));
        std::process::exit(1);
    }
}

// --- Town ---------------------------------------------------------------------------

fn town_day(report: &mut Report) {
    let mut sim = Simulation::new();
    scenarios::build_town(&mut sim);
    let (mut trucks, mut failed, mut arrivals) = (0, 0, 0);
    let mut exports: HashSet<i32> = HashSet::new();
    let mut commutes: HashMap<CitizenId, f64> = HashMap::new();
    let mut everyone_at_work = -1.0;
    let mut violations = 0;
    let mut worst_gap = f64::INFINITY;
    let (mut peak, mut most_stuck) = (0, 0);
    for tick in 0..DAY_TICKS {
        sim.step();
        for event in sim.take_view_events() {
            match event {
                SimEvent::VehicleArrived(id) => {
                    let v = sim.vehicles.vehicle_any(id).unwrap();
                    arrivals += 1;
                    if v.kind == Kind::Truck {
                        trucks += 1;
                        if v.destination_lot_id.is_none() {
                            exports
                                .insert(sim.roads.lane_any(v.lane.unwrap()).unwrap().to_node_id.0);
                        }
                    }
                }
                SimEvent::VehicleFailed(_) => failed += 1,
                SimEvent::TripEnded { citizen, completed } => {
                    let c = &sim.citizens.citizens[&citizen];
                    // The event is read after the arrival: a completed trip to work
                    // leaves the citizen at work.
                    if completed && c.state == State::AtWork {
                        commutes
                            .entry(citizen)
                            .or_insert((c.trip_end_time - c.trip_start_time) / c.trip_ideal_time);
                    }
                }
                _ => {}
            }
        }
        violations += connector_conflict_violations(&sim);
        worst_gap = worst_gap.min(min_queue_gap(&sim));
        peak = peak.max(sim.vehicles.vehicles.len());
        let drivers = sim.citizens.citizens.values().filter(|c| c.has_car).count();
        let at_work = sim
            .citizens
            .citizens
            .values()
            .filter(|c| c.has_car && c.state == State::AtWork)
            .count();
        if everyone_at_work < 0.0 && drivers > 0 && at_work as f64 >= drivers as f64 * 0.95 {
            everyone_at_work = sim.clock.time_of_day_hours();
        }
        if tick % Simulation::TICK_RATE == 0 {
            most_stuck = most_stuck.max(sim.vehicles.stuck_count(30.0));
        }
    }
    let mut ratios: Vec<f64> = commutes.values().copied().collect();
    ratios.sort_by(f64::total_cmp);
    let median = ratios.get(ratios.len() / 2).copied().unwrap_or(0.0);
    let now = sim.sim_time();
    let flow = sim.flow.flow_percent(now);
    let shares = sim.flow.mode_shares(now);
    let carless = sim
        .citizens
        .citizens
        .values()
        .filter(|c| !c.has_car)
        .count();
    let citizens = sim.citizens.citizens.len();

    println!("## Town, one day from 06:00\n");
    println!("`Scenarios::build_town`, {DAY_TICKS} ticks, the safety checks on every tick.\n");
    println!("| Metric | Rust | Godot | Band | |");
    println!("|---|---|---|---|---|");
    report.row("Citizens", citizens as f64, 176.0, 0, Band::Exact);
    report.row(
        "Citizens without a car",
        carless as f64,
        40.0,
        0,
        Band::Exact,
    );
    report.row("Flow %", flow, 60.3, 1, Band::Points(5.0));
    report.row(
        "Failed vehicle trips",
        f64::from(failed),
        2.0,
        0,
        Band::AtMost(3.0),
    );
    report.row(
        "Vehicle arrivals",
        f64::from(arrivals),
        573.0,
        0,
        Band::Percent(10.0),
    );
    report.row(
        "Truck trips",
        f64::from(trucks),
        261.0,
        0,
        Band::Percent(15.0),
    );
    report.row(
        "Highways used for exports",
        exports.len() as f64,
        2.0,
        0,
        Band::Exact,
    );
    report.row("Peak vehicles", peak as f64, 165.0, 0, Band::Percent(15.0));
    report.row(
        "95% of drivers at work (hour)",
        everyone_at_work,
        12.37,
        2,
        Band::Points(1.0),
    );
    report.row(
        "Commuters who reached work",
        commutes.len() as f64,
        175.0,
        0,
        Band::Points(5.0),
    );
    report.row(
        "Median commute (× ideal)",
        median,
        1.77,
        2,
        Band::Points(0.3),
    );
    for (mode, godot) in [(Mode::Car, 69.6), (Mode::Bus, 5.4), (Mode::Walk, 25.0)] {
        let share = shares.get(&mode).copied().unwrap_or(0.0) * 100.0;
        report.row(
            &format!("Mode share: {} (%)", mode.name()),
            share,
            godot,
            1,
            Band::Points(5.0),
        );
    }
    report.row(
        "Conflict-zone violations",
        violations as f64,
        0.0,
        0,
        Band::Exact,
    );
    report.row(
        "Smallest queue gap (px)",
        worst_gap,
        4.0,
        2,
        Band::AtLeast(MIN_GAP - 0.01),
    );
    report.row(
        "Most vehicles stuck 30 s+",
        most_stuck as f64,
        0.0,
        0,
        Band::Exact,
    );
    println!();
}

// --- Bottleneck ---------------------------------------------------------------------

struct Morning {
    time: f64,
    driving: f64,
    took_bypass: usize,
}

fn morning(seed: u64, bypass: bool) -> Morning {
    let mut sim = Simulation::new();
    sim.citizens.set_seed(seed);
    let info = scenarios::build_bottleneck(&mut sim, BOTTLENECK_ORIGIN);
    let before: Vec<SegmentId> = sim.roads.segments.keys().copied().collect();
    if bypass {
        scenarios::build_bypass(&mut sim, &info);
    }
    let y = info.junction.y;
    let watched: Vec<SegmentId> = sim
        .roads
        .segments
        .iter()
        .filter(|(id, segment)| {
            let start = sim.roads.nodes[&segment.start_node_id].position.y;
            let end = sim.roads.nodes[&segment.end_node_id].position.y;
            !before.contains(id) && !((start - y).abs() < 1e-4 && (end - y).abs() < 1e-4)
        })
        .map(|(&id, _)| id)
        .collect();
    let mut driving = 0.0;
    let mut reached: HashSet<CitizenId> = HashSet::new();
    let mut seen: HashSet<i32> = HashSet::new();
    let mut time = f64::INFINITY;
    for _ in 0..Simulation::TICK_RATE * 300 {
        sim.step();
        for event in sim.take_view_events() {
            if let SimEvent::VehicleArrived(id) = event {
                let v = sim.vehicles.vehicle_any(id).unwrap();
                if let Some(c) = sim.citizens.citizens.get(&CitizenId(v.owner_id))
                    && v.destination_lot_id == c.work_lot_id
                {
                    driving += v.trip_end_time - v.trip_start_time;
                }
            }
        }
        for v in sim.vehicles.vehicles.values() {
            let commuter = sim.citizens.citizens.get(&CitizenId(v.owner_id));
            let lane = sim.roads.lane_any(v.lane.unwrap()).unwrap();
            if commuter.is_some_and(|c| c.state == State::ToWork)
                && lane.segment_id.is_some_and(|s| watched.contains(&s))
            {
                seen.insert(v.owner_id);
            }
        }
        for c in sim.citizens.citizens.values() {
            if c.state == State::AtWork {
                reached.insert(c.id);
            }
        }
        if reached.len() == sim.citizens.citizens.len() {
            time = sim.sim_time();
            break;
        }
    }
    Morning {
        time,
        driving,
        took_bypass: seen.len(),
    }
}

fn bottleneck_mornings(report: &mut Report) {
    println!("## Bottleneck mornings\n");
    println!(
        "`test_bottleneck.gd`'s `_morning_for_seed` (origin (30, 30)), run until every commuter has reached work (max 300 s). Godot's rows: `(seed, bypass, time, driving, took bypass)`.\n"
    );
    println!("| Metric | Rust | Godot | Band | |");
    println!("|---|---|---|---|---|");
    let godot = [
        (1, false, 80.4, 1169.0, 0),
        (1, true, 82.1, 1187.0, 2),
        (2, false, 88.7, 814.0, 0),
        (2, true, 87.9, 883.0, 2),
        (3, false, 89.8, 977.0, 0),
        (3, true, 89.8, 979.0, 1),
    ];
    for (seed, bypass, time, driving, took) in godot {
        let m = morning(seed, bypass);
        let label = format!(
            "seed {seed}, {}",
            if bypass { "bypass" } else { "no bypass" }
        );
        report.row(
            &format!("{label}: everyone at work (s)"),
            m.time,
            time,
            1,
            Band::Percent(15.0),
        );
        report.row(
            &format!("{label}: driving (s)"),
            m.driving,
            driving,
            0,
            Band::Percent(15.0),
        );
        report.row(
            &format!("{label}: took bypass"),
            m.took_bypass as f64,
            f64::from(took),
            0,
            Band::Points(2.0),
        );
    }
    println!();
}

fn bottleneck_flow(report: &mut Report) {
    println!("## Bottleneck, Flow after the morning\n");
    println!(
        "Default seeds, {MORNING_TICKS} ticks (120 s, 10 game hours) from the start of the scenario.\n"
    );
    println!("| Metric | Rust | Godot | Band | |");
    println!("|---|---|---|---|---|");
    let godot = [
        ("plain", 50.2, 39.0),
        ("bypass", 50.5, 39.0),
        ("bus line (curbside)", 52.8, 44.0),
        ("bus lanes", 54.3, 49.0),
    ];
    for (setup, flow_godot, arrivals_godot) in godot {
        let mut sim = Simulation::new();
        let info = scenarios::build_bottleneck(&mut sim, BOTTLENECK_ORIGIN);
        match setup {
            "bypass" => scenarios::build_bypass(&mut sim, &info),
            "bus line (curbside)" => {
                scenarios::add_bottleneck_bus_line(&mut sim, BOTTLENECK_ORIGIN);
            }
            "bus lanes" => {
                scenarios::add_bottleneck_bus_lanes(&mut sim, BOTTLENECK_ORIGIN);
            }
            _ => {}
        }
        let mut arrivals = 0;
        for _ in 0..MORNING_TICKS {
            sim.step();
            arrivals += sim
                .take_view_events()
                .iter()
                .filter(|e| matches!(e, SimEvent::VehicleArrived(_)))
                .count();
        }
        let now = sim.sim_time();
        let modes: Vec<String> = sim
            .flow
            .mode_shares(now)
            .iter()
            .map(|(mode, share)| format!("{} {:.0}%", mode.name(), share * 100.0))
            .collect();
        report.row(
            &format!("{setup}: Flow %"),
            sim.flow.flow_percent(now),
            flow_godot,
            1,
            Band::Points(5.0),
        );
        report.row(
            &format!("{setup}: vehicle arrivals"),
            arrivals as f64,
            arrivals_godot,
            0,
            Band::Percent(10.0),
        );
        println!("| {setup}: mode shares | {} | | | |", modes.join(", "));
    }
    println!();
}

// --- Stress grids -------------------------------------------------------------------

fn stress_grids(report: &mut Report) {
    println!("## Stress grids\n");
    println!(
        "Random trips (`StressTraffic`, seed 0) on a 5 × 5 grid of roads 3 cells apart, safety checks on every tick.\n"
    );
    println!("| Metric | Rust | Godot | Band | |");
    println!("|---|---|---|---|---|");
    // (label, vehicles at end, arrivals) of Godot.
    for (control, label, vehicles, arrivals) in [
        (JunctionControl::None, "local, none, 250", 250.0, 507.0),
        (
            JunctionControl::TrafficLight,
            "local, traffic_light, 250",
            249.0,
            622.0,
        ),
    ] {
        let mut sim = Simulation::new();
        for i in 0..5 {
            let at = (1 + i * 3) as f32;
            add_road(
                &mut sim,
                Vec2::new(1.0, at),
                Vec2::new(13.0, at),
                "local_2lane",
            );
            add_road(
                &mut sim,
                Vec2::new(at, 1.0),
                Vec2::new(at, 13.0),
                "local_2lane",
            );
        }
        let nodes: Vec<_> = sim
            .roads
            .nodes
            .values()
            .filter(|n| n.degree() >= 3)
            .map(|n| n.id)
            .collect();
        for node in nodes {
            sim.call(|sim, out| {
                sim.roads.set_node_control(node, control, out);
            });
        }
        stress_row(report, sim, label, 250, 120, vehicles, arrivals);
    }
    let mixes: [(&[&str], f64, f64); 3] = [
        (&["avenue_4lane", "local_2lane"], 200.0, 491.0),
        (
            &["local_2lane", "one_way_2lane", "one_way_1lane"],
            199.0,
            283.0,
        ),
        (
            &["avenue_4lane", "one_way_2lane", "one_way_1lane"],
            199.0,
            419.0,
        ),
    ];
    for (mix, vehicles, arrivals) in mixes {
        let mut sim = Simulation::new();
        let odd_h = mix[1];
        let odd_v = if mix.len() > 2 { mix[2] } else { mix[1] };
        for i in 0..5 {
            let at = (1 + i * 3) as f32;
            let horizontal = if i % 2 == 0 { mix[0] } else { odd_h };
            let vertical = if i % 2 == 0 { mix[0] } else { odd_v };
            if i % 4 == 1 {
                add_road(
                    &mut sim,
                    Vec2::new(13.0, at),
                    Vec2::new(1.0, at),
                    horizontal,
                );
                add_road(&mut sim, Vec2::new(at, 13.0), Vec2::new(at, 1.0), vertical);
            } else {
                add_road(
                    &mut sim,
                    Vec2::new(1.0, at),
                    Vec2::new(13.0, at),
                    horizontal,
                );
                add_road(&mut sim, Vec2::new(at, 1.0), Vec2::new(at, 13.0), vertical);
            }
        }
        let label = format!("{}, 200", mix.join(" / "));
        stress_row(report, sim, &label, 200, 90, vehicles, arrivals);
    }
}

fn stress_row(
    report: &mut Report,
    mut sim: Simulation,
    label: &str,
    target: usize,
    seconds: u32,
    vehicles_godot: f64,
    arrivals_godot: f64,
) {
    sim.stress.target_count = target;
    let mut arrived = 0;
    let mut violations = 0;
    let mut worst_gap = f64::INFINITY;
    for _ in 0..Simulation::TICK_RATE * seconds {
        sim.step();
        arrived += sim
            .take_view_events()
            .iter()
            .filter(|e| matches!(e, SimEvent::VehicleArrived(_)))
            .count();
        violations += connector_conflict_violations(&sim);
        worst_gap = worst_gap.min(min_queue_gap(&sim));
    }
    report.row(
        &format!("{label}: vehicles at end"),
        sim.vehicles.vehicles.len() as f64,
        vehicles_godot,
        0,
        Band::Percent(15.0),
    );
    report.row(
        &format!("{label}: arrivals"),
        arrived as f64,
        arrivals_godot,
        0,
        Band::Percent(15.0),
    );
    report.row(
        &format!("{label}: stuck 30 s+"),
        sim.vehicles.stuck_count(30.0) as f64,
        0.0,
        0,
        Band::Exact,
    );
    report.row(
        &format!("{label}: smallest gap"),
        worst_gap,
        4.0,
        2,
        Band::AtLeast(MIN_GAP - 0.01),
    );
    report.row(
        &format!("{label}: violations"),
        violations as f64,
        0.0,
        0,
        Band::Exact,
    );
}
