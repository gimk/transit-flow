//! Reads the reference data the Godot build exported (`migration/golden/`, written by
//! `migration/export_golden.gd`; its header documents the format). Only the parts tests
//! use so far are typed; add fields as ports need them.

use std::path::PathBuf;

use glam::Vec2;
use serde::Deserialize;
use tf_sim::geom::{Curve2D, CurvePoint};

/// Every layout file (not `edits.json`).
pub const LAYOUTS: [&str; 5] = [
    "bottleneck",
    "bottleneck_bypass",
    "bottleneck_bus_lanes",
    "town",
    "junctions",
];

pub fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../migration/golden")
        .join(format!("{name}.json"))
}

pub fn read<T: for<'de> Deserialize<'de>>(name: &str) -> T {
    let text = std::fs::read_to_string(path(name)).expect("golden file");
    serde_json::from_str(&text).expect("golden format")
}

pub fn v(p: [f32; 2]) -> Vec2 {
    Vec2::from_array(p)
}

#[derive(Deserialize)]
pub struct Layout {
    pub save: serde_json::Value,
    pub roads: Network,
}

#[derive(Deserialize)]
pub struct EditStep {
    pub command: String,
    pub args: serde_json::Value,
    pub result: serde_json::Value,
    pub changed: serde_json::Value,
    pub roads: Network,
}

#[derive(Deserialize)]
pub struct Network {
    pub nodes: Vec<Node>,
    pub segments: Vec<Segment>,
    pub lanes: Vec<Lane>,
}

#[derive(Deserialize)]
pub struct Node {
    pub id: i32,
    pub position: [f32; 2],
    pub elevation_layer: i32,
    pub control: i32,
    pub signal_phase_count: i32,
    pub segment_ids: Vec<i32>,
    pub connector_lane_ids: Vec<i32>,
}

#[derive(Deserialize)]
pub struct Segment {
    pub id: i32,
    pub start_node_id: i32,
    pub end_node_id: i32,
    pub road_type: String,
    pub elevation_layer: i32,
    pub lane_ids: Vec<i32>,
    pub polyline: Vec<[f32; 2]>,
    pub length: f32,
    pub centerline: Curve,
}

#[derive(Deserialize)]
pub struct Lane {
    pub id: i32,
    pub segment_id: i32,
    pub from_node_id: i32,
    pub to_node_id: i32,
    pub index_from_center: i32,
    pub turn: i32,
    pub speed_limit: f64,
    pub bus_only: bool,
    pub feeder_lane_id: i32,
    pub signal_phase: i32,
    pub is_straight: bool,
    pub length: f64,
    pub outgoing_lane_ids: Vec<i32>,
    pub conflicting_lane_ids: Vec<i32>,
    pub conflict_zones: std::collections::BTreeMap<String, [f32; 2]>,
    pub curve: Curve,
}

#[derive(Deserialize)]
pub struct Curve {
    pub points: Vec<[f32; 2]>,
    #[serde(rename = "in")]
    pub ins: Vec<[f32; 2]>,
    #[serde(rename = "out")]
    pub outs: Vec<[f32; 2]>,
    pub bake_interval: f32,
    pub baked_points: Vec<[f32; 2]>,
    pub baked_length: f32,
    /// `[offset, x, y, rotation]`.
    pub samples: Vec<[f32; 4]>,
}

impl Curve {
    /// The same curve built in Rust from Godot's control points.
    pub fn to_curve(&self) -> Curve2D {
        let points = (0..self.points.len())
            .map(|i| CurvePoint {
                position: v(self.points[i]),
                point_in: v(self.ins[i]),
                point_out: v(self.outs[i]),
            })
            .collect();
        Curve2D::with_bake_interval(points, self.bake_interval)
    }
}

/// Every layout's network, then every step of `edits.json`, with a label.
pub fn all_networks() -> Vec<(String, Network)> {
    let mut networks: Vec<(String, Network)> = LAYOUTS
        .iter()
        .map(|name| (name.to_string(), read::<Layout>(name).roads))
        .collect();
    let edits: Vec<EditStep> = read("edits");
    for (i, step) in edits.into_iter().enumerate() {
        networks.push((format!("edits step {i} ({})", step.command), step.roads));
    }
    networks
}

/// Smallest difference between two angles, in radians.
pub fn angle_difference(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    d.min(std::f32::consts::TAU - d)
}
