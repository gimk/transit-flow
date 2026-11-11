//! Port of `tests/unit/test_bottleneck.gd` (Phase 2c "done when"): a single bottleneck
//! junction visibly jams, a parallel road draws drivers out of the jam, and rerouting is
//! observable. Same scenario as F7 in game.

mod common;

use std::collections::HashSet;

use glam::IVec2;
use tf_sim::Simulation;
use tf_sim::city::citizen::State;
use tf_sim::debug::scenarios::{self, BottleneckInfo};
use tf_sim::events::SimEvent;
use tf_sim::ids::{CitizenId, LaneId, SegmentId};
use tf_sim::roads::{JunctionControl, lane_builder};

const ORIGIN: IVec2 = IVec2::new(30, 30);

fn built() -> (Simulation, BottleneckInfo) {
    let mut sim = Simulation::new();
    let info = scenarios::build_bottleneck(&mut sim, ORIGIN);
    (sim, info)
}

/// The main-road lane arriving at the lights from the west.
fn approach_lane(sim: &Simulation, info: &BottleneckInfo) -> LaneId {
    let junction = sim.roads.find_node_at(info.junction, 0).unwrap();
    sim.roads
        .lanes
        .values()
        .find(|lane| {
            !lane.is_connector()
                && lane.to_node_id == junction
                && lane_builder::end_direction(lane.curve()).x > 0.9
        })
        .map(|lane| lane.id)
        .expect("a lane arrives at the lights from the west")
}

/// What `_run_morning` measured.
struct Morning {
    time: f64,
    driving: f64,
    used_watched: usize,
}

/// Runs until every commuter has reached work. Returns the sim time (infinite on
/// timeout), the total time commuters spent driving to work, and how many of them drove
/// over any of `watched`.
fn run_morning(sim: &mut Simulation, watched: &[SegmentId]) -> Morning {
    let mut reached_work: HashSet<CitizenId> = HashSet::new();
    let mut seen: HashSet<i32> = HashSet::new();
    let mut driving = 0.0;
    for _ in 0..Simulation::TICK_RATE * 300 {
        sim.step();
        for event in sim.take_view_events() {
            if let SimEvent::VehicleArrived(id) = event {
                let vehicle = sim.vehicles.vehicle_any(id).unwrap();
                if let Some(citizen) = sim.citizens.citizens.get(&CitizenId(vehicle.owner_id))
                    && vehicle.destination_lot_id == citizen.work_lot_id
                {
                    driving += vehicle.trip_end_time - vehicle.trip_start_time;
                }
            }
        }
        for vehicle in sim.vehicles.vehicles.values() {
            let commuting = sim.citizens.citizens.get(&CitizenId(vehicle.owner_id));
            let to_work = commuting.is_some_and(|c| c.state == State::ToWork);
            let lane = sim.roads.lane_any(vehicle.lane.unwrap()).unwrap();
            if to_work && lane.segment_id.is_some_and(|s| watched.contains(&s)) {
                seen.insert(vehicle.owner_id);
            }
        }
        for citizen in sim.citizens.citizens.values() {
            if citizen.state == State::AtWork {
                reached_work.insert(citizen.id);
            }
        }
        if reached_work.len() == sim.citizens.citizens.len() {
            return Morning {
                time: sim.sim_time(),
                driving,
                used_watched: seen.len(),
            };
        }
    }
    Morning {
        time: f64::INFINITY,
        driving,
        used_watched: seen.len(),
    }
}

#[test]
fn scenario_builds() {
    let (mut sim, info) = built();
    assert_eq!(sim.citizens.citizens.len(), 56);
    let junction = sim.roads.find_node_at(info.junction, 0).unwrap();
    assert_eq!(
        sim.roads.nodes[&junction].control,
        JunctionControl::TrafficLight
    );
    let ids: Vec<_> = sim.lots.lots.keys().copied().collect();
    for id in ids {
        assert!(
            sim.lots.access(&sim.roads, id).is_some(),
            "every lot reaches a road"
        );
    }
}

