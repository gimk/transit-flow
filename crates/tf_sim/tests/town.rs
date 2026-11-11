//! Port of `tests/unit/test_town.gd`: the Town scenario end to end, a whole day of a
//! complete small city, checking that the daily rhythm completes, traffic stays safe and
//! nothing deadlocks. And the Godot parity of that day (`migration/probes/town_trace.gd`
//! → `migration/golden/town_trace.json`): every vehicle and pedestrian arrival or failure,
//! every rider, state change, trip start and trip end on the same tick as in Godot, Flow
//! every game hour, and the end state; with the safety checks on every tick.

mod common;

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use common::golden;
use serde::Deserialize;
use tf_sim::Simulation;
use tf_sim::city::LotType;
use tf_sim::city::citizen::State;
use tf_sim::clock::SimClock;
use tf_sim::debug::checks::{connector_conflict_violations, crosswalk_violations, min_queue_gap};
use tf_sim::debug::scenarios::{self, TownInfo};
use tf_sim::events::SimEvent;
use tf_sim::ids::CitizenId;
use tf_sim::roads::JunctionControl;
use tf_sim::traffic::vehicle::Kind;
use tf_sim::traffic::vehicle_system::MIN_GAP;

const DAY_TICKS: u32 = Simulation::TICK_RATE * SimClock::MINUTES_PER_DAY as u32
    / SimClock::GAME_MINUTES_PER_SIM_SECOND as u32;

fn built() -> (Simulation, TownInfo) {
    let mut sim = Simulation::new();
    let info = scenarios::build_town(&mut sim);
    (sim, info)
}

fn count_lots(sim: &Simulation, lot_type: LotType) -> usize {
    sim.lots.lots_of_type(lot_type).len()
}

#[test]
fn town_is_built_and_developed() {
    let (mut sim, info) = built();
    assert_eq!(count_lots(&sim, LotType::Residential), 88);
    assert_eq!(count_lots(&sim, LotType::Commercial), 38);
    assert_eq!(count_lots(&sim, LotType::Industrial), 36);
    for &cell in sim.zones.zones.keys() {
        assert!(
            sim.zones.is_developed(cell),
            "zoned cell {cell} developed at once"
        );
    }
    assert_eq!(
        sim.externals.active_points(&sim.roads).len(),
        2,
        "both highways connected"
    );
    for &point in &info.lights {
        let node = sim.roads.find_node_at(point, 0).unwrap();
        assert!(sim.roads.nodes[&node].has_lights());
    }
    sim.step();
    assert_eq!(sim.citizens.citizens.len(), 176);
    assert!(
        sim.citizens.citizens.len() as i32 <= sim.citizens.job_count(),
        "a job for everyone"
    );
}

/// The Town shows off every feature (keep this list growing with each phase).
#[test]
fn town_uses_every_feature() {
    let (mut sim, info) = built();
    let road_types: HashSet<&str> = sim
        .roads
        .segments
        .values()
        .map(|s| s.road_type.id.as_str())
        .collect();
    for type_id in [
        "local_2lane",
        "avenue_4lane",
        "avenue_4lane_bus",
        "one_way_2lane",
    ] {
        assert!(road_types.contains(type_id), "a road of type {type_id}");
    }
    assert!(
        sim.roads
            .segments
            .values()
            .any(|s| s.centerline().point_count() > 2),
        "curved roads"
    );
    let controls: Vec<JunctionControl> = sim.roads.nodes.values().map(|n| n.control).collect();
    assert!(
        controls.contains(&JunctionControl::TrafficLight),
        "traffic lights"
    );
    assert!(
        controls.contains(&JunctionControl::TrafficLightProtectedLeft),
        "protected left turns"
    );
    assert_eq!(info.bus_lines.len(), 4);
    assert!(
        !sim.crosswalks.mid_block.is_empty(),
        "crosswalks at bus stops"
    );
    let stops: Vec<_> = sim.transit.stops.values().collect();
    assert!(stops.iter().any(|s| s.bay), "bus bays");
    assert!(stops.iter().any(|s| !s.bay), "curbside stops");
    assert!(
        stops
            .iter()
            .any(|s| sim.roads.lanes[&s.lane_id.unwrap()].bus_only),
        "stops in bus lanes"
    );
    for line in sim.transit.lines.values() {
        assert!(
            line.loop_time() < f64::INFINITY,
            "line {} runs round",
            line.id.0
        );
    }
    sim.step();
    assert!(
        sim.citizens.citizens.values().any(|c| !c.has_car),
        "people without a car"
    );
}

