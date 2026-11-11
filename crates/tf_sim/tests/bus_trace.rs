//! M6.2 review: buses traced against Godot (`migration/probes/bus_trace.gd`,
//! `migration/golden/bus_trace.json`). One street with bays and curbside stops under 40
//! cars, and a grid with two lines under 80 cars, a street added, a road retyped, a stop
//! removed and a block of a line cut. Every vehicle arrival and failure, every bus
//! stopping, leaving or pulling into a bay, and every rider getting off or stranded must
//! happen on the same tick as in Godot (and the doors at the same place); and the buses,
//! stops, line timings and vehicles must end the same.

mod common;

use common::golden;
use common::roads::{G, road_type};
use common::traffic::road as add_road;
use common::transit::{add_line, add_stop};
use glam::Vec2;
use serde::Deserialize;
use std::collections::HashMap;
use tf_sim::Simulation;
use tf_sim::events::SimEvent;
use tf_sim::ids::{CitizenId, LineId, StopId};
use tf_sim::transit::transit_system::RideLeg;

type Event = (usize, String, i32, i32, i32);

#[derive(Deserialize)]
struct Trace {
    events: Vec<Event>,
    end: Vec<(i32, i32, f64, f64)>,
    waiting: usize,
    buses: Vec<(i32, i32, usize, i32, usize, f64)>,
    lines: Vec<(i32, i32, Vec<f64>)>,
    stops: Vec<(i32, i32, usize, i32, bool)>,
}

#[derive(Deserialize)]
struct Plan {
    from: [f32; 2],
    to: [f32; 2],
    max_walk: f64,
    weight: f64,
    cost: Option<f64>,
    legs: Vec<(i32, i32, i32, i32)>,
}

#[derive(Deserialize)]
struct Plans {
    plans: Vec<Plan>,
}

#[derive(Deserialize)]
struct Golden {
    bays: Trace,
    grid: Trace,
    plans: Plans,
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn local() -> &'static tf_sim::roads::RoadType {
    road_type("local_2lane")
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// 1e9 stands for an unreachable stop, as in the probe.
fn time(x: f64) -> f64 {
    if x.is_infinite() { 1e9 } else { round3(x) }
}

struct Recorder {
    events: Vec<Event>,
    state: HashMap<i32, i32>,
}

impl Recorder {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            state: HashMap::new(),
        }
    }

    /// Steps once and records what the vehicles and riders reported, then each bus whose
    /// state changed (the probe's `_record` and `_poll`).
    fn step(&mut self, sim: &mut Simulation, tick: usize) {
        sim.step();
        for event in sim.take_view_events() {
            match event {
                SimEvent::VehicleArrived(id) => self.events.push((tick, "a".into(), id.0, 0, 0)),
                SimEvent::VehicleFailed(id) => {
                    let gave_up = sim.vehicles.vehicle_any(id).unwrap().gave_up;
                    let kind = if gave_up { "g" } else { "f" };
                    self.events.push((tick, kind.into(), id.0, 0, 0));
                }
                SimEvent::RiderArrived { citizen, door } => self.events.push((
                    tick,
                    "r".into(),
                    citizen.0,
                    (f64::from(door.x) * 10.0).round() as i32,
                    (f64::from(door.y) * 10.0).round() as i32,
                )),
                SimEvent::RiderStranded(citizen) => {
                    self.events.push((tick, "x".into(), citizen.0, 0, 0));
                }
                _ => {}
            }
        }
        for bus in sim.transit.buses.values() {
            let vehicle = &sim.vehicles.vehicles[&bus.vehicle];
            let state = bus.stop_index as i32 * 4
                + if vehicle.dwelling { 2 } else { 0 }
                + i32::from(vehicle.in_bay);
            if self.state.get(&bus.vehicle.0).copied().unwrap_or(-1) != state {
                self.state.insert(bus.vehicle.0, state);
                self.events
                    .push((tick, "s".into(), bus.vehicle.0, state, 0));
            }
        }
    }
}

fn wait(sim: &mut Simulation, citizen: i32, line: LineId, from: StopId, to: StopId) {
    sim.transit.wait_for_bus(CitizenId(citizen), line, from, to);
}

