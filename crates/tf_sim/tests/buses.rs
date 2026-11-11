//! Port of `tests/unit/test_buses.gd`: bus stops, bus lines, buses, riders, and the bus in
//! mode choice.

mod common;

use std::collections::{HashMap, HashSet};

use common::roads::{G, road_type};
use common::traffic::{min_queue_gap, road as add_road};
use common::transit::{add_line, add_stop};
use glam::Vec2;
use tf_sim::Simulation;
use tf_sim::city::lot::LotType;
use tf_sim::events::SimEvent;
use tf_sim::ids::{CitizenId, LaneId, LineId, StopId};
use tf_sim::roads::RoadType;
use tf_sim::traffic::vehicle::{BUS_LENGTH, BUS_WIDTH, Kind};
use tf_sim::traffic::vehicle_system::{MID_BLOCK_HALF_WIDTH, MIN_GAP};
use tf_sim::transit::TransitSystem;
use tf_sim::transit::transit_system::RideLeg;
use tf_sim::walking::Crosswalks;

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn road(sim: &mut Simulation, from: Vec2, to: Vec2, road_type: &'static RoadType) {
    add_road(sim, from, to, road_type);
}

fn local() -> &'static RoadType {
    road_type("local_2lane")
}

/// A stop at the curb on the south (eastbound) or north (westbound) side of the road
/// along y = `y`, at x = `x` cells.
fn stop(sim: &mut Simulation, x: f32, y: f32, eastbound: bool) -> StopId {
    let offset = v(0.0, if eastbound { 10.0 } else { -10.0 });
    add_stop(sim, v(x, y) * G + offset, false).expect("test setup: a stop")
}

/// A long street (y 4) with a line along it both ways: east on the south side, back west
/// on the north side, turning round at the dead ends.
fn street_line(sim: &mut Simulation) -> LineId {
    road(sim, v(0.0, 4.0), v(24.0, 4.0), local());
    let mut stops = Vec::new();
    for x in [3.0, 9.0, 15.0, 21.0] {
        stops.push(stop(sim, x, 4.0, true));
    }
    for x in [21.0, 15.0, 9.0, 3.0] {
        stops.push(stop(sim, x, 4.0, false));
    }
    add_line(sim, &stops).expect("test setup: a line")
}

fn step(sim: &mut Simulation, ticks: u32) {
    for _ in 0..ticks {
        sim.step();
    }
}

fn wait_for_bus(
    sim: &mut Simulation,
    citizen: i32,
    line: LineId,
    from: StopId,
    to: StopId,
) -> bool {
    sim.transit.wait_for_bus(CitizenId(citizen), line, from, to)
}

#[test]
fn stops_go_at_the_curb() {
    let mut sim = Simulation::new();
    road(
        &mut sim,
        v(0.0, 4.0),
        v(12.0, 4.0),
        road_type("avenue_4lane"),
    );
    let east = stop(&mut sim, 5.0, 4.0, true);
    let stop_ = &sim.transit.stops[&east];
    let lane = &sim.roads.lanes[&stop_.lane_id.unwrap()];
    assert_eq!(lane.index_from_center, 1, "the outer lane of the avenue");
    assert!(
        (stop_.direction - Vec2::X).abs().max_element() < 0.01,
        "south side: eastbound"
    );
    assert!(stop_.position.y > 4.0 * G + 14.0);
    let next_to = stop_.position + v(8.0, 0.0);
    assert_eq!(
        add_stop(&mut sim, next_to, false),
        Some(east),
        "clicking next to a stop picks it"
    );
    let west = stop(&mut sim, 5.0, 4.0, false);
    assert!(
        (sim.transit.stops[&west].direction + Vec2::X)
            .abs()
            .max_element()
            < 0.01
    );
    assert_eq!(
        add_stop(&mut sim, v(5.0, 9.0) * G, false),
        None,
        "no road there"
    );
}