#[test]
fn town_survives_a_save() {
    let (sim, info) = built();
    let loaded = Simulation::from_dict(&serde_json::from_str(&sim.to_dict().to_string()).unwrap())
        .expect("the save loads");
    assert_eq!(loaded.lots.lots.len(), sim.lots.lots.len());
    assert_eq!(loaded.zones.zones.len(), sim.zones.zones.len());
    assert_eq!(loaded.externals.active_points(&loaded.roads).len(), 2);
    let lights = loaded
        .roads
        .nodes
        .values()
        .filter(|n| n.has_lights())
        .count();
    assert_eq!(lights, info.lights.len());
}

type Event = (usize, String, i64, i64, i64, i64, i64);
/// (tick, flow in millionths of a percent, trips in the window, (mode, share in millionths)).
type FlowSample = (usize, Option<i64>, usize, Vec<(i32, i64)>);

#[derive(Deserialize)]
struct Stats {
    commutes: usize,
    trucks: usize,
    failed: usize,
    exports: usize,
    arrivals: usize,
    peak: usize,
    everyone_at_work_ms: i64,
    median_ratio: i64,
    violations: usize,
    worst_gap: i64,
    most_stuck: usize,
}

#[derive(Deserialize)]
struct Golden {
    events: Vec<Event>,
    flow: Vec<FlowSample>,
    /// (tick, vehicles, pedestrians, waiting departures, riders, stuck 30 s+).
    series: Vec<(usize, usize, usize, usize, usize, usize)>,
    setup: [usize; 4],
    stats: Stats,
    citizens: Vec<[i64; 13]>,
    parking: Vec<(i32, i32)>,
    counts: [usize; 8],
    vehicles: Vec<(i32, i32, f64, f64)>,
}

fn ms(x: f64) -> i64 {
    (x * 1000.0).round() as i64
}

/// What one run of the day measured (plain data, so the tests can share a single run).
struct Day {
    events: Vec<Event>,
    flow: Vec<FlowSample>,
    series: Vec<(usize, usize, usize, usize, usize, usize)>,
    setup: [usize; 4],
    stats: Stats,
    /// The Town's own measurements the GDScript test asserts on.
    everyone_at_work: f64,
    median_ratio: f64,
    flow_percent: f64,
    modes: String,
    crosswalk_violations: usize,
    out_late: usize,
    citizens: Vec<[i64; 13]>,
    parking: Vec<(i32, i32)>,
    counts: [usize; 8],
    vehicles: Vec<(i32, i32, f64, f64)>,
    citizens_len: usize,
}

fn day() -> &'static Day {
    static DAY: OnceLock<Day> = OnceLock::new();
    DAY.get_or_init(run_day)
}

