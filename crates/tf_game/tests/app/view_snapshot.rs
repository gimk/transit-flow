//! Port of `tests/unit/test_view_snapshot.gd`: what the views get from each tick:
//! lateness for the lateness colours, parked cars, indicators.

use glam::{IVec2, Vec2};
use tf_game::snapshot::{
    CAR_COLOR, LAMP_LEFT, LAMP_RIGHT, ViewSnapshot, car_color, color_for_vehicle, lerp_angle,
};
use tf_sim::Simulation;
use tf_sim::city::LotType;
use tf_sim::ids::{LotId, VehicleId};
use tf_sim::roads::RoadType;
use tf_sim::traffic::vehicle::Kind;
use tf_sim::traffic::vehicle_system::{IMPATIENT_FACTOR, IMPATIENT_GRACE};

const G: f32 = 64.0;

fn local() -> &'static RoadType {
    RoadType::load_by_id("local_2lane").unwrap()
}

fn road(sim: &mut Simulation, from: Vec2, to: Vec2) {
    sim.call(|sim, out| {
        sim.roads.add_road(from * G, to * G, local(), 0, out);
    });
}

fn setup() -> Simulation {
    let mut sim = Simulation::new();
    road(&mut sim, Vec2::new(0.0, 4.0), Vec2::new(12.0, 4.0));
    sim
}

fn add_lot(sim: &mut Simulation, cell: IVec2, lot_type: LotType) -> LotId {
    let mut id = None;
    sim.call(|sim, out| id = sim.lots.add_lot(&sim.roads, cell, lot_type, out));
    id.expect("lot placed")
}

/// Spawns a car between two points that lie on lanes (in grid units).
fn spawn(sim: &mut Simulation, from: Vec2, to: Vec2) -> VehicleId {
    let a = sim.roads.find_lane_position(from * G, 4.0, 0).unwrap();
    let b = sim.roads.find_lane_position(to * G, 4.0, 0).unwrap();
    let route = sim
        .router
        .find_route(
            &sim.roads, a.lane_id, a.distance, b.lane_id, b.distance, false,
        )
        .unwrap();
    sim.vehicles
        .spawn(&sim.roads, route, None, 1, Kind::Car)
        .expect("spawned")
}

#[test]
fn cars_are_parked_at_home_until_driven() {
    let mut sim = setup();
    let home = add_lot(&mut sim, IVec2::new(2, 3), LotType::Residential);
    add_lot(&mut sim, IVec2::new(10, 3), LotType::Industrial);
    sim.step();
    let owners = sim.citizens.citizens.values().filter(|c| c.has_car).count() as i32;
    assert_eq!(
        ViewSnapshot::capture(&mut sim)
            .parked
            .get(&home)
            .copied()
            .unwrap_or(0),
        owners
    );
    for citizen in sim.citizens.citizens.values_mut() {
        citizen.has_car = true;
        citizen.car_preference = f64::from(tf_sim::city::citizen_system::CAR_PREFERENCE_RANGE.0);
    }
    sim.citizens.depart_everyone_now(0.0);
    sim.step();
    assert_eq!(
        ViewSnapshot::capture(&mut sim)
            .parked
            .get(&home)
            .copied()
            .unwrap_or(0),
        0,
        "both cars out"
    );
}

#[test]
fn lateness_grows_from_on_time_to_impatient() {
    let mut sim = setup();
    let car = spawn(
        &mut sim,
        Vec2::new(1.0, 4.0) + Vec2::new(0.0, 7.0 / G),
        Vec2::new(11.0, 4.0) + Vec2::new(0.0, 7.0 / G),
    );
    sim.step();
    let snapshot = ViewSnapshot::capture(&mut sim);
    assert_eq!(
        snapshot.lateness[snapshot.vehicle_index[&car]], 0.0,
        "on time"
    );
    {
        let vehicle = sim.vehicles.vehicles.get_mut(&car).unwrap();
        vehicle.trip_start_time -= vehicle.ideal_trip_time * IMPATIENT_FACTOR + IMPATIENT_GRACE;
    }
    let snapshot = ViewSnapshot::capture(&mut sim);
    assert_eq!(
        snapshot.lateness[snapshot.vehicle_index[&car]], 1.0,
        "as late as an impatient driver"
    );
    let color = snapshot.colors[snapshot.vehicle_index[&car]];
    assert_eq!(
        color,
        color_for_vehicle(&sim.vehicles.vehicles[&car]),
        "normal colours stay"
    );
    // A shade of green.
    assert!((hue(color) - hue(CAR_COLOR)).abs() < 0.05);
}

