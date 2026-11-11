//! Port of `tests/unit/test_lane_builder.gd`, plus connector curves checked against the
//! golden files.

mod common;

use approx::assert_abs_diff_eq;
use glam::Vec2;
use tf_sim::geom::{Curve2D, CurvePoint};
use tf_sim::roads::RoadType;
use tf_sim::roads::lane_builder::{self, LaneSpec};

use common::golden::{self, v};

fn road_type() -> RoadType {
    RoadType {
        lanes_per_direction: 1,
        lane_width: 14.0,
        ..RoadType::default()
    }
}

/// `RoadNetwork.straight_curve`.
fn straight_curve(from: Vec2, to: Vec2) -> Curve2D {
    Curve2D::from_points(&[from, to])
}

fn points(curve: &Curve2D) -> Vec<Vec2> {
    lane_builder::get_polyline(curve)
}

#[test]
fn eastbound_road_drives_on_the_right() {
    let specs = lane_builder::build_lane_specs(
        &straight_curve(Vec2::ZERO, Vec2::new(128.0, 0.0)),
        &road_type(),
        0.0,
        0.0,
    );
    assert_eq!(specs.len(), 2);
    let forward = specs.iter().find(|s| s.forward).unwrap();
    let backward = specs.iter().find(|s| !s.forward).unwrap();
    // Heading east (+x) in y-down space, the right-hand side is south (+y).
    assert_eq!(
        points(&forward.curve),
        vec![Vec2::new(0.0, 7.0), Vec2::new(128.0, 7.0)]
    );
    // The backward lane heads west, so its right-hand side is north (-y).
    assert_eq!(
        points(&backward.curve),
        vec![Vec2::new(128.0, -7.0), Vec2::new(0.0, -7.0)]
    );
}

#[test]
fn straight_lane_length_matches_road() {
    let specs = lane_builder::build_lane_specs(
        &straight_curve(Vec2::ZERO, Vec2::new(0.0, 192.0)),
        &road_type(),
        0.0,
        0.0,
    );
    for spec in &specs {
        assert_abs_diff_eq!(spec.curve.baked_length(), 192.0, epsilon = 0.01);
    }
}

#[test]
fn multiple_lanes_per_direction_stack_outward() {
    let road_type = RoadType {
        lanes_per_direction: 2,
        ..road_type()
    };
    let specs = lane_builder::build_lane_specs(
        &straight_curve(Vec2::ZERO, Vec2::new(128.0, 0.0)),
        &road_type,
        0.0,
        0.0,
    );
    assert_eq!(specs.len(), 4);
    for spec in &specs {
        let expected_offset = (spec.index as f32 + 0.5) * 14.0;
        assert_abs_diff_eq!(
            points(&spec.curve)[0].y.abs(),
            expected_offset,
            epsilon = 0.001
        );
    }
}

#[test]
fn curved_centerline_lanes_stay_at_constant_offset() {
    // Quarter circle-ish arc from (0,0) to (128,128).
    let centerline = Curve2D::new(vec![
        CurvePoint {
            position: Vec2::ZERO,
            point_in: Vec2::ZERO,
            point_out: Vec2::new(70.0, 0.0),
        },
        CurvePoint {
            position: Vec2::new(128.0, 128.0),
            point_in: Vec2::new(0.0, -70.0),
            point_out: Vec2::ZERO,
        },
    ]);
    let centerline_points = lane_builder::get_polyline(&centerline);
    let specs: Vec<LaneSpec> = lane_builder::build_lane_specs(&centerline, &road_type(), 0.0, 0.0);
    for spec in &specs {
        for point in points(&spec.curve) {
            assert_abs_diff_eq!(
                lane_builder::distance_to_polyline(point, &centerline_points),
                7.0,
                epsilon = 0.5
            );
        }
    }
}

/// Every connector in the golden files is `build_connector_curve(feeder, exit)`: the
/// same control points and handles.
#[test]
fn connector_curves_match_godot() {
    let mut count = 0;
    for (label, network) in golden::all_networks() {
        let lanes: std::collections::HashMap<i32, &golden::Lane> =
            network.lanes.iter().map(|lane| (lane.id, lane)).collect();
        for lane in network.lanes.iter().filter(|lane| lane.segment_id == -1) {
            let feeder = lanes[&lane.feeder_lane_id];
            let exit = lanes[&lane.outgoing_lane_ids[0]];
            let curve = lane_builder::build_connector_curve(
                &feeder.curve.to_curve(),
                &exit.curve.to_curve(),
            );
            let expected = &lane.curve;
            for i in 0..2 {
                for (got, want, what) in [
                    (curve.point_position(i), expected.points[i], "position"),
                    (curve.point_in(i), expected.ins[i], "in"),
                    (curve.point_out(i), expected.outs[i], "out"),
                ] {
                    assert!(
                        got.distance(v(want)) < 1e-3,
                        "{label}, connector {}: point {i} {what} {got} vs Godot {want:?}",
                        lane.id
                    );
                }
            }
            count += 1;
        }
    }
    assert!(count > 500, "{count} connectors");
}