/// With nobody about, buses drive past every stop round the line; a rider waiting at a
/// stop makes the next bus stop there (holding up the lane while they get on).
#[test]
fn buses_run_round_the_line_stopping_only_where_needed() {
    let mut sim = Simulation::new();
    let line_id = street_line(&mut sim);
    let line = sim.transit.lines[&line_id].clone();
    assert!(line.bus_count > 1, "enough buses for a short headway");
    assert!(line.loop_time() < f64::INFINITY, "every stop reachable");
    let mut headed_for = HashSet::new();
    let mut dwelt = false;
    for _ in 0..Simulation::TICK_RATE * 40 {
        sim.step();
        for bus in sim.transit.buses.values() {
            headed_for.insert(bus.stop_index);
            dwelt = dwelt || sim.vehicles.vehicles[&bus.vehicle].dwelling;
        }
    }
    assert_eq!(sim.transit.buses.len() as i32, line.bus_count);
    assert_eq!(
        headed_for.len(),
        line.stop_ids.len(),
        "buses went round every stop"
    );
    assert!(!dwelt, "nobody waiting, nobody getting off: no stopping");
    let stop_id = line.stop_ids[2];
    assert!(wait_for_bus(
        &mut sim,
        999,
        line_id,
        stop_id,
        line.stop_ids[5]
    ));
    let mut stopped_there = false;
    for _ in 0..Simulation::TICK_RATE * 40 {
        sim.step();
        for bus in sim.transit.buses.values() {
            stopped_there = stopped_there
                || (sim.vehicles.vehicles[&bus.vehicle].dwelling
                    && line.stop_ids[bus.stop_index] == stop_id);
        }
    }
    assert!(stopped_there, "a bus stopped for the rider");
    assert_eq!(sim.transit.stops[&stop_id].waiting.len(), 0, "who got on");
    assert_eq!(sim.vehicles.stuck_count(30.0), 0);
}

#[test]
#[ignore = "needs citizens (M7.1): test_a_citizen_without_a_car_rides_the_bus"]
fn a_citizen_without_a_car_rides_the_bus() {}

#[test]
#[ignore = "needs citizens (M7.1): test_the_bus_when_it_is_quicker_the_car_when_it_is_not"]
fn the_bus_when_it_is_quicker_the_car_when_it_is_not() {}

#[test]
fn riders_change_lines_at_a_shared_stop() {
    let mut sim = Simulation::new();
    // An east-west street and a north-south one crossing it at (12, 4).
    road(&mut sim, v(0.0, 4.0), v(24.0, 4.0), local());
    road(&mut sim, v(12.0, 4.0), v(12.0, 20.0), local());
    let shared = stop(&mut sim, 9.0, 4.0, true);
    let stops = [
        stop(&mut sim, 2.0, 4.0, true),
        shared,
        stop(&mut sim, 9.0, 4.0, false),
        stop(&mut sim, 2.0, 4.0, false),
    ];
    let east_west = add_line(&mut sim, &stops);
    let down = add_stop(&mut sim, v(12.0, 7.0) * G + v(-10.0, 0.0), false).unwrap();
    let bottom = add_stop(&mut sim, v(12.0, 18.0) * G + v(-10.0, 0.0), false).unwrap();
    let up = add_stop(&mut sim, v(12.0, 18.0) * G + v(10.0, 0.0), false).unwrap();
    let north_south = add_line(&mut sim, &[shared, down, bottom, up]);
    assert!(east_west.is_some());
    assert!(north_south.is_some());
    let (transit, mut ctx) = sim.transit_ctx();
    let plan = transit
        .plan_ride(
            &mut ctx,
            v(2.0, 3.5) * G,
            v(11.5, 18.0) * G,
            4.0 * f64::from(G),
            1.5,
            &mut Vec::new(),
        )
        .expect("a plan");
    assert!(matches!(
        plan.legs.as_slice(),
        [
            RideLeg::Walk { .. },
            RideLeg::Bus { .. },
            RideLeg::Bus { .. },
            RideLeg::Walk { .. }
        ]
    ));
    assert_eq!(
        plan.legs[1],
        RideLeg::Bus {
            line: east_west.unwrap(),
            from: plan_from(&plan.legs[1]),
            to: shared
        }
    );
    assert_eq!(
        plan.legs[2],
        RideLeg::Bus {
            line: north_south.unwrap(),
            from: shared,
            to: plan_to(&plan.legs[2])
        }
    );
}