fn compare(name: &str, sim: &Simulation, events: &[Event], golden: &Trace) {
    let first_difference = events.iter().zip(&golden.events).position(|(a, b)| a != b);
    assert!(
        first_difference.is_none() && events.len() == golden.events.len(),
        "{name}: events differ from Godot at #{first_difference:?} ({} here, {} in Godot): {:?} vs {:?}",
        events.len(),
        golden.events.len(),
        first_difference.map(|i| &events[i]),
        first_difference.map(|i| &golden.events[i]),
    );

    let end: Vec<(i32, i32, f64, f64)> = sim
        .vehicles
        .vehicles
        .values()
        .map(|v| {
            (
                v.id.0,
                v.lane.unwrap().0,
                round3(v.distance),
                round3(v.speed),
            )
        })
        .collect();
    assert_eq!(end.len(), golden.end.len(), "{name}: vehicles at the end");
    for (here, there) in end.iter().zip(&golden.end) {
        assert_eq!(
            (here.0, here.1),
            (there.0, there.1),
            "{name}: vehicle, lane"
        );
        assert!(
            (here.2 - there.2).abs() <= 0.002 && (here.3 - there.3).abs() <= 0.002,
            "{name}: vehicle {} at {:?}, Godot {:?}",
            here.0,
            (here.2, here.3),
            (there.2, there.3)
        );
    }
    assert_eq!(sim.vehicles.waiting_departures().len(), golden.waiting);

    let buses: Vec<_> = sim
        .transit
        .buses
        .values()
        .map(|b| {
            (
                b.vehicle.0,
                b.line_id.0,
                b.stop_index,
                b.last_index.map_or(-1, |i| i as i32),
                b.riders.len(),
            )
        })
        .collect();
    let expected: Vec<_> = golden
        .buses
        .iter()
        .map(|b| (b.0, b.1, b.2, b.3, b.4))
        .collect();
    assert_eq!(buses, expected, "{name}: buses");
    for (bus, g) in sim.transit.buses.values().zip(&golden.buses) {
        assert!(
            (time(bus.dwell_until) - g.5).abs() <= 0.002,
            "{name}: bus {} dwells until {}, Godot {}",
            g.0,
            bus.dwell_until,
            g.5
        );
    }

    assert_eq!(sim.transit.lines.len(), golden.lines.len());
    for (line, g) in sim.transit.lines.values().zip(&golden.lines) {
        assert_eq!((line.id.0, line.bus_count), (g.0, g.1), "{name}: line");
        assert_eq!(line.segment_times.len(), g.2.len());
        for (got, want) in line.segment_times.iter().zip(&g.2) {
            assert!(
                (time(f64::from(*got)) - want).abs() <= 0.002,
                "{name}: line {} times {:?} vs Godot {:?}",
                g.0,
                line.segment_times,
                g.2
            );
        }
    }

    let stops: Vec<_> = sim
        .transit
        .stops
        .values()
        .map(|s| {
            (
                s.id.0,
                s.lane_id.map_or(-1, |l| l.0),
                s.waiting.len(),
                s.boarded,
                s.bay,
            )
        })
        .collect();
    assert_eq!(stops, golden.stops, "{name}: stops");
}

/// One street with a bay and a curbside stop each way, riders turning up at every stop,
/// 40 cars driving about: they pass the buses in the bays and queue behind the others.
#[test]
fn buses_and_bays_under_traffic_match_godot() {
    let golden: Golden = golden::read("bus_trace");
    let mut sim = Simulation::new();
    add_road(&mut sim, v(0.0, 4.0), v(24.0, 4.0), local());
    let stops = [
        add_stop(&mut sim, v(6.0, 4.0) * G + v(0.0, 10.0), true).unwrap(),
        add_stop(&mut sim, v(18.0, 4.0) * G + v(0.0, 10.0), false).unwrap(),
        add_stop(&mut sim, v(18.0, 4.0) * G + v(0.0, -10.0), true).unwrap(),
        add_stop(&mut sim, v(6.0, 4.0) * G + v(0.0, -10.0), false).unwrap(),
    ];
    let line_id = add_line(&mut sim, &stops).unwrap();
    for i in 0..30 {
        wait(&mut sim, 1000 + i, line_id, stops[0], stops[2]);
    }
    sim.stress.target_count = 40;
    let mut recorder = Recorder::new();
    for tick in 0..(Simulation::TICK_RATE * 90) as usize {
        if tick % 60 == 0 {
            let k = tick / 60;
            wait(
                &mut sim,
                2000 + k as i32,
                line_id,
                stops[k % 4],
                stops[(k + 2) % 4],
            );
        }
        recorder.step(&mut sim, tick);
    }
    compare("bays", &sim, &recorder.events, &golden.bays);
}

