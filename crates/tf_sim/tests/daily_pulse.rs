//! Port of `tests/unit/test_daily_pulse.gd`: Phase 3 "done when": one in-game day plays
//! through the four pulse periods with visibly different traffic: commuter cars in the
//! rushes, trucks at midday, quiet nights.

mod common;

use std::collections::{HashMap, HashSet};

use common::roads::{G, local};
use glam::{IVec2, Vec2};
use tf_sim::Simulation;
use tf_sim::city::LotType;
use tf_sim::clock::{Period, SimClock};
use tf_sim::debug::scenarios;
use tf_sim::grid;
use tf_sim::traffic::vehicle::Kind;

fn cells(from: IVec2, to: IVec2) -> Vec<IVec2> {
    let mut cells = Vec::new();
    for x in from.x..=to.x {
        for y in from.y..=to.y {
            cells.push(IVec2::new(x, y));
        }
    }
    cells
}

#[test]
fn a_day_has_four_distinct_traffic_periods() {
    let mut sim = Simulation::new();
    scenarios::add_highway(&mut sim);
    let origin = grid::world_to_cell(sim.externals.points[0]) + IVec2::new(6, 0);
    let at = |x: i32, y: i32| Vec2::new((origin.x + x) as f32, (origin.y + y) as f32) * G;
    let road = |sim: &mut Simulation, a: Vec2, b: Vec2| {
        sim.call(|sim, out| {
            sim.roads.add_road(a, b, local(), 0, out);
        });
    };
    road(&mut sim, at(0, 0), at(16, 0));
    for x in [0, 4, 8, 12, 16] {
        road(&mut sim, at(x, -3), at(x, 3));
    }
    road(&mut sim, at(0, -3), at(16, -3));
    road(&mut sim, at(0, 3), at(16, 3));
    let zones = [
        (IVec2::new(0, -3), IVec2::new(7, 2), LotType::Residential),
        (IVec2::new(8, -3), IVec2::new(15, -1), LotType::Commercial),
        (IVec2::new(8, 0), IVec2::new(15, 2), LotType::Industrial),
    ];
    for (from, to, lot_type) in zones {
        let area = cells(origin + from, origin + to);
        sim.call(|sim, out| sim.zones.paint(&mut sim.lots, &area, lot_type, out));
    }

    // New vehicles per period: [cars, trucks].
    let mut counts: HashMap<Period, [i32; 2]> = HashMap::new();
    for period in [
        Period::MorningRush,
        Period::MiddayLogistics,
        Period::EveningRush,
        Period::Night,
    ] {
        counts.insert(period, [0, 0]);
    }
    let mut seen = HashSet::new();
    let ticks = (f64::from(SimClock::MINUTES_PER_DAY) / SimClock::GAME_MINUTES_PER_SIM_SECOND
        * f64::from(Simulation::TICK_RATE)) as u32;
    for _ in 0..ticks {
        sim.step();
        let period = sim.clock.period();
        for vehicle in sim.vehicles.vehicles.values() {
            if seen.insert(vehicle.id) {
                counts.get_mut(&period).unwrap()[usize::from(vehicle.kind == Kind::Truck)] += 1;
            }
        }
    }
    let morning = counts[&Period::MorningRush];
    let midday = counts[&Period::MiddayLogistics];
    let evening = counts[&Period::EveningRush];
    let night = counts[&Period::Night];
    // Godot prints: cars/trucks per period: morning [61, 23], midday [0, 74],
    // evening [59, 24], night [6, 13], flow 71%.
    assert_eq!(
        (morning, midday, evening, night),
        ([61, 23], [0, 74], [59, 24], [6, 13]),
        "the same vehicles per period as Godot"
    );

    assert!(morning[0] > morning[1], "morning: mostly commuter cars");
    assert!(midday[1] > midday[0], "midday: mostly trucks");
    assert!(evening[0] > evening[1], "evening: mostly commuter cars");
    let night_total = night[0] + night[1];
    assert!(night_total < morning[0], "night is quiet");
    let now = sim.sim_time();
    let flow = sim.flow.flow_percent(now);
    assert_eq!(format!("{flow:.0}"), "71", "the same Flow as Godot (71%)");
    assert!(flow > 0.0, "trips were scored");
}