fn plan_from(leg: &RideLeg) -> StopId {
    match leg {
        RideLeg::Bus { from, .. } => *from,
        RideLeg::Walk { .. } => panic!("a bus leg"),
    }
}

fn plan_to(leg: &RideLeg) -> StopId {
    match leg {
        RideLeg::Bus { to, .. } => *to,
        RideLeg::Walk { .. } => panic!("a bus leg"),
    }
}

#[test]
#[ignore = "needs citizens (M7.1): test_removing_a_line_strands_its_riders"]
fn removing_a_line_strands_its_riders() {}

#[test]
fn stops_and_lines_survive_road_edits() {
    let mut sim = Simulation::new();
    let line_id = street_line(&mut sim);
    step(&mut sim, Simulation::TICK_RATE * 5);
    // A crossing street splits the road under two of the stops' lanes...
    road(&mut sim, v(12.0, 0.0), v(12.0, 8.0), local());
    // ...and the west end becomes an avenue: its stops move out to the new curb lane.
    let segment = sim
        .roads
        .find_segment_at(v(3.0, 4.0) * G, 4.0, 0)
        .expect("the west end");
    sim.call(|sim, out| {
        sim.roads
            .set_segment_type(segment, road_type("avenue_4lane"), false, out);
    });
    step(&mut sim, Simulation::TICK_RATE * 30);
    for stop in sim.transit.stops.values() {
        assert!(stop.is_active(), "stop {} still on a road", stop.id);
        let lane = &sim.roads.lanes[&stop.lane_id.unwrap()];
        assert_eq!(
            lane.index_from_center,
            sim.roads.segments[&lane.segment_id.unwrap()]
                .road_type
                .lanes_per_direction
                - 1,
            "at the curb"
        );
    }
    assert!(sim.transit.lines[&line_id].loop_time() < f64::INFINITY);
    assert_eq!(
        sim.transit.buses.len() as i32,
        sim.transit.lines[&line_id].bus_count,
        "buses kept running (or came back)"
    );
    assert_eq!(sim.vehicles.stuck_count(20.0), 0);
}

#[test]
#[ignore = "needs Simulation saves (M7.3): test_stops_and_lines_are_saved"]
fn stops_and_lines_are_saved() {}

/// What the GDScript test checks through `Simulation.from_dict`, at the transit system's
/// level: the saved stops and lines come back on their roads.
#[test]
fn stops_and_lines_round_trip_through_their_save() {
    let mut sim = Simulation::new();
    let line_id = street_line(&mut sim);
    let data: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&sim.transit.to_dict()).unwrap()).unwrap();
    let roads = sim.roads.clone();
    let mut loaded = Simulation::with_roads(roads);
    {
        let (transit, mut ctx) = loaded.transit_ctx();
        transit.restore(&mut ctx, &data);
    }
    assert_eq!(loaded.transit.stops.len(), 8);
    assert_eq!(
        loaded.transit.lines[&line_id].stop_ids,
        sim.transit.lines[&line_id].stop_ids
    );
    assert_eq!(
        loaded.transit.lines[&line_id].bus_count,
        sim.transit.lines[&line_id].bus_count
    );
    for stop in loaded.transit.stops.values() {
        assert!(stop.is_active());
    }
    assert_eq!(
        loaded.crosswalks.mid_block.len(),
        sim.crosswalks.mid_block.len()
    );
}

#[test]
#[ignore = "needs citizens (M7.1): test_no_trip_uses_more_than_three_modes"]
fn no_trip_uses_more_than_three_modes() {}

#[test]
#[ignore = "needs citizens and Scenarios (M7.1, M7.3): test_a_bus_line_speeds_up_trips_without_a_car"]
fn a_bus_line_speeds_up_trips_without_a_car() {}

