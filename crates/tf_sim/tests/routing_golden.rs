//! The router finds Godot's routes: every sampled route in the golden files, lane for
//! lane (equally quick alternatives included: A* breaks ties as Godot's does), with the
//! same travel and free-flow times.

mod common;

use common::golden::{self, AccessPoint, GoldenRoute, Layout};
use tf_sim::ids::LaneId;
use tf_sim::roads::RoadNetwork;
use tf_sim::routing::{LanePoint, Route, Router};

fn check(roads: &RoadNetwork, got: Option<Route>, expected: &Option<GoldenRoute>, what: &str) {
    match (got, expected) {
        (None, None) => {}
        (Some(route), Some(expected)) => {
            let lanes: Vec<i32> = route.lane_ids.iter().map(|id| id.0).collect();
            assert_eq!(lanes, expected.lane_ids, "{what}: lanes");
            assert!(
                (route.start_distance - expected.start_distance).abs() < 1e-9,
                "{what}: start"
            );
            assert!(
                (route.end_distance - expected.end_distance).abs() < 1e-9,
                "{what}: end"
            );
            assert!(
                (route.travel_time - expected.travel_time).abs() < 1e-6,
                "{what}: travel time {} vs Godot {}",
                route.travel_time,
                expected.travel_time
            );
            let free_flow = route.free_flow_time(roads);
            assert!(
                (free_flow - expected.free_flow_time).abs() < 1e-6,
                "{what}: free-flow time"
            );
        }
        (got, _) => panic!(
            "{what}: got {got:?}, Godot found {}",
            if expected.is_some() {
                "a route"
            } else {
                "none"
            }
        ),
    }
}

fn points(access: &[AccessPoint]) -> Vec<LanePoint> {
    access
        .iter()
        .map(|p| LanePoint {
            lane_id: LaneId(p.lane_id),
            distance: p.distance,
            penalty: p.penalty,
        })
        .collect()
}

#[test]
fn every_sampled_route_is_godots() {
    let mut count = 0;
    for name in golden::LAYOUTS {
        let layout: Layout = golden::read(name);
        let roads = RoadNetwork::from_dict(&layout.save["roads"]);
        let mut router = Router::new();
        for (i, sample) in layout.routes.iter().enumerate() {
            let route = router.find_route(
                &roads,
                LaneId(sample.from_lane),
                sample.from_distance,
                LaneId(sample.to_lane),
                sample.to_distance,
                sample.bus,
            );
            check(&roads, route, &sample.route, &format!("{name}, route {i}"));
            count += 1;
        }
        // Lot to lot through both lanes in front of each building (access points from
        // Godot: the lot system is ported later).
        for (i, sample) in layout.lot_routes.iter().enumerate() {
            let access =
                |id: i32| points(&layout.lots.iter().find(|lot| lot.id == id).unwrap().access);
            let route = router.find_best_route(
                &roads,
                &access(sample.from_lot),
                &access(sample.to_lot),
                false,
            );
            check(
                &roads,
                route,
                &sample.route,
                &format!("{name}, lot route {i}"),
            );
            count += 1;
        }
    }
    assert!(count > 500, "{count} routes");
}
