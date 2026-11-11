//! Port of `tests/unit/test_lots.gd`.

mod common;

use common::roads::{G, RoutedRoads, changes, end_direction, local, road_type};
use glam::{IVec2, Vec2};
use tf_sim::Simulation;
use tf_sim::city::{LotSystem, LotType, lot_system};
use tf_sim::events::{Outbox, SimEvent};
use tf_sim::geom::Rect2;
use tf_sim::grid;
use tf_sim::ids::SegmentId;
use tf_sim::roads::{RoadNetwork, RoadType};

/// A road network with a lot system listening to it (`LotSystem.new(roads)`), and a
/// router for the tests that route.
struct Town {
    net: RoutedRoads,
    lots: LotSystem,
}

impl Town {
    fn new() -> Self {
        Self {
            net: RoutedRoads::new(),
            lots: LotSystem::new(&RoadNetwork::new()),
        }
    }

    fn road(&mut self, from: Vec2, to: Vec2) -> Option<SegmentId> {
        let mut out = Outbox::new();
        let id = self
            .net
            .roads
            .add_road(from * G, to * G, local(), 0, &mut out);
        self.deliver(out);
        id
    }

    fn remove_road(&mut self, id: SegmentId) {
        let mut out = Outbox::new();
        self.net.roads.remove_segment(id, &mut out);
        self.deliver(out);
    }

    fn deliver(&mut self, out: Outbox) {
        for change in changes(&out) {
            self.lots
                .on_roads_changed(&self.net.roads, change, &mut Outbox::new());
        }
        self.net.deliver(out);
    }

