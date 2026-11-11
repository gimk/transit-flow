//! Port of `tests/unit/test_walking.gd`: sidewalks, crosswalks, pedestrians, and how they
//! meet cars.

mod common;

use common::roads::{G, end_direction, local};
use glam::{IVec2, Vec2};
use tf_sim::Simulation;
use tf_sim::city::lot::LotType;
use tf_sim::events::SimEvent;
use tf_sim::grid;
use tf_sim::ids::{LotId, NodeId, PedestrianId, SegmentId};
use tf_sim::rng::SimRng;
use tf_sim::traffic::traffic_lights::TrafficLights;
use tf_sim::traffic::vehicle::Kind;
use tf_sim::walking::crosswalks::NONE;

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn road(sim: &mut Simulation, from: Vec2, to: Vec2) {
    sim.call(|sim, out| {
        sim.roads.add_road(from * G, to * G, local(), 0, out);
    });
}

/// A four-way junction at (6, 4).
fn crossroads(sim: &mut Simulation) -> NodeId {
    road(sim, v(0.0, 4.0), v(12.0, 4.0));
    road(sim, v(6.0, 0.0), v(6.0, 8.0));
    sim.roads.find_node_at(v(6.0, 4.0) * G, 0).unwrap()
}

fn add_lot(sim: &mut Simulation, cell: IVec2, lot_type: LotType) -> LotId {
    let mut id = None;
    sim.call(|sim, out| id = sim.lots.add_lot(&sim.roads, cell, lot_type, out));
    id.expect("test setup: the lot fits")
}

fn lot_center(cell: IVec2) -> Vec2 {
    grid::cell_center(cell)
}

/// `pedestrians.walk(from, to, owner)` with no length limit.
fn walk(sim: &mut Simulation, from: LotId, to: LotId, owner: i32) -> Option<PedestrianId> {
    let (pedestrians, ctx) = sim.walking();
    pedestrians.walk(&ctx, from, to, owner, f64::INFINITY)
}

/// Crosswalk across the road arm of `junction` that lies in `direction` from it.
fn crosswalk(sim: &Simulation, junction: NodeId, direction: Vec2) -> IVec2 {
    let roads = &sim.roads;
    for segment_id in &roads.nodes[&junction].segment_ids {
        let segment = &roads.segments[segment_id];
        let other = roads.nodes[&segment.other_node_id(junction)].position;
        if (other - roads.nodes[&junction].position)
            .normalize()
            .dot(direction)
            > 0.9
        {
            return IVec2::new(junction.0, segment_id.0);
        }
    }
    NONE
}

/// `walker.on_crosswalk` (a pedestrian who arrived is off every crosswalk).
fn on_crosswalk(sim: &Simulation, id: PedestrianId) -> IVec2 {
    sim.pedestrians
        .pedestrian_any(id)
        .map_or(NONE, |p| p.on_crosswalk)
}

#[test]
fn sidewalks_and_crosswalks_at_a_junction() {
    let mut sim = Simulation::new();
    let junction = crossroads(&mut sim);
    // Builds it.
    sim.pedestrians
        .network
        .find_path(&sim.roads, &sim.crosswalks, Vec2::ZERO, Vec2::ONE);
    let network = &mut sim.pedestrians.network;
    assert_eq!(network.sidewalks.len(), 4 * 2, "two sidewalks per road");
    let crossed: Vec<IVec2> = network.crosswalk_edges.values().copied().collect();
    assert_eq!(
        crossed.len(),
        4,
        "a crosswalk across each road at the junction"
    );
    for crosswalk in &crossed {
        assert_eq!(crosswalk.x, junction.0);
    }
    // Diagonally across the junction: two roads to cross.
    let (from, to) = (lot_center(IVec2::new(4, 3)), lot_center(IVec2::new(8, 4)));
    let path = network
        .find_path(&sim.roads, &sim.crosswalks, from, to)
        .unwrap();
    let crossings = path.crossings.iter().filter(|c| **c != NONE).count();
    assert_eq!(crossings, 2);
    assert!(
        path.length < f64::from(from.distance(to)) * 1.6,
        "no big detour"
    );
    // Along the same side of a street: nothing to cross.
    let along = network
        .find_path(
            &sim.roads,
            &sim.crosswalks,
            lot_center(IVec2::new(1, 3)),
            lot_center(IVec2::new(4, 3)),
        )
        .unwrap();
    assert!(along.crossings.iter().all(|c| *c == NONE));
}

#[test]
fn cars_give_way_to_people_crossing() {
    let mut sim = Simulation::new();
    let junction = crossroads(&mut sim);
    let west_arm = crosswalk(&sim, junction, Vec2::NEG_X);
    // Walking across the western arm (north side to south side, west of the junction)...
    let home = add_lot(&mut sim, IVec2::new(5, 3), LotType::Residential);
    let shop = add_lot(&mut sim, IVec2::new(5, 4), LotType::Commercial);
    let walker = walk(&mut sim, home, shop, 1).expect("a walk");
    assert!(
        sim.pedestrians.pedestrians[&walker]
            .crossings
            .contains(&west_arm),
        "test setup: crosses the western arm"
    );
    // ...while a car comes from the west to go straight on: its turn crosses that crosswalk.
    let from = sim
        .roads
        .find_lane_position((v(3.5, 4.0) + v(0.0, 7.0) / G) * G, 4.0, 0)
        .unwrap();
    let to = sim
        .roads
        .find_lane_position((v(11.0, 4.0) + v(0.0, 7.0) / G) * G, 4.0, 0)
        .unwrap();
    let route = sim
        .router
        .find_route(
            &sim.roads,
            from.lane_id,
            from.distance,
            to.lane_id,
            to.distance,
            false,
        )
        .unwrap();
    let car = sim
        .vehicles
        .spawn(&sim.roads, route, None, 2, Kind::Car)
        .unwrap();
    let mut car_waited = false;
    let mut both = 0;
    for _ in 0..600 {
        sim.step();
        if let Some(vehicle) = sim.vehicles.vehicles.get(&car) {
            if vehicle.requested_connector.is_some() && sim.crosswalks.has_walkers(west_arm) {
                car_waited = true;
            }
            if sim
                .roads
                .lane_any(vehicle.lane.unwrap())
                .unwrap()
                .is_connector()
                && sim.crosswalks.has_walkers(west_arm)
            {
                both += 1;
            }
        }
    }
    assert_eq!(both, 0, "never on the crossing together");
    assert!(car_waited, "the car waited for the pedestrian");
    assert!(
        !sim.pedestrians.pedestrians.contains_key(&walker),
        "walked all the way"
    );
    assert!(!sim.vehicles.vehicles.contains_key(&car), "drove on after");
}