/// On a grid with many ways round, and traffic to tempt them off it, buses keep to the
/// lanes their line was laid out on (`BusLine::routes`), turnarounds included.
#[test]
fn buses_keep_to_their_line() {
    let mut sim = Simulation::new();
    for y in [2.0, 4.0, 6.0] {
        road(&mut sim, v(2.0, y), v(22.0, y), local());
    }
    for x in [2.0, 8.0, 16.0, 22.0] {
        road(&mut sim, v(x, 2.0), v(x, 6.0), local());
    }
    let stops = [
        stop(&mut sim, 5.0, 4.0, true),
        stop(&mut sim, 12.0, 4.0, true),
        stop(&mut sim, 19.0, 4.0, true),
        stop(&mut sim, 19.0, 4.0, false),
        stop(&mut sim, 12.0, 4.0, false),
        stop(&mut sim, 5.0, 4.0, false),
    ];
    let line_id = add_line(&mut sim, &stops).unwrap();
    let line = sim.transit.lines[&line_id].clone();
    let mut line_lanes: HashSet<LaneId> = HashSet::new();
    for route in line.routes.iter().flatten() {
        line_lanes.extend(route.lane_ids.iter().copied());
    }
    sim.stress.target_count = 80;
    let mut off_line = 0;
    for _ in 0..Simulation::TICK_RATE * 60 {
        sim.step();
        for bus in sim.transit.buses.values() {
            if !line_lanes.contains(&sim.vehicles.vehicles[&bus.vehicle].lane.unwrap()) {
                off_line += 1;
            }
        }
    }
    assert_eq!(sim.transit.buses.len() as i32, line.bus_count);
    assert_eq!(off_line, 0, "never off the line's lanes");
}

/// At a bus bay the bus pulls out of the lane to dwell: the car behind drives past, the
/// bus gets back into the lane (drivers let it in), and nobody overlaps on the way.
#[test]
fn traffic_passes_a_bus_in_a_bay() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(24.0, 4.0), local());
    let mut stops = Vec::new();
    for x in [6.0, 18.0] {
        stops.push(add_stop(&mut sim, v(x, 4.0) * G + v(0.0, 10.0), true).unwrap());
    }
    for x in [18.0, 6.0] {
        stops.push(add_stop(&mut sim, v(x, 4.0) * G + v(0.0, -10.0), true).unwrap());
    }
    let line_id = add_line(&mut sim, &stops).unwrap();
    assert!(sim.transit.stops[&stops[1]].bay);
    // Riders keep the buses stopping.
    for i in 0..30 {
        wait_for_bus(&mut sim, 1000 + i, line_id, stops[1], stops[2]);
    }
    sim.stress.target_count = 40;
    let mut worst_gap = f64::INFINITY;
    let mut passed = false;
    let mut left_bay = false;
    let mut behind: HashMap<i32, f64> = HashMap::new();
    for _ in 0..Simulation::TICK_RATE * 60 {
        sim.step();
        worst_gap = worst_gap.min(min_queue_gap(&sim));
        for bus in sim.transit.buses.values() {
            let vehicle = &sim.vehicles.vehicles[&bus.vehicle];
            let lane = vehicle.lane.unwrap();
            if vehicle.in_bay {
                assert!(
                    !sim.vehicles.queue(lane).contains(&vehicle.id),
                    "out of the lane"
                );
                for other_id in sim.vehicles.queue(lane) {
                    let other = &sim.vehicles.vehicles[other_id];
                    if other.distance < vehicle.distance {
                        behind.insert(other.id.0, vehicle.distance);
                    } else if behind.contains_key(&other.id.0) {
                        passed = true;
                    }
                }
            } else if !behind.is_empty() && !vehicle.dwelling {
                left_bay = true;
            }
        }
    }
    assert!(passed, "a car drove past the bus in the bay");
    assert!(left_bay, "and the bus pulled out again");
    assert!(worst_gap >= MIN_GAP - 0.01, "no overlaps");
    assert_eq!(sim.vehicles.stuck_count(30.0), 0);
}

