//! Port of `tests/unit/test_driveways.gd` (Phase 6: cars leave and arrive through
//! driveways, disturbing the traffic going by).

mod common;

use common::roads::{G, local};
use common::traffic::{min_queue_gap, on_road, road, vehicle};
use glam::{IVec2, Vec2};
use tf_sim::Simulation;
use tf_sim::city::LotType;
use tf_sim::ids::{LotId, VehicleId};
use tf_sim::roads::Lane;
use tf_sim::routing::LanePoint;
use tf_sim::traffic::vehicle::{Kind, MAX_LENGTH};
use tf_sim::traffic::vehicle_system::{CROSSING_HALF_WIDTH, DRIVEWAY_SPEED, MIN_GAP, MIN_SPEED};

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// An east-west street along y = 4, open to the east, a dead end to the west.
fn sim() -> Simulation {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(16.0, 4.0), local());
    sim
}

/// Lots on the north side of the street: their near lane is the westbound one.
fn lot(sim: &mut Simulation, x: i32) -> LotId {
    let mut id = None;
    sim.call(|sim, out| {
        id = sim
            .lots
            .add_lot(&sim.roads, IVec2::new(x, 3), LotType::Residential, out)
    });
    id.unwrap()
}

fn westbound(sim: &Simulation) -> Lane {
    let at = sim
        .roads
        .find_lane_position(v(8.0, 4.0) * G + v(0.0, -7.0), 4.0, 0)
        .unwrap();
    sim.roads.lanes[&at.lane_id].clone()
}

fn eastbound(sim: &Simulation) -> Lane {
    let at = sim
        .roads
        .find_lane_position(v(8.0, 4.0) * G + v(0.0, 7.0), 4.0, 0)
        .unwrap();
    sim.roads.lanes[&at.lane_id].clone()
}

/// A car every `every` ticks along a lane, from near its start to near its end.
fn stream(sim: &mut Simulation, lane: &Lane, tick: usize, every: usize, owner: i32) {
    if tick.is_multiple_of(every)
        && sim
            .vehicles
            .can_spawn_at(&sim.roads, lane.id, 10.0, MAX_LENGTH)
    {
        let route = sim
            .router
            .find_route(&sim.roads, lane.id, 10.0, lane.id, lane.length - 5.0, false)
            .unwrap();
        sim.vehicles
            .spawn(&sim.roads, route, None, owner, Kind::Car);
    }
}

fn depart(
    sim: &mut Simulation,
    from: Option<LotId>,
    from_points: Vec<LanePoint>,
    to: Option<LotId>,
    to_points: Vec<LanePoint>,
    owner: i32,
) -> Option<VehicleId> {
    let mut id = None;
    sim.call(|sim, _out| {
        let (vehicles, mut ctx) = sim.traffic();
        id = vehicles.depart(&mut ctx, from, from_points, to, to_points, owner, Kind::Car);
    });
    id
}

fn waiting(sim: &Simulation, id: VehicleId) -> Option<&tf_sim::traffic::vehicle::Vehicle> {
    sim.vehicles
        .waiting_departures()
        .iter()
        .find(|v| v.id == id)
}

#[test]
fn pulling_out_waits_for_a_gap_in_the_traffic() {
    let mut sim = sim();
    let home = lot(&mut sim, 8);
    let shop = lot(&mut sim, 2); // West: a right turn onto the near lane.
    let west = westbound(&sim);
    let mut car = None;
    let mut waited = 0;
    let mut worst_gap = f64::INFINITY;
    for tick in 0..1200 {
        stream(&mut sim, &west, tick, 25, 9);
        if tick == 100 {
            let id = depart(&mut sim, Some(home), vec![], Some(shop), vec![], 1).unwrap();
            assert!(
                waiting(&sim, id).unwrap().crossing_lanes.is_empty(),
                "joins the near lane: nothing to cross"
            );
            car = Some(id);
        }
        if car.is_some_and(|id| waiting(&sim, id).is_some()) {
            waited += 1;
        }
        sim.step();
        worst_gap = worst_gap.min(min_queue_gap(&sim));
    }
    let car = car.unwrap();
    assert!(waited > 0, "had to wait for a gap");
    assert!(waiting(&sim, car).is_none(), "got out in the end");
    assert!(!on_road(&sim, car), "and arrived");
    assert!(worst_gap >= MIN_GAP - 0.01, "never squeezed in too close");
}

#[test]
fn turning_out_across_the_road_waits_for_both_directions() {
    let mut sim = sim();
    let home = lot(&mut sim, 8);
    let east = eastbound(&sim);
    let exit = vec![LanePoint::new(east.id, east.length - 5.0)];
    let west = westbound(&sim);
    let mut car = None;
    for tick in 0..1200 {
        stream(&mut sim, &west, tick, 30, 9);
        if tick == 100 {
            let id = depart(&mut sim, Some(home), vec![], None, exit.clone(), 1).unwrap();
            assert!(
                !waiting(&sim, id).unwrap().crossing_lanes.is_empty(),
                "must cross the westbound lane first"
            );
            car = Some(id);
        }
        let waiting_before = car.is_some_and(|id| waiting(&sim, id).is_some());
        sim.step();
        if waiting_before && on_road(&sim, car.unwrap()) {
            // It just pulled out: nobody was on the crossing point or about to be.
            let center = sim.lots.lots[&home].center();
            let at = sim
                .roads
                .find_lane_position(center, 48.0, 0)
                .unwrap()
                .distance;
            for other_id in sim.vehicles.queue(west.id) {
                let other = &sim.vehicles.vehicles[other_id];
                let clear = (other.distance - at).abs() - other.length * 0.5 - CROSSING_HALF_WIDTH;
                assert!(clear > 0.0, "the crossing point was clear");
            }
        }
    }
    let car = car.unwrap();
    assert!(
        !(on_road(&sim, car) || waiting(&sim, car).is_some()),
        "made it"
    );
}