#[test]
fn drivers_signal_before_turning() {
    let mut sim = setup();
    road(&mut sim, Vec2::new(6.0, 0.0), Vec2::new(6.0, 8.0));
    let start = sim
        .roads
        .find_lane_position(Vec2::new(6.0 * G - 40.0, 4.0 * G + 7.0), 4.0, 0)
        .unwrap();
    // Eastbound: north is a left turn, south a right turn.
    let targets = [
        (LAMP_LEFT, Vec2::new(6.0 * G + 7.0, G)),
        (LAMP_RIGHT, Vec2::new(6.0 * G - 7.0, 7.0 * G)),
        (0, Vec2::new(11.0 * G, 4.0 * G + 7.0)),
    ];
    for (lamp, target) in targets {
        let to = sim.roads.find_lane_position(target, 4.0, 0).unwrap();
        let route = sim
            .router
            .find_route(
                &sim.roads,
                start.lane_id,
                start.distance,
                to.lane_id,
                to.distance,
                false,
            )
            .unwrap();
        let car = sim
            .vehicles
            .spawn(&sim.roads, route, None, 1, Kind::Car)
            .expect("spawned");
        let snapshot = ViewSnapshot::capture(&mut sim);
        assert_eq!(
            snapshot.indicators[snapshot.vehicle_index[&car]], lamp,
            "indicator for lamp {lamp}"
        );
        sim.vehicles.remove(&sim.roads, car);
    }
}

fn hue(color: [f32; 4]) -> f32 {
    let [r, g, b, _] = color;
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    if delta == 0.0 {
        0.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    }
}

#[test]
fn car_colours_vary_but_stay_green() {
    let shades: Vec<_> = (1..40).map(car_color).collect();
    assert!(shades.iter().any(|c| *c != shades[0]), "not all alike");
    for color in shades {
        assert!((hue(color) - hue(CAR_COLOR)).abs() < 0.05, "{color:?}");
    }
}

#[test]
fn headings_interpolate_the_short_way_round() {
    use std::f32::consts::PI;
    let angle = lerp_angle(PI - 0.1, -PI + 0.1, 0.5);
    assert!((angle.abs() - PI).abs() < 1e-4, "{angle}");
    assert!((lerp_angle(0.0, 1.0, 0.25) - 0.25).abs() < 1e-6);
}

#[test]
fn a_vehicle_is_interpolated_between_two_snapshots() {
    let mut sim = setup();
    let car = spawn(
        &mut sim,
        Vec2::new(1.0, 4.0) + Vec2::new(0.0, 7.0 / G),
        Vec2::new(11.0, 4.0) + Vec2::new(0.0, 7.0 / G),
    );
    sim.step();
    let from = ViewSnapshot::capture(&mut sim);
    sim.step();
    sim.step();
    let to = ViewSnapshot::capture(&mut sim);
    let (a, _) = ViewSnapshot::vehicle_pose(&from, &to, car, 0.0).unwrap();
    let (b, _) = ViewSnapshot::vehicle_pose(&from, &to, car, 1.0).unwrap();
    let (mid, _) = ViewSnapshot::vehicle_pose(&from, &to, car, 0.5).unwrap();
    assert!(b.x > a.x, "it moves east");
    assert!((mid - (a + b) / 2.0).length() < 1e-3);
}