#[test]
fn at_lights_people_cross_on_the_walk_light() {
    use tf_sim::roads::JunctionControl;
    let mut sim = Simulation::new();
    let junction = crossroads(&mut sim);
    sim.call(|sim, out| {
        sim.roads
            .set_node_control(junction, JunctionControl::TrafficLight, out);
    });
    let home = add_lot(&mut sim, IVec2::new(5, 3), LotType::Residential);
    let shop = add_lot(&mut sim, IVec2::new(5, 4), LotType::Commercial);
    let west_arm = crosswalk(&sim, junction, Vec2::NEG_X);
    let walker = walk(&mut sim, home, shop, 1).expect("a walk");
    let mut stepped_out_on_red = false;
    let mut waited = false;
    for _ in 0..1200 {
        let was_on = on_crosswalk(&sim, walker) == west_arm;
        sim.step();
        if !was_on && on_crosswalk(&sim, walker) == west_arm {
            let node = &sim.roads.nodes[&junction];
            stepped_out_on_red = stepped_out_on_red
                || !sim
                    .signals
                    .is_walk_green(&sim.roads, node, SegmentId(west_arm.y));
        }
        waited = waited || sim.crosswalks.has_waiting(west_arm);
    }
    let _ = waited;
    assert!(
        !stepped_out_on_red,
        "never stepped out unless the walk light was on"
    );
    assert!(
        !sim.pedestrians.pedestrians.contains_key(&walker),
        "the walk light came (pedestrians call it)"
    );
    // Crossing the western arm walks alongside north-south traffic.
    let node = &sim.roads.nodes[&junction];
    let phase = TrafficLights::walk_phase(&sim.roads, node, SegmentId(west_arm.y));
    let mut north_lane = None;
    for lane in sim.roads.lanes.values() {
        if !lane.is_connector() && lane.to_node_id == junction && end_direction(lane).y.abs() > 0.9
        {
            north_lane = Some(lane);
        }
    }
    assert_eq!(phase, Some(north_lane.unwrap().signal_phase));
}

/// Random traffic and random walks around a grid: nobody is ever on a crosswalk while a
/// car is on a turn over it.
#[test]
fn people_and_cars_never_share_a_crosswalk() {
    let mut sim = Simulation::new();
    for i in 0..4 {
        let d = (i * 3) as f32;
        road(&mut sim, v(1.0, 1.0 + d), v(10.0, 1.0 + d));
        road(&mut sim, v(1.0 + d, 1.0), v(1.0 + d, 10.0));
    }
    let mut lots: Vec<LotId> = Vec::new();
    for x in 1..10 {
        for y in [0, 3, 6] {
            if x % 3 != 1 {
                lots.push(add_lot(&mut sim, IVec2::new(x, y), LotType::Residential));
            }
        }
    }
    let mut rng = SimRng::new(5);
    sim.stress.target_count = 120;
    let mut violations = 0;
    let mut walked = 0;
    for tick in 0..(Simulation::TICK_RATE * 60) {
        if tick % 5 == 0 {
            let from = lots[(rng.randi() % lots.len() as i64) as usize];
            let to = lots[(rng.randi() % lots.len() as i64) as usize];
            walk(&mut sim, from, to, 1);
        }
        sim.step();
        for event in sim.take_view_events() {
            if matches!(event, SimEvent::PedestrianArrived(_)) {
                walked += 1;
            }
        }
        for vehicle in sim.vehicles.vehicles.values() {
            let lane = sim.roads.lane_any(vehicle.lane.unwrap()).unwrap();
            if !lane.is_connector() {
                continue;
            }
            let feeder = &sim.roads.lanes[&lane.feeder_lane_id.unwrap()];
            let exit = &sim.roads.lanes[&lane.outgoing_lane_ids[0]];
            for segment_id in [feeder.segment_id.unwrap(), exit.segment_id.unwrap()] {
                if sim
                    .crosswalks
                    .has_walkers(IVec2::new(lane.from_node_id.0, segment_id.0))
                {
                    violations += 1;
                }
            }
        }
    }
    println!(
        "walks finished: {walked}, pedestrians now {}, vehicles {}",
        sim.pedestrians.pedestrians.len(),
        sim.vehicles.vehicles.len()
    );
    assert_eq!(violations, 0);
    assert!(walked > 50, "people get where they're going ({walked})");
    assert_eq!(
        sim.vehicles.stuck_count(30.0),
        0,
        "and cars aren't stopped for good"
    );
}

#[test]
#[ignore = "needs citizens (M7.1): test_citizens_walk_short_trips"]
fn citizens_walk_short_trips() {}