/// A street grid with two lines, 80 cars; a street is added at 30 s, the west end becomes
/// an avenue at 60 s, a stop is removed at 75 s and a block of the second line at 90 s.
#[test]
fn lines_through_road_edits_match_godot() {
    let golden: Golden = golden::read("bus_trace");
    let mut sim = Simulation::new();
    for y in [2.0, 4.0, 6.0] {
        add_road(&mut sim, v(2.0, y), v(22.0, y), local());
    }
    for x in [2.0, 8.0, 16.0, 22.0] {
        add_road(&mut sim, v(x, 2.0), v(x, 6.0), local());
    }
    let mut a = Vec::new();
    for x in [5.0, 12.0, 19.0] {
        a.push(add_stop(&mut sim, v(x, 4.0) * G + v(0.0, 10.0), false).unwrap());
    }
    for x in [19.0, 12.0, 5.0] {
        a.push(add_stop(&mut sim, v(x, 4.0) * G + v(0.0, -10.0), false).unwrap());
    }
    let mut b = Vec::new();
    for x in [5.0, 19.0] {
        b.push(add_stop(&mut sim, v(x, 6.0) * G + v(0.0, 10.0), true).unwrap());
    }
    for x in [19.0, 5.0] {
        b.push(add_stop(&mut sim, v(x, 6.0) * G + v(0.0, -10.0), true).unwrap());
    }
    let line_a = add_line(&mut sim, &a).unwrap();
    let line_b = add_line(&mut sim, &b).unwrap();
    sim.stress.target_count = 80;
    let mut recorder = Recorder::new();
    for tick in 0..(Simulation::TICK_RATE * 120) as usize {
        if tick % 40 == 0 {
            let k = tick / 40;
            wait(&mut sim, 3000 + k as i32, line_a, a[k % 6], a[(k + 3) % 6]);
            wait(&mut sim, 5000 + k as i32, line_b, b[k % 4], b[(k + 2) % 4]);
        }
        if tick == 600 {
            add_road(&mut sim, v(14.0, 2.0), v(14.0, 6.0), local());
        }
        if tick == 1200 {
            let id = sim.roads.find_segment_at(v(5.0, 4.0) * G, 4.0, 0).unwrap();
            sim.call(|sim, out| {
                sim.roads
                    .set_segment_type(id, road_type("avenue_4lane"), false, out);
            });
        }
        if tick == 1500 {
            sim.call(|sim, out| {
                let (transit, mut ctx) = sim.transit_ctx();
                transit.remove_stop(&mut ctx, a[1], out);
            });
        }
        if tick == 1800 {
            let id = sim.roads.find_segment_at(v(11.0, 6.0) * G, 4.0, 0).unwrap();
            sim.call(|sim, out| {
                sim.roads.remove_segment(id, out);
            });
        }
        recorder.step(&mut sim, tick);
    }
    compare("grid", &sim, &recorder.events, &golden.grid);
}

/// Two lines crossing at a shared stop and a third apart, a minute of buses (so the
/// timings are measured), then 200 rides planned between points near the streets: the
/// same cost (to 1e-6) and the same legs as Godot's `plan_ride`, including the ones that
/// change lines (76 of them have a plan, 11 with a change).
#[test]
fn planned_rides_match_godot() {
    let golden: Golden = golden::read("bus_trace");
    let mut sim = Simulation::new();
    add_road(&mut sim, v(0.0, 4.0), v(24.0, 4.0), local());
    add_road(&mut sim, v(12.0, 4.0), v(12.0, 20.0), local());
    let stop = |sim: &mut Simulation, x: f32, y: f32, dx: f32, dy: f32| {
        add_stop(sim, v(x, y) * G + v(dx, dy), false).unwrap()
    };
    let shared = stop(&mut sim, 9.0, 4.0, 0.0, 10.0);
    let first = [
        stop(&mut sim, 2.0, 4.0, 0.0, 10.0),
        shared,
        stop(&mut sim, 9.0, 4.0, 0.0, -10.0),
        stop(&mut sim, 2.0, 4.0, 0.0, -10.0),
    ];
    add_line(&mut sim, &first).unwrap();
    let second = [
        shared,
        stop(&mut sim, 12.0, 7.0, -10.0, 0.0),
        stop(&mut sim, 12.0, 18.0, -10.0, 0.0),
        stop(&mut sim, 12.0, 18.0, 10.0, 0.0),
    ];
    add_line(&mut sim, &second).unwrap();
    let third = [
        stop(&mut sim, 20.0, 4.0, 0.0, 10.0),
        stop(&mut sim, 15.0, 4.0, 0.0, -10.0),
    ];
    add_line(&mut sim, &third).unwrap();
    for _ in 0..Simulation::TICK_RATE * 60 {
        sim.step();
    }
    let (mut planned, mut changes) = (0, 0);
    for (i, g) in golden.plans.plans.iter().enumerate() {
        let (transit, mut ctx) = sim.transit_ctx();
        let plan = transit.plan_ride(
            &mut ctx,
            Vec2::from_array(g.from),
            Vec2::from_array(g.to),
            g.max_walk,
            g.weight,
            &mut Vec::new(),
        );
        let Some(plan) = plan else {
            assert!(g.cost.is_none(), "plan {i}: Godot found one");
            continue;
        };
        let cost = g
            .cost
            .unwrap_or_else(|| panic!("plan {i}: Godot found none"));
        assert!(
            (plan.cost - cost).abs() < 1e-6,
            "plan {i}: cost {} vs Godot {cost}",
            plan.cost
        );
        let legs: Vec<(i32, i32, i32, i32)> = plan
            .legs
            .iter()
            .map(|leg| match leg {
                RideLeg::Walk { .. } => (0, 0, 0, 0),
                RideLeg::Bus { line, from, to } => (1, line.0, from.0, to.0),
            })
            .collect();
        assert_eq!(legs, g.legs, "plan {i}: legs");
        planned += 1;
        changes += usize::from(legs.iter().filter(|l| l.0 == 1).count() > 1);
    }
    assert_eq!(planned, 76);
    assert!(changes >= 10, "{changes} rides change lines");
}
