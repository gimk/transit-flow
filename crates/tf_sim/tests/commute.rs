//! Port of `tests/unit/test_commute.gd` (Phase 3b: citizens, jobs and the daily
//! schedule, on a whole `Simulation`).

mod common;

use std::collections::HashMap;

use common::citizens::add_lot;
use common::roads::{G, local};
use glam::{IVec2, Vec2};
use tf_sim::Simulation;
use tf_sim::city::LotType;
use tf_sim::city::citizen::State;
use tf_sim::clock::SimClock;
use tf_sim::ids::{CitizenId, SegmentId};

fn road(sim: &mut Simulation, from: Vec2, to: Vec2) -> SegmentId {
    let mut id = None;
    sim.call(|sim, out| id = sim.roads.add_road(from * G, to * G, local(), 0, out));
    id.expect("test setup: the road fits")
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn cell(x: i32, y: i32) -> IVec2 {
    IVec2::new(x, y)
}

fn steps(sim: &mut Simulation, count: u32) {
    for _ in 0..count {
        sim.step();
    }
}

/// Runs the sim for `game_hours`, recording (hour of day, new state) whenever a citizen's
/// state changes. Returns citizen id → list of (hour, state).
fn run_hours(sim: &mut Simulation, game_hours: f64) -> HashMap<CitizenId, Vec<(f64, State)>> {
    let mut history: HashMap<CitizenId, Vec<(f64, State)>> = HashMap::new();
    let mut last: HashMap<CitizenId, State> = HashMap::new();
    let ticks = (game_hours * 60.0 / SimClock::GAME_MINUTES_PER_SIM_SECOND
        * f64::from(Simulation::TICK_RATE)) as u32;
    for _ in 0..ticks {
        sim.step();
        for citizen in sim.citizens.citizens.values() {
            if last.get(&citizen.id) != Some(&citizen.state) {
                last.insert(citizen.id, citizen.state);
                history
                    .entry(citizen.id)
                    .or_default()
                    .push((sim.clock.time_of_day_hours(), citizen.state));
            }
        }
    }
    history
}

/// A street with homes on the north side and workplaces on the south side.
fn small_town(sim: &mut Simulation, homes: i32, factories: i32, shops: i32) {
    road(sim, v(0.0, 4.0), v(20.0, 4.0));
    for x in 0..homes {
        add_lot(sim, cell(1 + x, 3), LotType::Residential);
    }
    for x in 0..factories {
        add_lot(sim, cell(10 + x, 4), LotType::Industrial);
    }
    for x in 0..shops {
        add_lot(sim, cell(16 + x, 4), LotType::Commercial);
    }
}

#[test]
fn each_home_has_two_residents_who_find_the_nearest_free_jobs() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(20.0, 4.0));
    add_lot(&mut sim, cell(1, 3), LotType::Residential);
    add_lot(&mut sim, cell(2, 3), LotType::Residential);
    let near_shop = add_lot(&mut sim, cell(3, 4), LotType::Commercial).unwrap(); // 2 jobs
    let far_factory = add_lot(&mut sim, cell(15, 4), LotType::Industrial).unwrap(); // 3 jobs
    steps(&mut sim, 2);
    assert_eq!(sim.citizens.citizens.len(), 4);
    let workplaces: Vec<_> = sim
        .citizens
        .citizens
        .values()
        .map(|c| c.work_lot_id)
        .collect();
    assert_eq!(
        workplaces.iter().filter(|&&w| w == Some(near_shop)).count(),
        2,
        "nearest workplace fills up first"
    );
    assert_eq!(
        workplaces
            .iter()
            .filter(|&&w| w == Some(far_factory))
            .count(),
        2
    );
    assert_eq!(sim.citizens.job_count(), 5);
    assert_eq!(sim.citizens.employed_count(), 4);
}

#[test]
fn without_enough_jobs_some_stay_home() {
    let mut sim = Simulation::new();
    small_town(&mut sim, 3, 0, 1); // 6 people, 2 jobs.
    let history = run_hours(&mut sim, 4.0);
    let mut commuters = 0;
    for citizen in sim.citizens.citizens.values() {
        if citizen.work_lot_id.is_some() {
            commuters += 1;
        } else {
            assert_eq!(
                history[&citizen.id].len(),
                1,
                "the unemployed never leave home"
            );
        }
    }
    assert_eq!(commuters, 2);
}