    fn add_lot(&mut self, cell: IVec2, lot_type: LotType) -> Option<tf_sim::ids::LotId> {
        self.lots
            .add_lot(&self.net.roads, cell, lot_type, &mut Outbox::new())
    }
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn cell(x: i32, y: i32) -> IVec2 {
    IVec2::new(x, y)
}

#[test]
fn add_and_remove_lot() {
    let mut town = Town::new();
    let mut out = Outbox::new();
    let id = town
        .lots
        .add_lot(&town.net.roads, cell(3, 3), LotType::Residential, &mut out)
        .unwrap();
    assert_eq!(town.lots.lot_at_cell(cell(3, 3)), Some(id));
    assert!(town.lots.remove_lot(id, &mut out));
    assert_eq!(town.lots.lot_at_cell(cell(3, 3)), None);
    assert_eq!(
        out.iter().filter(|e| **e == SimEvent::LotsChanged).count(),
        2
    );
}

#[test]
fn cell_can_only_hold_one_lot() {
    let mut town = Town::new();
    town.add_lot(cell(3, 3), LotType::Residential);
    assert_eq!(town.add_lot(cell(3, 3), LotType::Industrial), None);
}

#[test]
fn cannot_place_outside_map() {
    let mut town = Town::new();
    assert_eq!(town.add_lot(cell(-1, 0), LotType::Residential), None);
    assert_eq!(
        town.add_lot(grid::MAP_SIZE_CELLS, LotType::Residential),
        None
    );
}

#[test]
fn lot_without_road_has_no_access() {
    let mut town = Town::new();
    let id = town.add_lot(cell(3, 3), LotType::Residential).unwrap();
    assert_eq!(town.lots.access(&town.net.roads, id), None);
}

#[test]
fn access_is_the_lane_on_the_lots_side_of_the_road() {
    // Road along y=4 (the bottom edge of cell row 3); the lot sits north of it.
    let mut town = Town::new();
    let segment = town.road(v(0.0, 4.0), v(8.0, 4.0)).unwrap();
    let id = town.add_lot(cell(3, 3), LotType::Residential).unwrap();
    let access = town.lots.access(&town.net.roads, id).unwrap();
    let lane = &town.net.roads.lanes[&access.lane_id];
    assert_eq!(lane.segment_id, Some(segment));
    // North side of an east-west road = the westbound lane (drives on the right).
    assert!(end_direction(lane).x < 0.0);
    let door = lane.curve().sample_baked(access.distance as f32).x;
    assert!((door - 3.5 * G).abs() <= 1.0, "door faces the lot's center");
}

#[test]
fn access_points_cover_both_directions() {
    let mut town = Town::new();
    town.road(v(0.0, 4.0), v(8.0, 4.0));
    let id = town.add_lot(cell(3, 3), LotType::Residential).unwrap();
    let points = town.lots.access_points(&town.net.roads, id);
    assert_eq!(points.len(), 2);
    assert_eq!(
        Some(points[0]),
        town.lots.access(&town.net.roads, id),
        "near lane first"
    );
    let far = &town.net.roads.lanes[&points[1].lane_id];
    assert!(
        end_direction(far).x > 0.0,
        "then the eastbound lane across the road"
    );
    assert!((far.curve().sample_baked(points[1].distance as f32).x - 3.5 * G).abs() <= 1.0);
}

/// A driver may turn out of a driveway either way: heading west from a lot on the north
/// side of the road, then east to a lot further along, they pick the eastbound lane
/// instead of driving to the end of the road and turning around.
#[test]
fn trips_leave_and_arrive_by_the_quicker_side() {
    let mut town = Town::new();
    town.road(v(0.0, 4.0), v(12.0, 4.0));
    let home = town.add_lot(cell(2, 3), LotType::Residential).unwrap();
    let work = town.add_lot(cell(9, 3), LotType::Industrial).unwrap();
    let roads = &town.net.roads;
    let (home_access, work_access) = (
        town.lots.access(roads, home).unwrap(),
        town.lots.access(roads, work).unwrap(),
    );
    let near_only = town
        .net
        .router
        .find_route(
            roads,
            home_access.lane_id,
            home_access.distance,
            work_access.lane_id,
            work_access.distance,
            false,
        )
        .unwrap();
    let (home_points, work_points) = (
        town.lots.access_points(roads, home),
        town.lots.access_points(roads, work),
    );
    let best = town
        .net
        .router
        .find_best_route(roads, &home_points, &work_points, false)
        .unwrap();
    assert_eq!(best.lane_ids.len(), 1, "straight along one lane");
    assert_eq!(best.lane_ids[0], home_points[1].lane_id);
    assert!(
        best.travel_time * 2.0 < near_only.travel_time,
        "much quicker than the U-turn trip"
    );
}

#[test]
fn access_follows_road_changes() {
    let mut town = Town::new();
    let id = town.add_lot(cell(3, 3), LotType::Residential).unwrap();
    let segment = town.road(v(0.0, 4.0), v(8.0, 4.0)).unwrap();
    assert!(
        town.lots.access(&town.net.roads, id).is_some(),
        "road built after the lot"
    );
    town.remove_road(segment);
    assert_eq!(
        town.lots.access(&town.net.roads, id),
        None,
        "road bulldozed"
    );
}

#[test]
fn save_round_trip() {
    let mut town = Town::new();
    town.add_lot(cell(1, 2), LotType::Residential);
    let removed = town.add_lot(cell(5, 5), LotType::Industrial).unwrap();
    town.add_lot(cell(7, 1), LotType::Industrial);
    town.lots.remove_lot(removed, &mut Outbox::new());
    let saved = town.lots.to_dict();
    let roads = RoadNetwork::new();
    let mut loaded = LotSystem::new(&roads);
    loaded.restore(
        &roads,
        &serde_json::from_str(&saved.to_string()).unwrap(),
        &mut Outbox::new(),
    );
    assert_eq!(loaded.to_dict(), saved);
    let id = loaded.lot_at_cell(cell(7, 1)).unwrap();
    assert_eq!(loaded.lots[&id].lot_type, LotType::Industrial);
}

#[test]
fn plots_leave_room_for_the_road_and_sidewalk() {
    let mut town = Town::new();
    town.road(v(0.0, 4.0), v(12.0, 4.0));
    let id = town.add_lot(cell(3, 3), LotType::Commercial).unwrap();
    let plot = town.lots.lots[&id].plot;
    let cell_rect = Rect2::new(v(3.0, 3.0) * G, Vec2::splat(G));
    let taken = local().total_width() * 0.5 + lot_system::ROAD_CLEARANCE;
    assert_eq!(
        plot.position, cell_rect.position,
        "untouched on the sides away from the road"
    );
    assert!(
        (f64::from(plot.end().y) - (f64::from(cell_rect.end().y) - taken)).abs() <= 0.01,
        "the road's side gives way"
    );
    assert_eq!(plot.end().x, cell_rect.end().x);
}

#[test]
fn an_avenue_too_close_removes_the_lots_between() {
    let mut sim = Simulation::new();
    let road = |sim: &mut Simulation, from: Vec2, to: Vec2, road_type: &'static RoadType| {
        sim.call(|sim, out| {
            sim.roads.add_road(from * G, to * G, road_type, 0, out);
        });
    };
    road(&mut sim, v(0.0, 4.0), v(12.0, 4.0), local());
    road(&mut sim, v(0.0, 5.0), v(12.0, 5.0), local());
    let cells = [cell(3, 4), cell(4, 4)];
    sim.call(|sim, out| {
        sim.zones
            .paint(&mut sim.lots, &cells, LotType::Residential, out)
    });
    sim.call(|sim, out| sim.zones.develop_all_now(&sim.roads, &mut sim.lots, out));
    let lot = sim.lots.lot_at_cell(cell(3, 4));
    assert!(lot.is_some(), "room between two local streets");
    assert!(
        f64::from(sim.lots.lots[&lot.unwrap()].plot.size.y) > lot_system::MIN_PLOT,
        "room between two local streets"
    );
    road(
        &mut sim,
        v(0.0, 5.0),
        v(12.0, 5.0),
        road_type("avenue_4lane"),
    );
    assert_eq!(
        sim.lots.lot_at_cell(cell(3, 4)),
        None,
        "the avenue took the land"
    );
    assert_eq!(sim.lots.lot_at_cell(cell(4, 4)), None);
    assert_eq!(
        sim.citizens.citizens.len(),
        0,
        "and its residents moved out"
    );
    assert!(sim.zones.zones.contains_key(&cell(3, 4)), "the zone stays");
    for _ in 0..100 {
        sim.step();
    }
    assert!(
        sim.zones.is_waiting_for_road(cell(3, 4)),
        "waiting for room"
    );
    assert_eq!(sim.lots.lot_at_cell(cell(3, 4)), None);
    let mut placed = None;
    sim.call(|sim, out| {
        placed = sim
            .lots
            .add_lot(&sim.roads, cell(5, 4), LotType::Commercial, out)
    });
    assert_eq!(placed, None, "nor can one be placed there");
}

