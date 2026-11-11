//! Port of `tests/unit/test_curves.gd` (Phase 9, curved roads): the road network parts.
//! The tests that drive traffic or place lots come with the work packages that port
//! those systems.

mod common;

use common::roads::{G, local};
use glam::Vec2;
use tf_sim::events::Outbox;
use tf_sim::ids::SegmentId;
use tf_sim::roads::{RoadNetwork, RoadType};

/// A quarter turn from (x, y) east then south, bending at (x + r, y), ending at
/// (x + r, y + r).
fn quarter_turn(
    roads: &mut RoadNetwork,
    x: f32,
    y: f32,
    r: f32,
    road_type: &'static RoadType,
) -> Vec<SegmentId> {
    roads.add_curved_road(
        Vec2::new(x, y) * G,
        Vec2::new(x + r, y) * G,
        Vec2::new(x + r, y + r) * G,
        road_type,
        0,
        &mut Outbox::new(),
    )
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

#[test]
fn tight_curves_are_refused() {
    let roads = RoadNetwork::new();
    let tight = RoadNetwork::curve_path(v(2.0, 2.0) * G, v(3.0, 2.0) * G, v(3.0, 3.0) * G);
    assert!(
        RoadNetwork::min_radius(&tight) < RoadNetwork::MIN_CURVE_RADIUS,
        "a quarter turn in one cell"
    );
    assert_eq!(
        roads.plan_path(&tight, 0, Some(local())).error,
        "Curve too tight (drivers need a wider turn)"
    );
    let wide = RoadNetwork::curve_path(v(2.0, 2.0) * G, v(4.0, 2.0) * G, v(4.0, 4.0) * G);
    assert!(
        RoadNetwork::min_radius(&wide) > RoadNetwork::MIN_CURVE_RADIUS,
        "in two cells"
    );
    assert_eq!(roads.plan_path(&wide, 0, Some(local())).error, "");
}

#[test]
fn a_curved_road_has_curved_lanes_driven_slower() {
    let mut roads = RoadNetwork::new();
    let ids = quarter_turn(&mut roads, 2.0, 2.0, 2.0, local()); // Radius ~90 px: slower than 90 px/s.
    assert_eq!(ids.len(), 1);
    let segment = &roads.segments[&ids[0]];
    assert!(
        segment.centerline().point_count() > 10,
        "sampled along the curve"
    );
    let quarter = std::f64::consts::PI / 2.0 * 2.0 * f64::from(G);
    assert!(
        (segment.length() - quarter).abs() <= 0.15 * 2.0 * f64::from(G),
        "about a quarter circle"
    );
    for lane_id in &segment.lane_ids {
        let lane = &roads.lanes[lane_id];
        assert!(
            lane.speed_limit < local().speed_limit,
            "slower round the bend"
        );
        assert!(lane.speed_limit > 50.0);
    }
    // The inside lane is tighter, so slower.
    let limits: Vec<f64> = segment
        .lane_ids
        .iter()
        .map(|id| roads.lanes[id].speed_limit)
        .collect();
    assert_ne!(limits[0], limits[1]);
}

#[test]
fn roads_cross_curves_and_curves_cross_roads() {
    let mut roads = RoadNetwork::new();
    // A straight road built across a curve splits it at a junction...
    quarter_turn(&mut roads, 2.0, 2.0, 4.0, local());
    let across = roads.add_road_pieces(
        v(0.0, 3.0) * G,
        v(10.0, 3.0) * G,
        local(),
        0,
        &mut Outbox::new(),
    );
    assert_eq!(across.len(), 2, "split where it meets the curve");
    let end = roads.nodes[&roads.segments[&across[0]].end_node_id].position;
    let junction = roads.find_node_at(end, 0).unwrap();
    assert_eq!(roads.nodes[&junction].degree(), 4);
    for segment_id in &roads.nodes[&junction].segment_ids {
        assert!(roads.segments[segment_id].length() > 16.0);
    }
    // ...and a curve built across straight roads is split at each of them.
    let curve = roads.add_curved_road(
        v(0.0, 8.0) * G,
        v(8.0, 8.0) * G,
        v(8.0, 14.0) * G,
        local(),
        0,
        &mut Outbox::new(),
    );
    roads.add_road(
        v(4.0, 6.0) * G,
        v(4.0, 12.0) * G,
        local(),
        0,
        &mut Outbox::new(),
    );
    let count_before = roads.segments.len();
    assert!(!curve.is_empty());
    assert_eq!(roads.segments.len(), count_before);
    let crossings = roads
        .nodes
        .values()
        .filter(|node| node.degree() == 4)
        .count();
    assert_eq!(crossings, 2);
}

#[test]
fn curves_survive_a_save() {
    let mut roads = RoadNetwork::new();
    quarter_turn(&mut roads, 2.0, 2.0, 3.0, local());
    let loaded =
        RoadNetwork::from_dict(&serde_json::from_str(&roads.to_dict().to_string()).unwrap());
    let segment = &loaded.segments[0];
    let original = &roads.segments[0];
    assert_eq!(
        segment.centerline().point_count(),
        original.centerline().point_count()
    );
    assert!((segment.length() - original.length()).abs() <= 0.01);
}

#[test]
#[ignore = "needs vehicles (M4.2): test_traffic_drives_round_curves"]
fn traffic_drives_round_curves() {
    unimplemented!("port with M4.2");
}

#[test]
fn lots_keep_clear_of_curves() {
    use tf_sim::city::LotSystem;
    use tf_sim::grid;
    use tf_sim::roads::lane_builder;
    let mut roads = RoadNetwork::new();
    quarter_turn(&mut roads, 2.0, 2.0, 3.0, local());
    let path = RoadNetwork::curve_path(v(2.0, 2.0) * G, v(5.0, 2.0) * G, v(5.0, 5.0) * G);
    let full = (grid::CELL_SIZE * grid::CELL_SIZE) as f32;
    let (mut under, mut beside) = (0, 0);
    for x in 1..7 {
        for y in 1..7 {
            let cell = glam::IVec2::new(x, y);
            let gap = lane_builder::distance_to_polyline(grid::cell_center(cell), &path);
            let plot = LotSystem::compute_plot(&roads, cell);
            if gap < 16.0 {
                assert!(
                    !plot.has_area(),
                    "no building on the road itself, cell {cell}"
                );
                under += 1;
            } else if plot.has_area() && plot.area() < full {
                beside += 1;
                // The plot keeps clear of the road (its center no closer than the road's edge).
                assert!(
                    lane_builder::distance_to_polyline(plot.center(), &path)
                        > local().total_width() * 0.5
                );
            }
        }
    }
    assert!(under > 0);
    assert!(beside > 0, "cells beside the curve keep a smaller plot");
}

#[test]
#[ignore = "needs citizens (M7.3): test_a_curved_bypass_works_like_a_straight_one"]
fn a_curved_bypass_works_like_a_straight_one() {
    unimplemented!("port with M7.3");
}
