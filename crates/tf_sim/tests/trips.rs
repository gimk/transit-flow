//! Port of `tests/unit/test_trips.gd` (Phase 8a: trips as legs, car availability,
//! parking and mode choice).

mod common;

use common::citizens::{add_lot, citizen, modes, nth, plan_trip, trips_ended};
use common::roads::{G, local};
use glam::{IVec2, Vec2};
use tf_sim::Simulation;
use tf_sim::city::LotType;
use tf_sim::city::citizen::{Leg, Mode, State};
use tf_sim::city::parking::Parking;
use tf_sim::ids::{CitizenId, LotId};

struct Fixture {
    sim: Simulation,
    home: LotId,
    work: LotId,
}

fn cell(x: i32, y: i32) -> IVec2 {
    IVec2::new(x, y)
}

/// `before_each`.
fn fixture() -> Fixture {
    let mut sim = Simulation::new();
    sim.call(|sim, out| {
        sim.roads.add_road(
            Vec2::new(0.0, 4.0) * G,
            Vec2::new(20.0, 4.0) * G,
            local(),
            0,
            out,
        );
    });
    let home = add_lot(&mut sim, cell(2, 3), LotType::Residential).unwrap();
    let work = add_lot(&mut sim, cell(14, 3), LotType::Commercial).unwrap();
    sim.step(); // Citizens move in and find jobs.
    Fixture { sim, home, work }
}

/// A trip that ended while running.
struct Trip {
    mode: Mode,
    completed: bool,
    ideal: f64,
    took: f64,
}

/// Steps until `citizen` reaches `state` (or `max_ticks` pass); returns ended trips.
fn run_until(sim: &mut Simulation, id: CitizenId, state: State, max_ticks: u32) -> Vec<Trip> {
    let mut trips = Vec::new();
    trips_ended(sim);
    for _ in 0..max_ticks {
        if sim.citizens.citizens[&id].state == state {
            break;
        }
        sim.step();
        for (who, completed) in trips_ended(sim) {
            if who == id {
                let c = &sim.citizens.citizens[&id];
                trips.push(Trip {
                    mode: c.trip_mode,
                    completed,
                    ideal: c.trip_ideal_time,
                    took: c.trip_end_time - c.trip_start_time,
                });
            }
        }
    }
    trips
}

#[test]
fn far_trips_are_driven_near_ones_walked() {
    let Fixture {
        mut sim,
        home,
        work,
    } = fixture();
    let id = nth(&sim, 0);
    assert_eq!(
        citizen(&mut sim, id).car_lot_id,
        home,
        "the car starts at home"
    );
    assert_eq!(
        plan_trip(&mut sim, id, home, work, false).unwrap().mode,
        Mode::Car,
        "12 cells: drive"
    );
    let next_door = add_lot(&mut sim, cell(4, 3), LotType::Commercial).unwrap();
    assert_eq!(
        plan_trip(&mut sim, id, home, next_door, false)
            .unwrap()
            .mode,
        Mode::Walk,
        "2 cells: walk"
    );
}

#[test]
fn a_commute_by_car_ends_with_the_car_at_work() {
    let Fixture { mut sim, work, .. } = fixture();
    let id = nth(&sim, 0);
    citizen(&mut sim, id).leave_now = true;
    let trips = run_until(&mut sim, id, State::AtWork, 2000);
    assert_eq!(sim.citizens.citizens[&id].state, State::AtWork);
    assert_eq!(trips.len(), 1, "one trip, scored once");
    assert_eq!(trips[0].mode, Mode::Car);
    assert!(trips[0].completed);
    assert_eq!(
        sim.citizens.citizens[&id].car_lot_id, work,
        "parked at work"
    );
    assert_eq!(
        sim.citizens.parking.used(work),
        1,
        "taking one of its spaces"
    );
}

#[test]
fn without_a_car_people_walk_however_far() {
    let Fixture {
        mut sim,
        home,
        work,
    } = fixture();
    let id = nth(&sim, 0);
    citizen(&mut sim, id).has_car = false;
    let plan = plan_trip(&mut sim, id, home, work, false).unwrap();
    assert_eq!(modes(&plan), vec![Mode::Walk]);
    citizen(&mut sim, id).leave_now = true;
    let trips = run_until(&mut sim, id, State::AtWork, 4000);
    assert_eq!(sim.citizens.citizens[&id].state, State::AtWork);
    assert_eq!(trips[0].mode, Mode::Walk);
}