fn run_day() -> Day {
    let (mut sim, _info) = built();
    let mut events: Vec<Event> = Vec::new();
    let mut state: HashMap<CitizenId, State> = HashMap::new();
    let mut start: HashMap<CitizenId, f64> = HashMap::new();
    let mut commutes: HashMap<CitizenId, f64> = HashMap::new();
    let mut exports: HashSet<i32> = HashSet::new();
    let (mut trucks, mut failed, mut arrivals) = (0, 0, 0);
    let mut everyone_at_work = -1.0;
    let (mut violations, mut crosswalks) = (0, 0);
    let mut worst_gap = f64::INFINITY;
    let (mut peak, mut most_stuck) = (0, 0);
    let (mut setup, mut series, mut flow_samples) = ([0; 4], Vec::new(), Vec::new());
    for tick in 0..DAY_TICKS as usize {
        sim.step();
        // The events of the step, as the probe's signal handlers record them.
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
                    events.push((
                        tick,
                        "a".into(),
                        id.0.into(),
                        v.owner_id.into(),
                        i64::from(v.kind as i32),
                        v.destination_lot_id.map_or(-1, |l| l.0.into()),
                        0,
                    ));
                }
                SimEvent::VehicleFailed(id) => {
                    let v = sim.vehicles.vehicle_any(id).unwrap();
                    failed += 1;
                    let kind = if v.gave_up { "g" } else { "f" };
                    events.push((tick, kind.into(), id.0.into(), v.owner_id.into(), 0, 0, 0));
                }
                SimEvent::PedestrianArrived(id) => {
                    let owner = sim.pedestrians.pedestrian_any(id).unwrap().owner_id;
                    events.push((tick, "p".into(), id.0.into(), owner.into(), 0, 0, 0));
                }
                SimEvent::RiderArrived { citizen, door } => events.push((
                    tick,
                    "r".into(),
                    citizen.0.into(),
                    (f64::from(door.x) * 10.0).round() as i64,
                    (f64::from(door.y) * 10.0).round() as i64,
                    0,
                    0,
                )),
                SimEvent::RiderStranded(citizen) => {
                    events.push((tick, "x".into(), citizen.0.into(), 0, 0, 0, 0));
                }
                SimEvent::TripEnded { citizen, completed } => {
                    let c = &sim.citizens.citizens[&citizen];
                    // A completed trip to work leaves the citizen at work, and nothing
                    // moves them on within the tick (Godot reads TO_WORK at the emit).
                    if completed && c.state == State::AtWork {
                        commutes
                            .entry(citizen)
                            .or_insert((c.trip_end_time - c.trip_start_time) / c.trip_ideal_time);
                    }
                    events.push((
                        tick,
                        "t".into(),
                        citizen.0.into(),
                        i64::from(completed),
                        i64::from(c.trip_mode as i32),
                        ms(c.trip_ideal_time),
                        ms(c.trip_end_time - c.trip_start_time),
                    ));
                }
                _ => {}
            }
        }
        // Records a citizen whenever their state changes or a new trip started.
        for c in sim.citizens.citizens.values() {
            if c.trip_start_time != start.get(&c.id).copied().unwrap_or(0.0) {
                start.insert(c.id, c.trip_start_time);
                let reference = if c.trip_reference_time.is_infinite() {
                    -1
                } else {
                    ms(c.trip_reference_time)
                };
                events.push((
                    tick,
                    "b".into(),
                    c.id.0.into(),
                    i64::from(c.trip_mode as i32),
                    c.trip_lot_id.map_or(-1, |l| l.0.into()),
                    reference,
                    0,
                ));
            }
            if state.get(&c.id) != Some(&c.state) {
                state.insert(c.id, c.state);
                events.push((
                    tick,
                    "c".into(),
                    c.id.0.into(),
                    i64::from(c.state as i32),
                    c.trip_lot_id.map_or(-1, |l| l.0.into()),
                    0,
                    0,
                ));
            }
        }
        // The safety checks, on every tick.
        violations += connector_conflict_violations(&sim);
        crosswalks += crosswalk_violations(&sim);
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
        if tick % Simulation::TICK_RATE as usize == 0 {
            most_stuck = most_stuck.max(sim.vehicles.stuck_count(30.0));
        }
        if tick == 0 {
            setup = [
                sim.citizens.citizens.len(),
                sim.citizens.job_count() as usize,
                sim.lots.lots.len(),
                sim.vehicles.vehicles.len(),
            ];
        }
        if tick % 120 == 119 {
            series.push((
                tick,
                sim.vehicles.vehicles.len(),
                sim.pedestrians.pedestrians.len(),
                sim.vehicles.waiting_departures().len(),
                sim.transit.rider_count(),
                sim.vehicles.stuck_count(30.0),
            ));
        }
        if tick % 240 == 239 {
            let now = sim.sim_time();
            let flow = sim.flow.flow_percent(now);
            let mut shares: Vec<(i32, i64)> = sim
                .flow
                .mode_shares(now)
                .into_iter()
                .map(|(mode, share)| (mode as i32, (share * 1e6).round() as i64))
                .collect();
            shares.sort();
            flow_samples.push((
                tick,
                (flow >= 0.0).then(|| (flow * 1e6).round() as i64),
                sim.flow.trip_count(now),
                shares,
            ));
        }
    }

    let mut ratios: Vec<f64> = commutes.values().copied().collect();
    ratios.sort_by(f64::total_cmp);
    let median_ratio = ratios[ratios.len() / 2];
    let now = sim.sim_time();
    let flow_percent = sim.flow.flow_percent(now);
    let modes = format!("{:?}", sim.flow.mode_shares(now));
    let out_late = sim
        .citizens
        .citizens
        .values()
        .filter(|c| {
            c.state != State::AtHome && !(c.state == State::ToHome && c.pedestrian_id.is_some())
        })
        .count();
    let citizens = sim
        .citizens
        .citizens
        .values()
        .map(|c| {
            [
                c.id.0.into(),
                (c.state as i32).into(),
                c.work_lot_id.map_or(-1, |l| l.0).into(),
                c.car_lot_id.0.into(),
                i64::from(c.has_car),
                c.last_commute_day.into(),
                c.shopping_decided_day.into(),
                i64::from(c.going_shopping),
                ms(c.shop_until),
                ms(c.retry_at),
                (c.leave_home_hour * 1e6).round() as i64,
                (c.leave_work_hour * 1e6).round() as i64,
                (c.car_preference * 1e6).round() as i64,
            ]
        })
        .collect();
    let parking = sim
        .lots
        .lots
        .keys()
        .filter(|&&l| sim.citizens.parking.used(l) > 0)
        .map(|&l| (l.0, sim.citizens.parking.used(l)))
        .collect();
    let counts = [
        sim.citizens.citizens.len(),
        sim.citizens.job_count() as usize,
        sim.citizens.employed_count(),
        sim.lots.lots.len(),
        sim.vehicles.vehicles.len(),
        sim.pedestrians.pedestrians.len(),
        sim.vehicles.waiting_departures().len(),
        sim.transit.rider_count(),
    ];
    let vehicles = sim
        .vehicles
        .vehicles
        .values()
        .map(|v| (v.id.0, v.lane.unwrap().0, v.distance, v.speed))
        .collect();
    Day {
        events,
        flow: flow_samples,
        series,
        setup,
        stats: Stats {
            commutes: commutes.len(),
            trucks,
            failed,
            exports: exports.len(),
            arrivals,
            peak,
            everyone_at_work_ms: (everyone_at_work * 1000.0).round() as i64,
            median_ratio: (median_ratio * 1e6).round() as i64,
            violations,
            worst_gap: (worst_gap * 1e6).round() as i64,
            most_stuck,
        },
        everyone_at_work,
        median_ratio,
        flow_percent,
        modes,
        crosswalk_violations: crosswalks,
        out_late,
        citizens,
        parking,
        counts,
        vehicles,
        citizens_len: sim.citizens.citizens.len(),
    }
}