#[test]
fn a_corner_lots_driveway_is_set_back_from_the_junction() {
    // Crossroads at (4, 4); the lot in the cell north-west of it.
    let mut town = Town::new();
    town.road(v(0.0, 4.0), v(8.0, 4.0));
    town.road(v(4.0, 0.0), v(4.0, 8.0));
    let id = town.add_lot(cell(3, 3), LotType::Commercial).unwrap();
    let points = town.lots.access_points(&town.net.roads, id);
    assert!(!points.is_empty());
    for point in points {
        let lane = &town.net.roads.lanes[&point.lane_id];
        assert!(
            point.distance >= lot_system::DRIVEWAY_SETBACK - 0.01,
            "clear of the junction it starts at"
        );
        assert!(
            point.distance <= lane.length - lot_system::DRIVEWAY_SETBACK + 0.01,
            "clear of the junction it ends at"
        );
    }
}

#[test]
fn a_road_edit_only_refits_lots_near_it() {
    let mut town = Town::new();
    town.road(v(0.0, 4.0), v(10.0, 4.0));
    town.road(v(0.0, 20.0), v(10.0, 20.0)); // Another street, far away.
    let near = town.add_lot(cell(3, 3), LotType::Residential).unwrap();
    let far = town.add_lot(cell(5, 19), LotType::Residential).unwrap();
    let far_access = town.lots.access(&town.net.roads, far);
    // A side street right next to the near lot takes some of its land.
    town.road(v(3.0, 0.0), v(3.0, 4.0));
    assert!(
        f64::from(town.lots.lots[&near].plot.size.x)
            < f64::from(grid::CELL_SIZE) - lot_system::ROAD_CLEARANCE,
        "near lot refitted"
    );
    assert_eq!(
        town.lots.access(&town.net.roads, far),
        far_access,
        "far lot keeps its access"
    );
    let near_access = town.lots.access(&town.net.roads, near).unwrap();
    assert!(town.net.roads.lanes.contains_key(&near_access.lane_id));
}
