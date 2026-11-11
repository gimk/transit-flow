//! R2 review gate: traffic traced against Godot (`migration/probes/traffic_trace.gd`,
//! `migration/golden/traffic_trace.json`) in what the stress grids don't cover: road edits
//! under traffic (a block removed, one retyped, the first re-added) and freight through
//! driveways and the highway gate (a whole game day). Every arrival and failure must happen on the same tick,
//! and every vehicle end on the same lane, at the same distance and speed.

mod common;

use common::golden;
use common::roads::{G, local, road_type};
use common::traffic::road;
use glam::{IVec2, Vec2};
use serde::Deserialize;
use tf_sim::Simulation;
use tf_sim::city::LotType;
use tf_sim::clock::SimClock;
use tf_sim::debug::scenarios;
use tf_sim::events::SimEvent;
use tf_sim::grid;
use tf_sim::roads::JunctionControl;

#[derive(Deserialize)]
struct Trace {
    events: Vec<(usize, String, i32)>,
    end: Vec<(i32, i32, f64, f64)>,
    waiting: usize,
}

#[derive(Deserialize)]
struct Golden {
    edits: Trace,
    freight: Trace,
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// Steps once and records what the vehicles reported, as the probe does.
fn step(sim: &mut Simulation, tick: usize, events: &mut Vec<(usize, String, i32)>) {
    sim.step();
    for event in sim.take_view_events() {
        match event {
            SimEvent::VehicleArrived(id) => events.push((tick, "a".into(), id.0)),
            SimEvent::VehicleFailed(id) => {
                let gave_up = sim.vehicles.vehicle_any(id).unwrap().gave_up;
                events.push((tick, if gave_up { "g" } else { "f" }.into(), id.0));
            }
            _ => {}
        }
    }
}

fn compare(name: &str, sim: &Simulation, events: &[(usize, String, i32)], golden: &Trace) {
    let first_difference = events.iter().zip(&golden.events).position(|(a, b)| a != b);
    assert!(
        first_difference.is_none() && events.len() == golden.events.len(),
        "{name}: events differ from Godot at #{first_difference:?} ({} here, {} in Godot)",
        events.len(),
        golden.events.len()
    );
    let end: Vec<(i32, i32, f64, f64)> = sim
        .vehicles
        .vehicles
        .values()
        .map(|v| {
            (
                v.id.0,
                v.lane.unwrap().0,
                (v.distance * 1000.0).round() / 1000.0,
                (v.speed * 1000.0).round() / 1000.0,
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
    assert_eq!(
        sim.vehicles.waiting_departures().len(),
        golden.waiting,
        "{name}: waiting in driveways"
    );
}

#[test]
fn road_edits_under_traffic_match_godot() {
    let golden: Golden = golden::read("traffic_trace");
    let mut sim = Simulation::new();
    for i in 0..5 {
        let at = (1 + i * 3) as f32;
        road(&mut sim, v(1.0, at), v(13.0, at), local());
        road(&mut sim, v(at, 1.0), v(at, 13.0), local());
    }
    let junctions: Vec<_> = sim
        .roads
        .nodes
        .values()
        .filter(|n| n.degree() >= 3)
        .map(|n| n.id)
        .collect();
    for node in junctions.into_iter().step_by(2) {
        sim.call(|sim, out| {
            sim.roads
                .set_node_control(node, JunctionControl::TrafficLight, out);
        });
    }
    sim.stress.target_count = 250;
    let mut events = Vec::new();
    for tick in 0..(Simulation::TICK_RATE * 120) as usize {
        if tick == 600 {
            let id = sim.roads.find_segment_at(v(4.0, 5.5) * G, 4.0, 0).unwrap();
            sim.call(|sim, out| {
                sim.roads.remove_segment(id, out);
            });
        }
        if tick == 1200 {
            let id = sim.roads.find_segment_at(v(8.5, 7.0) * G, 4.0, 0).unwrap();
            sim.call(|sim, out| {
                sim.roads
                    .set_segment_type(id, road_type("avenue_4lane"), false, out);
            });
        }
        if tick == 1800 {
            road(&mut sim, v(4.0, 4.0), v(4.0, 7.0), local());
        }
        step(&mut sim, tick, &mut events);
    }
    compare("edits", &sim, &events, &golden.edits);
}

#[test]
fn freight_trips_match_godot() {
    let golden: Golden = golden::read("traffic_trace");
    let mut sim = Simulation::new();
    scenarios::add_highway(&mut sim);
    let gate = sim.externals.points[0];
    sim.call(|sim, out| {
        sim.roads.add_road(
            gate + v(6.0, 0.0) * G,
            gate + v(20.0, 0.0) * G,
            local(),
            0,
            out,
        );
    });
    let row = grid::world_to_cell(gate).y;
    let first_x = grid::world_to_cell(gate).x;
    let lots = (0..5)
        .map(|x| (first_x + 8 + x, LotType::Industrial))
        .chain((0..3).map(|x| (first_x + 15 + x, LotType::Commercial)));
    for (x, lot_type) in lots {
        sim.call(|sim, out| {
            sim.lots
                .add_lot(&sim.roads, IVec2::new(x, row), lot_type, out);
        });
    }
    sim.clock.total_minutes = 10.5 * 60.0;
    let mut events = Vec::new();
    let ticks = (24.0 * 60.0 / SimClock::GAME_MINUTES_PER_SIM_SECOND
        * f64::from(Simulation::TICK_RATE)) as usize;
    for tick in 0..ticks {
        step(&mut sim, tick, &mut events);
    }
    compare("freight", &sim, &events, &golden.freight);
}
