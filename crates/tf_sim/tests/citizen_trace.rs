//! M7.1 review: citizens traced against Godot (`migration/probes/citizen_trace.gd`,
//! `migration/golden/citizen_trace.json`). A small city on zoned land (homes, shops,
//! factories), a highway with trucks and a bus line, living through a day with a road cut
//! and laid again, a shop removed and everyone leaving at once. Every vehicle and
//! pedestrian arrival or failure, every rider, every citizen's state change, trip start
//! and trip end must happen on the same tick as in Godot; and the citizens (schedules,
//! taste, car, job), parking, vehicles and planned trips must end the same.

mod common;

use common::citizens::plan_trip;
use common::golden;
use common::roads::{G, local};
use glam::{IVec2, Vec2};
use serde::Deserialize;
use std::collections::HashMap;
use tf_sim::Simulation;
use tf_sim::city::LotType;
use tf_sim::city::citizen::{Mode, State};
use tf_sim::debug::scenarios;
use tf_sim::events::SimEvent;
use tf_sim::geom::sort::sort_custom;
use tf_sim::grid;
use tf_sim::ids::CitizenId;
use tf_sim::rng::SimRng;

const TICKS: usize = 6000;

type Event = (usize, String, i64, i64, i64, i64, i64);

#[derive(Deserialize)]
struct Golden {
    events: Vec<Event>,
    flow: Vec<serde_json::Value>,
    citizens: Vec<[i64; 13]>,
    parking: Vec<(i32, i32)>,
    counts: [usize; 8],
    vehicles: Vec<(i32, i32, f64, f64)>,
    plans: Vec<(i32, i32, i32, i32, usize, Option<i64>)>,
    sort: Vec<i32>,
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn ms(x: f64) -> i64 {
    (x * 1000.0).round() as i64
}

fn cells(from: IVec2, to: IVec2) -> Vec<IVec2> {
    let mut cells = Vec::new();
    for x in from.x..=to.x {
        for y in from.y..=to.y {
            cells.push(IVec2::new(x, y));
        }
    }
    cells
}

struct Recorder {
    events: Vec<Event>,
    state: HashMap<CitizenId, State>,
    start: HashMap<CitizenId, f64>,
}

impl Recorder {
    /// What the probe's signal handlers record, from the events delivered since the last
    /// call (same order: a listener's own events come before the event itself).
    fn drain(&mut self, sim: &mut Simulation, tick: usize) {
        for event in sim.take_view_events() {
            match event {
                SimEvent::VehicleArrived(id) => {
                    let owner = sim.vehicles.vehicle_any(id).unwrap().owner_id;
                    self.events
                        .push((tick, "a".into(), id.0.into(), owner.into(), 0, 0, 0));
                }
                SimEvent::VehicleFailed(id) => {
                    let v = sim.vehicles.vehicle_any(id).unwrap();
                    let kind = if v.gave_up { "g" } else { "f" };
                    self.events
                        .push((tick, kind.into(), id.0.into(), v.owner_id.into(), 0, 0, 0));
                }
                SimEvent::PedestrianArrived(id) => {
                    let owner = sim.pedestrians.pedestrian_any(id).unwrap().owner_id;
                    self.events
                        .push((tick, "p".into(), id.0.into(), owner.into(), 0, 0, 0));
                }
                SimEvent::RiderArrived { citizen, door } => self.events.push((
                    tick,
                    "r".into(),
                    citizen.0.into(),
                    (f64::from(door.x) * 10.0).round() as i64,
                    (f64::from(door.y) * 10.0).round() as i64,
                    0,
                    0,
                )),
                SimEvent::RiderStranded(citizen) => {
                    self.events
                        .push((tick, "x".into(), citizen.0.into(), 0, 0, 0, 0));
                }
                SimEvent::TripEnded { citizen, completed } => {
                    let c = &sim.citizens.citizens[&citizen];
                    self.events.push((
                        tick,
                        "t".into(),
                        citizen.0.into(),
                        i64::from(completed),
                        i64::from(c.trip_mode as i32),
                        ms(c.trip_ideal_time),
                        ms(c.trip_end_time - c.trip_start_time),
                    ));
                }
                _ => {}
            }
        }
    }