/// One full day from 06:00: the morning rush jams but everyone gets to work, trucks use
/// both highways, everyone is home by night, and the safety rules hold on every tick
/// (people on crosswalks too).
#[test]
fn town_lives_through_a_whole_day() {
    let day = day();
    let stats = &day.stats;
    println!("town: modes {}", day.modes);
    println!(
        "town: peak {} vehicles, everyone at work at {:.1} h, median commute {:.1}x ideal, {} trucks, {} failed, flow {:.0}%",
        stats.peak,
        day.everyone_at_work,
        day.median_ratio,
        stats.trucks,
        stats.failed,
        day.flow_percent
    );
    assert_eq!(
        stats.violations, 0,
        "never two vehicles where conflicting paths meet"
    );
    assert_eq!(
        day.crosswalk_violations, 0,
        "never a car over a crosswalk with people on it"
    );
    assert!(
        stats.worst_gap as f64 / 1e6 >= MIN_GAP - 0.01,
        "no overlaps in any queue"
    );
    assert_eq!(
        stats.most_stuck, 0,
        "nobody ever stopped for 30 s (no deadlock)"
    );
    assert!(
        stats.commutes + 5 >= day.citizens_len,
        "(almost) every commuter reached work (a few give up in the jam)"
    );
    assert!(
        (9.0..=16.0).contains(&day.everyone_at_work),
        "the morning rush (95% of drivers at work) is over before the evening one"
    );
    assert!(
        day.median_ratio > 1.2,
        "the morning rush is congested (that's the point of the scenario)"
    );
    assert_eq!(
        day.out_late, 0,
        "everyone home by 06:00 (or walking home: on foot, crossing town takes hours at this clock)"
    );
    assert!(stats.trucks > 150, "freight runs all day");
    assert_eq!(stats.exports, 2, "exports leave by both highways");
    assert!(stats.failed < 10, "almost every trip succeeds");
    assert!(
        day.flow_percent > 50.0,
        "driveways (Phase 6) add friction: Flow was 72% before them, ~58% after"
    );
}