#[test]
fn queue_builds_at_the_lights_and_shows_as_congestion() {
    let (mut sim, info) = built();
    let approach = approach_lane(&sim, &info);
    let mut longest = 0;
    let mut worst_congestion = 0.0f64;
    for _ in 0..Simulation::TICK_RATE * 30 {
        sim.step();
        longest = longest.max(sim.vehicles.queue(approach).len());
        worst_congestion = worst_congestion.max(
            sim.congestion
                .congestion
                .get(&approach)
                .copied()
                .unwrap_or(0.0),
        );
    }
    println!(
        "longest queue on the approach: {longest}, worst congestion there: {worst_congestion:.2}"
    );
    assert!(longest >= 6, "a queue forms");
    assert!(worst_congestion > 0.5, "the approach is marked congested");
    // Godot: 26 and 6.21.
    assert_eq!(longest, 26);
    assert_eq!(format!("{worst_congestion:.2}"), "6.21");
}

/// Rebuilds the scenario with a given departure order (and optionally the bypass), then
/// runs the morning, counting commuters who drove over the bypass.
fn morning_for_seed(seed: u64, bypass: bool) -> Morning {
    let mut sim = Simulation::new();
    sim.citizens.set_seed(seed);
    let info = scenarios::build_bottleneck(&mut sim, ORIGIN);
    let before: Vec<SegmentId> = sim.roads.segments.keys().copied().collect();
    if bypass {
        scenarios::build_bypass(&mut sim, &info);
    }
    let y = info.junction.y;
    let on_main_road = |sim: &Simulation, id: &SegmentId| {
        let segment = &sim.roads.segments[id];
        let start = sim.roads.nodes[&segment.start_node_id].position.y;
        let end = sim.roads.nodes[&segment.end_node_id].position.y;
        (start - y).abs() < 0.0001 && (end - y).abs() < 0.0001
    };
    let watched: Vec<SegmentId> = sim
        .roads
        .segments
        .keys()
        .filter(|id| !before.contains(id) && !on_main_road(&sim, id))
        .copied()
        .collect();
    run_morning(&mut sim, &watched)
}

/// The simulation is deterministic but chaotic (small differences in who leaves when
/// change which drivers get stuck), so compare over a few departure orders.
#[test]
fn bypass_draws_drivers_out_of_the_jam() {
    // Godot's numbers: everyone at work (s), driving (s), took the bypass.
    let godot = [
        (1, (80.4, 1169.0), (82.1, 1187.0, 2)),
        (2, (88.7, 814.0), (87.9, 883.0, 2)),
        (3, (89.8, 977.0), (89.8, 979.0, 1)),
    ];
    let (mut total_without, mut total_with, mut took_bypass) = (0.0, 0.0, 0);
    for (seed, without_godot, with_godot) in godot {
        let without = morning_for_seed(seed, false);
        let with_bypass = morning_for_seed(seed, true);
        println!(
            "seed {seed}: everyone at work {:.1} s / commuting {:.0} s without bypass, {:.1} s / {:.0} s with ({} took it)",
            without.time,
            without.driving,
            with_bypass.time,
            with_bypass.driving,
            with_bypass.used_watched
        );
        assert!(
            without.time < f64::INFINITY,
            "the morning finishes even without the bypass"
        );
        total_without += without.driving;
        total_with += with_bypass.driving;
        took_bypass += with_bypass.used_watched;
        assert_eq!(
            format!("{:.1}", without.time),
            format!("{:.1}", without_godot.0)
        );
        assert_eq!(
            format!("{:.0}", without.driving),
            format!("{:.0}", without_godot.1)
        );
        assert_eq!(
            format!("{:.1}", with_bypass.time),
            format!("{:.1}", with_godot.0)
        );
        assert_eq!(
            format!("{:.0}", with_bypass.driving),
            format!("{:.0}", with_godot.1)
        );
        assert_eq!(with_bypass.used_watched, with_godot.2);
    }
    // Every factory sits on the one main road, so no bypass adds capacity where the cars
    // end up: the bypass spreads the load (drivers held up in the queue take it) but can't
    // do much better than break even. It must not make the morning worse.
    assert!(
        total_with < total_without * 1.05,
        "the bypass doesn't make commuting (much) slower"
    );
    assert!(
        took_bypass >= 5,
        "congestion pushed traffic onto the bypass"
    );
}
