//! Port of `tests/unit/test_zones.gd` (Phase 3a: zoning and buildings developing on
//! zoned land), plus external connections and a save check against the golden layouts.

mod common;

use common::golden::{self, Layout, assert_json_matches};
use common::roads::{G, local};
use glam::{IVec2, Vec2};
use tf_sim::Simulation;
use tf_sim::city::zone_system::{DEVELOP_MAX_DELAY, MAX_DEVELOPMENTS_PER_TICK};
use tf_sim::city::{ExternalConnections, LotSystem, LotType, ZoneSystem};
use tf_sim::events::Outbox;
use tf_sim::roads::RoadNetwork;

/// A simulation with a street along y = 4 (`before_each`).
fn sim() -> Simulation {
    let mut sim = Simulation::new();
    road(&mut sim, Vec2::new(0.0, 4.0), Vec2::new(10.0, 4.0));
    sim
}

fn road(sim: &mut Simulation, from: Vec2, to: Vec2) {
    sim.call(|sim, out| {
        sim.roads.add_road(from * G, to * G, local(), 0, out);
    });
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

fn paint(sim: &mut Simulation, cells: &[IVec2], zone_type: LotType) {
    sim.call(|sim, out| sim.zones.paint(&mut sim.lots, cells, zone_type, out));
}

fn step_zones(sim: &mut Simulation, time: f64) {
    sim.call(|sim, out| sim.zones.step(time, &sim.roads, &mut sim.lots, out));
}

/// Advances the zone clock well past every pending development (in several steps, as
/// developments per tick are capped).
fn develop_all(sim: &mut Simulation, clock: &mut f64) {
    for _ in 0..20 {
        *clock += 100.0;
        step_zones(sim, *clock);
    }
}

#[test]
fn zoned_cells_next_to_a_road_develop_after_a_delay() {
    let mut sim = sim();
    paint(
        &mut sim,
        &cells(IVec2::new(1, 3), IVec2::new(4, 3)),
        LotType::Commercial,
    );
    step_zones(&mut sim, 0.0);
    assert_eq!(sim.lots.lots.len(), 0, "not instantly");
    step_zones(&mut sim, DEVELOP_MAX_DELAY + 0.1);
    assert_eq!(sim.lots.lots.len(), 4);
    for lot in sim.lots.lots.values() {
        assert_eq!(lot.lot_type, LotType::Commercial);
    }
}

#[test]
fn cells_without_road_wait_until_a_road_arrives() {
    let mut sim = sim();
    let mut clock = 0.0;
    paint(&mut sim, &[IVec2::new(1, 8)], LotType::Residential);
    develop_all(&mut sim, &mut clock);
    assert_eq!(sim.lots.lots.len(), 0);
    assert!(sim.zones.is_waiting_for_road(IVec2::new(1, 8)));
    road(&mut sim, Vec2::new(0.0, 9.0), Vec2::new(4.0, 9.0));
    develop_all(&mut sim, &mut clock);
    assert_eq!(sim.lots.lots.len(), 1);
    assert!(!sim.zones.is_waiting_for_road(IVec2::new(1, 8)));
}

#[test]
fn rezoning_replaces_and_dezoning_removes_the_building() {
    let mut sim = sim();
    let mut clock = 0.0;
    let cell = [IVec2::new(2, 3)];
    paint(&mut sim, &cell, LotType::Residential);
    develop_all(&mut sim, &mut clock);
    let first_lot = sim.lots.lot_at_cell(cell[0]);
    paint(&mut sim, &cell, LotType::Industrial);
    assert_eq!(
        sim.lots.lot_at_cell(cell[0]),
        None,
        "old building goes at once"
    );
    develop_all(&mut sim, &mut clock);
    let second_lot = sim.lots.lot_at_cell(cell[0]);
    assert_ne!(second_lot, first_lot);
    assert_eq!(
        sim.lots.lots[&second_lot.unwrap()].lot_type,
        LotType::Industrial
    );
    sim.call(|sim, out| sim.zones.erase(&mut sim.lots, &cell, out));
    assert_eq!(sim.lots.lots.len(), 0);
    assert!(!sim.zones.zones.contains_key(&cell[0]));
}

#[test]
fn hand_placed_lots_are_left_alone() {
    let mut sim = sim();
    let mut lot_id = None;
    sim.call(|sim, out| {
        lot_id = sim
            .lots
            .add_lot(&sim.roads, IVec2::new(2, 3), LotType::Residential, out)
    });
    let cell = [IVec2::new(2, 3)];
    paint(&mut sim, &cell, LotType::Industrial);
    assert!(sim.lots.lots.contains_key(&lot_id.unwrap()));
    assert!(!sim.zones.zones.contains_key(&cell[0]));
}

#[test]
fn developments_per_tick_are_capped() {
    let mut sim = sim();
    paint(
        &mut sim,
        &cells(IVec2::new(0, 3), IVec2::new(9, 3)),
        LotType::Residential,
    );
    step_zones(&mut sim, 1000.0);
    assert_eq!(sim.lots.lots.len(), MAX_DEVELOPMENTS_PER_TICK);
}

#[test]
fn save_round_trip_keeps_zones_and_their_buildings() {
    let mut sim = sim();
    paint(
        &mut sim,
        &cells(IVec2::new(1, 3), IVec2::new(3, 3)),
        LotType::Residential,
    );
    let mut clock = 0.0;
    develop_all(&mut sim, &mut clock);
    // Not developed yet.
    paint(
        &mut sim,
        &cells(IVec2::new(6, 3), IVec2::new(6, 3)),
        LotType::Industrial,
    );
    let saved = sim.to_dict();
    let mut loaded = Simulation::from_dict(&serde_json::from_str(&saved.to_string()).unwrap())
        .expect("the save loads");
    assert_eq!(loaded.zones.zones, sim.zones.zones);
    assert_eq!(loaded.lots.lots.len(), 3);
    assert!(loaded.zones.is_developed(IVec2::new(1, 3)));
    for i in 0..20 {
        step_zones(&mut loaded, 100.0 * f64::from(i + 1));
    }
    assert_eq!(
        loaded.lots.lots.len(),
        4,
        "the undeveloped cell develops after loading"
    );
}

/// The golden layouts' zones and external connections restore and save back unchanged.
#[test]
fn golden_zones_and_connections_survive_a_save() {
    for name in golden::LAYOUTS {
        let layout: Layout = golden::read(name);
        let roads = RoadNetwork::from_dict(&layout.save["roads"]);
        let mut lots = LotSystem::new(&roads);
        let mut out = Outbox::new();
        lots.restore(&roads, &layout.save["lots"], &mut out);
        let mut zones = ZoneSystem::new();
        zones.restore(&lots, &layout.save["zones"], &mut out);
        assert_json_matches(
            &zones.to_dict(),
            &layout.save["zones"],
            &format!("{name} zones"),
        );
        let mut externals = ExternalConnections::new();
        externals.restore(&layout.save["externals"], &mut out);
        assert_json_matches(
            &externals.to_dict(),
            &layout.save["externals"],
            &format!("{name} externals"),
        );
    }
}

#[test]
fn external_connections_need_a_road_ending_on_their_point() {
    let mut sim = sim();
    let edge = Vec2::new(0.0, 4.0) * G;
    sim.call(|sim, out| sim.externals.add(edge, out));
    sim.call(|sim, out| sim.externals.add(edge, out)); // Once only.
    sim.call(|sim, out| sim.externals.add(Vec2::new(0.0, 9.0) * G, out));
    assert_eq!(sim.externals.points.len(), 2);
    assert_eq!(sim.externals.active_points(&sim.roads), vec![edge]);
    let inbound = sim.externals.inbound(&sim.roads, edge).unwrap();
    let outbound = sim.externals.outbound(&sim.roads, edge).unwrap();
    let lanes = &sim.roads.lanes;
    assert_eq!(
        lanes[&inbound.lane_id].from_node_id,
        sim.roads.find_node_at(edge, 0).unwrap()
    );
    assert_eq!(
        lanes[&outbound.lane_id].to_node_id,
        sim.roads.find_node_at(edge, 0).unwrap()
    );
    assert_eq!(inbound.distance, 17.0);
    assert_eq!(outbound.distance, lanes[&outbound.lane_id].length - 1.0);
    assert!(
        sim.externals
            .inbound(&sim.roads, Vec2::new(0.0, 9.0) * G)
            .is_none()
    );
}