#[test]
fn the_town_day_matches_godot() {
    let golden: Golden = golden::read("town_trace");
    let day = day();

    let first_difference = day
        .events
        .iter()
        .zip(&golden.events)
        .position(|(a, b)| a != b);
    assert!(
        first_difference.is_none() && day.events.len() == golden.events.len(),
        "events differ from Godot at #{first_difference:?} ({} here, {} in Godot): {:?} vs {:?}",
        day.events.len(),
        golden.events.len(),
        first_difference.map(|i| &day.events[i]),
        first_difference.map(|i| &golden.events[i]),
    );
    assert_eq!(
        day.setup, golden.setup,
        "citizens, jobs, lots, vehicles after one tick"
    );
    assert_eq!(
        day.series, golden.series,
        "vehicles, pedestrians, departures, riders, stuck"
    );
    assert_eq!(day.flow.len(), golden.flow.len());
    for (here, there) in day.flow.iter().zip(&golden.flow) {
        assert_eq!(here.0, there.0);
        assert_eq!(here.2, there.2, "trips in the window at tick {}", here.0);
        assert_eq!(here.3, there.3, "mode shares at tick {}", here.0);
        match (here.1, there.1) {
            (Some(a), Some(b)) => {
                assert!((a - b).abs() <= 1, "flow at tick {}: {a} vs {b}", here.0)
            }
            (a, b) => assert_eq!(a, b, "flow at tick {}", here.0),
        }
    }
    let (a, b) = (&day.stats, &golden.stats);
    assert_eq!(
        (
            a.commutes, a.trucks, a.failed, a.exports, a.arrivals, a.peak
        ),
        (
            b.commutes, b.trucks, b.failed, b.exports, b.arrivals, b.peak
        ),
        "commuters, trucks, failed trips, exports, arrivals, peak vehicles"
    );
    assert_eq!(
        a.everyone_at_work_ms, b.everyone_at_work_ms,
        "95% of drivers at work"
    );
    assert!(
        (a.median_ratio - b.median_ratio).abs() <= 1,
        "median commute"
    );
    assert_eq!((a.violations, a.most_stuck), (b.violations, b.most_stuck));
    assert_eq!(a.worst_gap, b.worst_gap, "smallest queue gap");

    assert_eq!(day.citizens, golden.citizens, "citizens at the end");
    assert_eq!(day.parking, golden.parking, "parking");
    assert_eq!(
        day.counts, golden.counts,
        "citizens, jobs, employed, lots, vehicles, pedestrians, waiting, riders"
    );
    assert_eq!(day.vehicles.len(), golden.vehicles.len());
    for (v, g) in day.vehicles.iter().zip(&golden.vehicles) {
        assert_eq!((v.0, v.1), (g.0, g.1), "vehicle, lane");
        let round3 = |x: f64| (x * 1000.0).round() / 1000.0;
        assert!(
            (round3(v.2) - g.2).abs() <= 0.002 && (round3(v.3) - g.3).abs() <= 0.002,
            "vehicle {} at {:?}, Godot {:?}",
            g.0,
            (v.2, v.3),
            (g.2, g.3)
        );
    }
}