#[test]
fn a_day_follows_the_rush_hours() {
    let mut sim = Simulation::new();
    small_town(&mut sim, 4, 3, 0);
    let history = run_hours(&mut sim, 24.0);
    for events in history.values() {
        let left_home: Vec<_> = events.iter().filter(|e| e.1 == State::ToWork).collect();
        let left_work: Vec<_> = events.iter().filter(|e| e.1 == State::ToHome).collect();
        assert_eq!(left_home.len(), 1, "one commute a day");
        assert_eq!(left_work.len(), 1);
        assert!(
            (6.5..=9.1).contains(&left_home[0].0),
            "left home in the morning rush"
        );
        assert!(
            (16.0..=19.1).contains(&left_work[0].0),
            "left work in the evening rush"
        );
        assert_eq!(
            events.last().unwrap().1,
            State::AtHome,
            "home again at the end of the day"
        );
    }
}

#[test]
fn some_people_go_shopping_after_work() {
    let mut sim = Simulation::new();
    small_town(&mut sim, 8, 3, 2);
    let history = run_hours(&mut sim, 24.0);
    let shoppers = history
        .values()
        .filter(|events| events.iter().any(|e| e.1 == State::AtShop))
        .count();
    assert!(
        (1..=15).contains(&shoppers),
        "some, not all, went shopping ({shoppers})"
    );
}

#[test]
fn bulldozing_the_only_road_mid_trip_still_ends_at_the_destination() {
    let mut sim = Simulation::new();
    let segment = road(&mut sim, v(0.0, 4.0), v(8.0, 4.0));
    add_lot(&mut sim, cell(1, 3), LotType::Residential);
    add_lot(&mut sim, cell(6, 3), LotType::Industrial);
    sim.citizens.depart_everyone_now(0.0);
    while !sim
        .citizens
        .citizens
        .values()
        .any(|c| c.state == State::ToWork)
    {
        sim.step();
    }
    sim.call(|sim, out| {
        sim.roads.remove_segment(segment, out);
    });
    sim.step();
    assert_eq!(sim.vehicles.vehicles.len(), 0);
    assert!(
        sim.citizens
            .citizens
            .values()
            .all(|c| c.state == State::AtWork || c.state == State::AtHome)
    );
}

#[test]
fn losing_the_workplace_mid_trip_sends_people_home_job_hunting() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(8.0, 4.0));
    add_lot(&mut sim, cell(1, 3), LotType::Residential);
    let work = add_lot(&mut sim, cell(6, 3), LotType::Industrial).unwrap();
    sim.citizens.depart_everyone_now(0.0);
    while sim.vehicles.vehicles.is_empty() {
        sim.step();
    }
    sim.call(|sim, out| {
        sim.lots.remove_lot(work, out);
    });
    sim.step();
    for citizen in sim.citizens.citizens.values() {
        assert_eq!(citizen.state, State::AtHome);
        assert_eq!(citizen.work_lot_id, None);
    }
    assert_eq!(sim.vehicles.vehicles.len(), 0);
}

#[test]
#[ignore = "needs Simulation saves (M7.3): test_save_round_trip_keeps_lots_and_recreates_residents"]
fn save_round_trip_keeps_lots_and_recreates_residents() {}

#[test]
fn shoppers_never_pick_the_shop_they_work_at() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(12.0, 4.0));
    let only_shop = add_lot(&mut sim, cell(5, 3), LotType::Commercial).unwrap();
    assert_eq!(
        sim.citizens.pick_shop(&sim.lots, only_shop),
        None,
        "the only shop is their own: no shopping"
    );
    let other = add_lot(&mut sim, cell(8, 3), LotType::Commercial).unwrap();
    for _ in 0..10 {
        assert_eq!(sim.citizens.pick_shop(&sim.lots, only_shop), Some(other));
    }
}