#[test]
fn turning_in_across_the_road_holds_up_the_traffic_behind() {
    let mut sim = sim();
    let shop = lot(&mut sim, 10); // North side: from the eastbound lane, a left turn in.
    let east = eastbound(&sim);
    let west = westbound(&sim);
    let start = vec![LanePoint::new(east.id, 60.0)];
    let behind_start = vec![LanePoint::new(east.id, 25.0)];
    let beyond = vec![LanePoint::new(east.id, east.length - 5.0)];
    let turning = depart(&mut sim, None, start, Some(shop), vec![], 1).unwrap();
    let mut follower = None;
    let mut waited_to_turn = false;
    let mut crossed = 0;
    let mut follower_stopped_behind = false;
    let mut worst_gap = f64::INFINITY;
    for tick in 0..1500 {
        stream(&mut sim, &west, tick, 18, 9);
        if tick == 10 {
            follower = depart(
                &mut sim,
                None,
                behind_start.clone(),
                None,
                beyond.clone(),
                2,
            );
        }
        sim.step();
        worst_gap = worst_gap.min(min_queue_gap(&sim));
        if on_road(&sim, turning) && vehicle(&sim, turning).driveway_wait_since < f64::INFINITY {
            waited_to_turn = true;
            crossed = vehicle(&sim, turning).crossing_lanes.len();
            if let Some(follower) = follower
                && on_road(&sim, follower)
                && vehicle(&sim, follower).speed < 1.0
            {
                follower_stopped_behind = follower_stopped_behind
                    || vehicle(&sim, follower).distance < vehicle(&sim, turning).distance;
            }
        }
    }
    println!(
        "turning car waited: {waited_to_turn}, follower stopped behind: {follower_stopped_behind}"
    );
    assert_eq!(crossed, 1, "it turns across the westbound lane");
    assert!(waited_to_turn, "waited for a gap in the oncoming traffic");
    assert!(follower_stopped_behind, "the car behind had to stop for it");
    assert!(
        !on_road(&sim, turning),
        "turned in (or went round) in the end"
    );
    assert!(worst_gap >= MIN_GAP - 0.01);
}

#[test]
fn arriving_cars_slow_down_to_turn_in() {
    let mut sim = sim();
    let shop = lot(&mut sim, 10);
    let west = westbound(&sim);
    let car = depart(
        &mut sim,
        None,
        vec![LanePoint::new(west.id, 10.0)],
        Some(shop),
        vec![],
        1,
    )
    .unwrap();
    assert!(
        waiting(&sim, car).unwrap().crossing_lanes.is_empty(),
        "near side: no crossing"
    );
    let (mut last_speed, mut top_speed) = (0.0, 0.0f64);
    for _ in 0..600 {
        sim.step();
        if !on_road(&sim, car) && waiting(&sim, car).is_none() {
            break;
        }
        if on_road(&sim, car) {
            last_speed = vehicle(&sim, car).speed;
            top_speed = top_speed.max(last_speed);
        }
    }
    assert!(top_speed > 60.0, "drove at speed on the way");
    assert!(
        (MIN_SPEED..=DRIVEWAY_SPEED + 4.0).contains(&last_speed),
        "turned in at driveway speed ({last_speed})"
    );
}

/// (GDScript checks this through `ViewSnapshot.capture`, which copies the waiting
/// vehicles' positions; the snapshot is ported with the game shell, M8.1.)
#[test]
fn cars_waiting_in_driveways_are_drawn_on_their_lot() {
    let mut sim = sim();
    let home = lot(&mut sim, 8);
    let shop = lot(&mut sim, 2);
    let car = depart(&mut sim, Some(home), vec![], Some(shop), vec![], 1).unwrap();
    let second = depart(&mut sim, Some(home), vec![], Some(shop), vec![], 2).unwrap();
    assert_eq!(sim.vehicles.waiting_departures().len(), 2);
    let lot_center = sim.lots.lots[&home].center();
    let first_at = waiting(&sim, car).unwrap().position;
    let second_at = waiting(&sim, second).unwrap().position;
    assert!(first_at.y < 4.0 * G - 7.0, "off the road, on the lot side");
    assert!(
        second_at.distance(lot_center) < first_at.distance(lot_center),
        "the second one waits behind the first"
    );
}

#[test]
fn lot_right_at_an_avenue_junction_can_pull_out() {
    // A wide avenue's lanes start well back from the junction, so a corner lot's driveway
    // point lands at the very start of the lane: the car must still get out.
    let mut sim = sim();
    road(
        &mut sim,
        v(8.0, 0.0),
        v(8.0, 8.0),
        common::roads::road_type("avenue_4lane"),
    );
    let mut corner = None;
    sim.call(|sim, out| {
        corner = sim
            .lots
            .add_lot(&sim.roads, IVec2::new(8, 4), LotType::Commercial, out)
    });
    let home = lot(&mut sim, 14);
    let car = depart(&mut sim, corner, vec![], Some(home), vec![], 1).expect("a way there");
    for _ in 0..200 {
        sim.step();
    }
    assert!(waiting(&sim, car).is_none(), "pulled out");
}

#[test]
fn a_trip_to_the_same_lot_needs_no_car() {
    let mut sim = sim();
    let shop = lot(&mut sim, 8);
    assert!(
        depart(&mut sim, Some(shop), vec![], Some(shop), vec![], 1).is_none(),
        "already there"
    );
    assert!(sim.vehicles.waiting_departures().is_empty());
}
