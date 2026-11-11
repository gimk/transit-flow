//! `geom::Curve2D` bakes and samples exactly like Godot's `Curve2D`: every lane and road
//! centreline in the golden files, rebuilt from its control points.

mod common;

use common::golden::{self, Curve, angle_difference, v};

fn check(curve: &Curve, what: &str, worst: &mut (f32, f32)) {
    let rust = curve.to_curve();
    assert_eq!(
        rust.baked_points().len(),
        curve.baked_points.len(),
        "{what}: baked point count"
    );
    for (i, (&got, &expected)) in rust
        .baked_points()
        .iter()
        .zip(&curve.baked_points)
        .enumerate()
    {
        let error = got.distance(v(expected));
        worst.0 = worst.0.max(error);
        assert!(
            error < 1e-3,
            "{what}: baked point {i} {got} vs Godot {expected:?}"
        );
    }
    assert!(
        (rust.baked_length() - curve.baked_length).abs() < 1e-3,
        "{what}: baked length {} vs Godot {}",
        rust.baked_length(),
        curve.baked_length
    );
    for &[offset, x, y, rotation] in &curve.samples {
        let (position, angle) = rust.sample_baked_with_rotation(offset);
        let error = position.distance(v([x, y]));
        worst.0 = worst.0.max(error);
        assert!(
            error < 1e-3,
            "{what}: sample at {offset}: {position} vs Godot ({x}, {y})"
        );
        let turn = angle_difference(angle, rotation);
        worst.1 = worst.1.max(turn);
        assert!(
            turn < 1e-4,
            "{what}: rotation at {offset}: {angle} vs Godot {rotation}"
        );
    }
}

#[test]
fn every_golden_curve_bakes_and_samples_like_godot() {
    let mut worst = (0.0f32, 0.0f32);
    let mut count = 0;
    for (label, network) in golden::all_networks() {
        for lane in &network.lanes {
            check(
                &lane.curve,
                &format!("{label}, lane {}", lane.id),
                &mut worst,
            );
            count += 1;
        }
        for segment in &network.segments {
            check(
                &segment.centerline,
                &format!("{label}, segment {}", segment.id),
                &mut worst,
            );
            count += 1;
        }
    }
    println!(
        "{count} curves; worst position error {} px, worst rotation error {} rad",
        worst.0, worst.1
    );
    assert!(count > 1000);
}
