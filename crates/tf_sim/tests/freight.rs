//! Port of `tests/unit/test_freight.gd` (Phase 3b: highway connections and freight
//! trucks).

mod common;

use common::roads::{G, local};
use glam::{IVec2, Vec2};
use tf_sim::Simulation;
use tf_sim::city::LotType;
use tf_sim::clock::SimClock;
use tf_sim::debug::scenarios;
use tf_sim::grid;
use tf_sim::roads::lane_builder;
use tf_sim::traffic::FreightSystem;
use tf_sim::traffic::vehicle::Kind;

/// A highway extended into a street with five factories and three shops on it.
fn setup() -> (Simulation, Vec2) {
    let mut sim = Simulation::new();
    scenarios::add_highway(&mut sim);
    let gate = sim.externals.points[0];
    sim.call(|sim, out| {
        sim.roads.add_road(
            gate + Vec2::new(6.0, 0.0) * G,
            gate + Vec2::new(20.0, 0.0) * G,
            local(),
            0,
            out,
        );
    });
    let row = grid::world_to_cell(gate).y;
    let first_x = grid::world_to_cell(gate).x;
    for x in 0..5 {
        sim.call(|sim, out| {
            sim.lots.add_lot(
                &sim.roads,
                IVec2::new(first_x + 8 + x, row),
                LotType::Industrial,
                out,
            );
        });
    }
    for x in 0..3 {
        sim.call(|sim, out| {
            sim.lots.add_lot(
                &sim.roads,
                IVec2::new(first_x + 15 + x, row),
                LotType::Commercial,
                out,
            );
        });
    }
    (sim, gate)
}

fn set_hour(sim: &mut Simulation, hour: f64) {
    sim.clock.total_minutes = hour * 60.0;
}

/// Number of trucks that set off during `game_hours`.
fn count_trucks(sim: &mut Simulation, game_hours: f64) -> usize {
    let mut seen = std::collections::HashSet::new();
    let ticks = (game_hours * 60.0 / SimClock::GAME_MINUTES_PER_SIM_SECOND
        * f64::from(Simulation::TICK_RATE)) as usize;
    for _ in 0..ticks {
        sim.step();
        for vehicle in sim.vehicles.vehicles.values() {
            if vehicle.kind == Kind::Truck {
                seen.insert(vehicle.id);
            }
        }
    }
    seen.len()
}

#[test]
fn highway_connection_has_lanes_in_and_out() {
    let (sim, gate) = setup();
    let inbound = sim.externals.inbound(&sim.roads, gate).unwrap();
    assert!(sim.externals.outbound(&sim.roads, gate).is_some());
    let in_lane = &sim.roads.lanes[&inbound.lane_id];
    assert!(
        lane_builder::start_direction(in_lane.curve()).x > 0.9,
        "inbound lane heads into the map"
    );
    assert_eq!(sim.externals.active_points(&sim.roads), vec![gate]);
}

#[test]
fn connection_is_inactive_without_a_road() {
    let (mut sim, gate) = setup();
    let stub = sim
        .roads
        .find_segment_at(gate + Vec2::new(G, 0.0), 4.0, 0)
        .unwrap();
    sim.call(|sim, out| {
        sim.roads.remove_segment(stub, out);
    });
    assert!(sim.externals.active_points(&sim.roads).is_empty());
    assert!(sim.externals.inbound(&sim.roads, gate).is_none());
}

#[test]
fn freight_peaks_at_midday() {
    let (mut sim, _) = setup();
    set_hour(&mut sim, 10.5);
    let midday = count_trucks(&mut sim, 5.0);
    set_hour(&mut sim, 0.5);
    let night = count_trucks(&mut sim, 5.0);
    println!("trucks in 5 game hours: {midday} midday, {night} at night");
    assert!(midday > 0, "trucks run at midday");
    assert!(midday > night * 2, "far busier at midday than at night");
}

#[test]
fn activity_curve() {
    assert_eq!(FreightSystem::activity(12.0), 1.0);
    assert!(FreightSystem::activity(8.0) < 1.0);
    assert!(FreightSystem::activity(2.0) < FreightSystem::activity(8.0));
}

#[test]
fn saves_external_connections() {
    let (sim, _) = setup();
    let saved = sim.to_dict();
    let loaded = Simulation::from_dict(&serde_json::from_str(&saved.to_string()).unwrap())
        .expect("the save loads");
    assert_eq!(loaded.externals.points, sim.externals.points);
}