/// A stop gets a crosswalk across its road (just behind where the bus stands): riders from
/// the other side walk straight over instead of round by the junction, cars give way to
/// them, and nobody is ever on it with a car.
#[test]
fn stops_get_a_crosswalk_that_cars_give_way_at() {
    let mut sim = Simulation::new();
    road(&mut sim, v(0.0, 4.0), v(24.0, 4.0), local());
    let stop_id = stop(&mut sim, 12.0, 4.0, true);
    assert_eq!(sim.crosswalks.mid_block.len(), 1);
    let key = *sim.crosswalks.mid_block.keys().next().unwrap();
    assert!(Crosswalks::is_mid_block(key));
    let crossing = sim.crosswalks.mid_block[&key].clone();
    let stop_position = sim.transit.stops[&stop_id].position;
    assert!(
        crossing.center.x < stop_position.x,
        "behind the eastbound bus"
    );
    // From a door across the street to the stop: over the crosswalk.
    let path = sim
        .pedestrians
        .network
        .find_path(&sim.roads, &sim.crosswalks, v(12.0, 3.5) * G, stop_position)
        .expect("a way across");
    assert!(path.crossings.contains(&key));
    assert!(path.length < 3.0 * f64::from(G), "no detour to a junction");
    // Traffic both ways and people crossing back and forth.
    for (cell, lot_type) in [
        (glam::IVec2::new(11, 3), LotType::Residential),
        (glam::IVec2::new(11, 4), LotType::Commercial),
    ] {
        sim.call(|sim, out| {
            sim.lots.add_lot(&sim.roads, cell, lot_type, out);
        });
    }
    sim.stress.target_count = 30;
    let mut both = 0;
    let mut walked = 0;
    for tick in 0..Simulation::TICK_RATE * 60 {
        if tick % 40 == 0 {
            let (pedestrians, ctx) = sim.walking();
            pedestrians.walk_between(&ctx, v(12.0, 3.5) * G, v(12.0, 4.5) * G, 1, f64::INFINITY);
            pedestrians.walk_between(&ctx, v(12.0, 4.5) * G, v(12.0, 3.5) * G, 1, f64::INFINITY);
        }
        sim.step();
        walked += sim
            .take_view_events()
            .iter()
            .filter(|event| matches!(event, SimEvent::PedestrianArrived(_)))
            .count();
        if sim.crosswalks.has_walkers(key) {
            for &(lane_id, offset) in &crossing.lanes {
                for id in sim.vehicles.queue(lane_id) {
                    let vehicle = &sim.vehicles.vehicles[id];
                    if (vehicle.distance - offset).abs()
                        < vehicle.length * 0.5 + MID_BLOCK_HALF_WIDTH
                    {
                        both += 1;
                    }
                }
            }
        }
    }
    assert_eq!(both, 0, "never a car on it with someone crossing");
    assert!(walked > 40, "people get across ({walked})");
    assert_eq!(sim.vehicles.stuck_count(30.0), 0, "and cars get past");
}

#[test]
#[ignore = "needs citizens (M7.1): test_riders_get_on_and_off_at_the_bus_door"]
fn riders_get_on_and_off_at_the_bus_door() {}

/// Not in the GDScript tests: what `riders_get_on_and_off_at_the_bus_door` checks of
/// `door_position`, without citizens. The door is beside the bus's front, outside it,
/// at the curb side.
#[test]
fn the_door_is_beside_the_front_of_the_bus() {
    let mut sim = Simulation::new();
    let line_id = street_line(&mut sim);
    step(&mut sim, 40);
    let bus = sim.transit.buses.values().next().expect("a bus");
    let vehicle = &sim.vehicles.vehicles[&bus.vehicle];
    assert_eq!(vehicle.kind, Kind::Bus);
    assert!(sim.transit.lines.contains_key(&line_id));
    let door = TransitSystem::door_position(vehicle);
    let distance = f64::from(door.distance(vehicle.position));
    assert!(distance < BUS_LENGTH * 0.5 + BUS_WIDTH);
    assert!(distance > BUS_WIDTH * 0.5);
}