#[test]
fn a_full_car_park_sends_drivers_next_door_to_walk_the_rest() {
    let Fixture {
        mut sim,
        home,
        work,
    } = fixture();
    let next_door = add_lot(&mut sim, cell(15, 3), LotType::Commercial).unwrap();
    for _ in 0..Parking::capacity_of(LotType::Commercial).unwrap() {
        sim.citizens.parking.take(&sim.lots, work);
    }
    let id = nth(&sim, 0);
    let plan = plan_trip(&mut sim, id, home, work, false).unwrap();
    assert_eq!(modes(&plan), vec![Mode::Car, Mode::Walk]);
    assert!(
        matches!(plan.legs[0], Leg::Car { to, .. } if to == next_door),
        "parks at the nearest free space"
    );
    citizen(&mut sim, id).leave_now = true;
    let trips = run_until(&mut sim, id, State::AtWork, 2000);
    assert_eq!(
        sim.citizens.citizens[&id].state,
        State::AtWork,
        "walked the rest"
    );
    assert_eq!(sim.citizens.citizens[&id].car_lot_id, next_door);
    assert_eq!(trips.len(), 1);
    assert!(trips[0].ideal > 0.0);
    let _ = trips[0].took;
}

#[test]
fn no_space_within_reach_means_parking_on_the_street() {
    let Fixture {
        mut sim,
        home,
        work,
    } = fixture();
    for _ in 0..Parking::capacity_of(LotType::Commercial).unwrap() {
        sim.citizens.parking.take(&sim.lots, work);
    }
    let id = nth(&sim, 0);
    let plan = plan_trip(&mut sim, id, home, work, false).unwrap();
    assert_eq!(modes(&plan), vec![Mode::Car]);
    assert!(matches!(plan.legs[0], Leg::Car { to, .. } if to == work));
}

#[test]
fn drivers_walk_back_to_their_car_and_take_it_home() {
    let Fixture {
        mut sim,
        home,
        work,
    } = fixture();
    let id = nth(&sim, 0);
    let car_park = add_lot(&mut sim, cell(16, 3), LotType::Industrial).unwrap();
    citizen(&mut sim, id).car_lot_id = car_park;
    citizen(&mut sim, id).state = State::AtWork;
    let plan = plan_trip(&mut sim, id, work, home, true).unwrap();
    assert_eq!(modes(&plan), vec![Mode::Walk, Mode::Car]);
    // Even when home is close enough to walk to, the car comes home.
    let near_home = add_lot(&mut sim, cell(3, 3), LotType::Commercial).unwrap();
    citizen(&mut sim, id).car_lot_id = add_lot(&mut sim, cell(4, 3), LotType::Commercial).unwrap();
    assert_eq!(
        plan_trip(&mut sim, id, near_home, home, true).unwrap().mode,
        Mode::Car
    );
    assert_eq!(
        plan_trip(&mut sim, id, near_home, home, false)
            .unwrap()
            .mode,
        Mode::Walk,
        "but not when it isn't going home"
    );
}

#[test]
fn a_car_left_far_away_cant_be_driven() {
    let Fixture {
        mut sim,
        home,
        work,
    } = fixture();
    let id = nth(&sim, 0);
    citizen(&mut sim, id).car_lot_id = work; // Left at work; they're at home.
    let shop = add_lot(&mut sim, cell(6, 3), LotType::Commercial).unwrap();
    assert_eq!(
        modes(&plan_trip(&mut sim, id, home, shop, false).unwrap()),
        vec![Mode::Walk]
    );
}

#[test]
fn a_removed_car_park_sends_the_car_home() {
    let Fixture { mut sim, home, .. } = fixture();
    let id = nth(&sim, 0);
    let car_park = add_lot(&mut sim, cell(16, 3), LotType::Industrial).unwrap();
    citizen(&mut sim, id).car_lot_id = car_park;
    sim.citizens.parking.take(&sim.lots, car_park);
    sim.call(|sim, out| {
        sim.lots.remove_lot(car_park, out);
    });
    assert_eq!(sim.citizens.citizens[&id].car_lot_id, home);
    assert_eq!(sim.citizens.parking.used(car_park), 0);
}

#[test]
fn flow_scores_whole_trips_and_counts_modes() {
    let Fixture { mut sim, .. } = fixture();
    let id = nth(&sim, 0);
    citizen(&mut sim, id).leave_now = true;
    run_until(&mut sim, id, State::AtWork, 2000);
    let now = sim.sim_time();
    assert_eq!(
        sim.flow.trip_count(now),
        1,
        "the car leg isn't scored on its own"
    );
    let shares = sim.flow.mode_shares(now);
    assert_eq!(shares.len(), 1);
    assert_eq!(shares[&Mode::Car], 1.0);
}