    /// Records a citizen whenever their state changes or a new trip started.
    fn poll(&mut self, sim: &Simulation, tick: usize) {
        for c in sim.citizens.citizens.values() {
            if c.trip_start_time != self.start.get(&c.id).copied().unwrap_or(0.0) {
                self.start.insert(c.id, c.trip_start_time);
                let reference = if c.trip_reference_time.is_infinite() {
                    -1
                } else {
                    ms(c.trip_reference_time)
                };
                self.events.push((
                    tick,
                    "b".into(),
                    c.id.0.into(),
                    i64::from(c.trip_mode as i32),
                    c.trip_lot_id.map_or(-1, |l| l.0.into()),
                    reference,
                    0,
                ));
            }
            if self.state.get(&c.id) != Some(&c.state) {
                self.state.insert(c.id, c.state);
                self.events.push((
                    tick,
                    "c".into(),
                    c.id.0.into(),
                    i64::from(c.state as i32),
                    c.trip_lot_id.map_or(-1, |l| l.0.into()),
                    0,
                    0,
                ));
            }
        }
    }
}

#[test]
fn a_day_of_citizens_matches_godot() {
    let golden: Golden = golden::read("citizen_trace");
    let mut sim = Simulation::new();
    scenarios::add_highway(&mut sim);
    let origin = grid::world_to_cell(sim.externals.points[0]) + IVec2::new(6, 0);
    let at = |x: f32, y: f32| (Vec2::new(origin.x as f32, origin.y as f32) + Vec2::new(x, y)) * G;
    let add_road = |sim: &mut Simulation, a: Vec2, b: Vec2| {
        sim.call(|sim, out| {
            sim.roads.add_road(a, b, local(), 0, out);
        });
    };
    add_road(&mut sim, at(0.0, 0.0), at(16.0, 0.0));
    for x in [0.0, 4.0, 8.0, 12.0, 16.0] {
        add_road(&mut sim, at(x, -3.0), at(x, 3.0));
    }
    add_road(&mut sim, at(0.0, -3.0), at(16.0, -3.0));
    add_road(&mut sim, at(0.0, 3.0), at(16.0, 3.0));
    let zones = [
        (IVec2::new(0, -3), IVec2::new(7, 2), LotType::Residential),
        (IVec2::new(8, -3), IVec2::new(15, -1), LotType::Commercial),
        (IVec2::new(8, 0), IVec2::new(15, 2), LotType::Industrial),
    ];
    for (from, to, lot_type) in zones {
        let area = cells(origin + from, origin + to);
        sim.call(|sim, out| sim.zones.paint(&mut sim.lots, &area, lot_type, out));
    }
    // A bus line along the middle street: east on the south side, back on the north side.
    let mut stops = Vec::new();
    for x in [1.0, 5.0, 9.0, 13.0] {
        stops.push(
            common::transit::add_stop(&mut sim, at(x, 0.0) + Vec2::new(0.0, 10.0), false).unwrap(),
        );
    }
    for x in [13.0, 9.0, 5.0, 1.0] {
        stops.push(
            common::transit::add_stop(&mut sim, at(x, 0.0) + Vec2::new(0.0, -10.0), false).unwrap(),
        );
    }
    common::transit::add_line(&mut sim, &stops).unwrap();

    let mut recorder = Recorder {
        events: Vec::new(),
        state: HashMap::new(),
        start: HashMap::new(),
    };
    recorder.drain(&mut sim, 0);
    for tick in 0..TICKS {
        if tick == 2400 {
            let id = sim.roads.find_segment_at(at(8.0, 1.5), 4.0, 0).unwrap();
            sim.call(|sim, out| {
                sim.roads.remove_segment(id, out);
            });
            recorder.drain(&mut sim, tick);
        }
        if tick == 3000 {
            add_road(&mut sim, at(8.0, 0.0), at(8.0, 3.0));
            recorder.drain(&mut sim, tick);
        }
        if tick == 3300 {
            let shop = sim.lots.lots_of_type(LotType::Commercial)[0].id;
            sim.call(|sim, out| {
                sim.lots.remove_lot(shop, out);
            });
            recorder.drain(&mut sim, tick);
        }
        if tick == 4000 {
            sim.citizens.depart_everyone_now(10.0);
        }
        sim.step();
        recorder.drain(&mut sim, tick);
        recorder.poll(&sim, tick);
    }

    let first_difference = recorder
        .events
        .iter()
        .zip(&golden.events)
        .position(|(a, b)| a != b);
    assert!(
        first_difference.is_none() && recorder.events.len() == golden.events.len(),
        "events differ from Godot at #{first_difference:?} ({} here, {} in Godot): {:?} vs {:?}",
        recorder.events.len(),
        golden.events.len(),
        first_difference.map(|i| &recorder.events[i]),
        first_difference.map(|i| &golden.events[i]),
    );

    // The end state.
    assert_eq!(sim.citizens.citizens.len(), golden.citizens.len());
    for (c, g) in sim.citizens.citizens.values().zip(&golden.citizens) {
        let here: [i64; 13] = [
            c.id.0.into(),
            (c.state as i32).into(),
            c.work_lot_id.map_or(-1, |l| l.0).into(),
            c.car_lot_id.0.into(),
            i64::from(c.has_car),
            c.last_commute_day.into(),
            c.shopping_decided_day.into(),
            i64::from(c.going_shopping),
            ms(c.shop_until),
            ms(c.retry_at),
            (c.leave_home_hour * 1e6).round() as i64,
            (c.leave_work_hour * 1e6).round() as i64,
            (c.car_preference * 1e6).round() as i64,
        ];
        assert_eq!(here, *g, "citizen {}", g[0]);
    }
    let parking: Vec<(i32, i32)> = sim
        .lots
        .lots
        .keys()
        .filter(|&&l| sim.citizens.parking.used(l) > 0)
        .map(|&l| (l.0, sim.citizens.parking.used(l)))
        .collect();
    assert_eq!(parking, golden.parking, "parking");
    let counts = [
        sim.citizens.citizens.len(),
        sim.citizens.job_count() as usize,
        sim.citizens.employed_count(),
        sim.lots.lots.len(),
        sim.vehicles.vehicles.len(),
        sim.pedestrians.pedestrians.len(),
        sim.vehicles.waiting_departures().len(),
        sim.transit.rider_count(),
    ];
    assert_eq!(
        counts, golden.counts,
        "citizens, jobs, employed, lots, vehicles, pedestrians, waiting, riders"
    );
    assert_eq!(sim.vehicles.vehicles.len(), golden.vehicles.len());
    for (v, g) in sim.vehicles.vehicles.values().zip(&golden.vehicles) {
        assert_eq!((v.id.0, v.lane.unwrap().0), (g.0, g.1), "vehicle, lane");
        assert!(
            (round3(v.distance) - g.2).abs() <= 0.002 && (round3(v.speed) - g.3).abs() <= 0.002,
            "vehicle {} at {:?}, Godot {:?}",
            g.0,
            (v.distance, v.speed),
            (g.2, g.3)
        );
    }

    // Planned trips (after the day, so with whatever state the car and the city are in).
    let mut plans = Vec::new();
    let ids: Vec<CitizenId> = sim.citizens.citizens.keys().copied().collect();
    for id in ids {
        let Some(work) = sim.citizens.citizens[&id].work_lot_id else {
            continue;
        };
        let home = sim.citizens.citizens[&id].home_lot_id;
        for has_car in [true, false] {
            for going_home in [false, true] {
                sim.citizens.citizens[&id].has_car = has_car;
                let (from, to) = if going_home {
                    (work, home)
                } else {
                    (home, work)
                };
                let plan = plan_trip(&mut sim, id, from, to, going_home);
                plans.push((
                    id.0,
                    i32::from(has_car),
                    i32::from(going_home),
                    plan.as_ref().map_or(-1, |p| p.mode as i32),
                    plan.as_ref().map_or(0, |p| p.legs.len()),
                    plan.as_ref().map(|p| (p.cost * 1e6).round() as i64),
                ));
            }
        }
    }
    assert_eq!(plans.len(), golden.plans.len());
    for (here, there) in plans.iter().zip(&golden.plans) {
        assert_eq!(
            (here.0, here.1, here.2, here.3, here.4),
            (there.0, there.1, there.2, there.3, there.4),
            "plan"
        );
        match (here.5, there.5) {
            (Some(a), Some(b)) => assert!(
                (a - b).abs() <= 1,
                "plan cost {a} vs Godot {b} for {here:?}"
            ),
            (a, b) => assert_eq!(a, b),
        }
    }

    // `sort_custom` on a list with ties everywhere.
    let mut rng = SimRng::new(5);
    let mut points: Vec<(i32, Vec2)> = (0..60)
        .map(|i| {
            let x = rng.randi() % 6;
            let y = rng.randi() % 6;
            (i, Vec2::new(x as f32, y as f32))
        })
        .collect();
    let centre = Vec2::new(2.5, 2.5);
    sort_custom(&mut points, |a, b| {
        centre.distance_squared(a.1) < centre.distance_squared(b.1)
    });
    let order: Vec<i32> = points.iter().map(|p| p.0).collect();
    assert_eq!(order, golden.sort, "sort_custom orders ties as Godot does");

    let _ = (golden.flow, Mode::Walk);
}
