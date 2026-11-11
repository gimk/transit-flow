//! Reads the reference data the Godot build exported (`migration/golden/`, written by
//! `migration/export_golden.gd`; its header documents the format). Only the parts tests
//! use so far are typed; add fields as ports need them.

use std::path::PathBuf;

use glam::Vec2;
use serde::Deserialize;
use tf_sim::geom::{Curve2D, CurvePoint};

/// Every layout file (not `edits.json`).
pub const LAYOUTS: [&str; 6] = [
    "bottleneck",
    "bottleneck_bypass",
    "bottleneck_bus_lanes",
    "town",
    "junctions",
    "grid",
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
    pub lots: Vec<Lot>,
    pub routes: Vec<RouteSample>,
    pub lot_routes: Vec<LotRouteSample>,
    pub walk: Walk,
    pub transit: Transit,
}

/// `walk` of a layout: the walk network after `_ensure_built`, and sampled paths.
#[derive(Deserialize)]
pub struct Walk {
    pub points: Vec<WalkPoint>,
    /// `"a,b"` → `[node id, segment id]`, in Godot's insertion order.
    pub crosswalk_edges: indexmap::IndexMap<String, [i32; 2]>,
    /// `"segment id,side"` → `[start point, end point]`.
    pub sidewalks: indexmap::IndexMap<String, [i32; 2]>,
    pub paths: Vec<WalkSample>,
}

#[derive(Deserialize)]
pub struct WalkPoint {
    pub id: i32,
    pub position: [f32; 2],
    pub connections: Vec<i32>,
}

#[derive(Deserialize)]
pub struct WalkSample {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub path: GoldenWalkPath,
}

/// A walking path, or all-empty when Godot found none (`{}`).
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct GoldenWalkPath {
    pub points: Vec<[f32; 2]>,
    pub crossings: Vec<[i32; 2]>,
    pub length: f64,
}

/// `transit` of a layout: the stops, lines and mid-block crosswalks of the reloaded save.
#[derive(Deserialize)]
pub struct Transit {
    pub stops: Vec<Stop>,
    pub lines: Vec<Line>,
    pub mid_block: indexmap::IndexMap<String, MidBlock>,
}

#[derive(Deserialize)]
pub struct Stop {
    pub id: i32,
    pub position: [f32; 2],
    pub direction: [f32; 2],
    pub lane_id: i32,
    pub distance: f64,
    pub bay: bool,
}

#[derive(Deserialize)]
pub struct Line {
    pub id: i32,
    pub stop_ids: Vec<i32>,
    pub color: [f32; 4],
    pub path: Vec<[f32; 2]>,
    pub free_flow_times: Vec<f64>,
    pub routes: Vec<Option<GoldenRoute>>,
}

#[derive(Deserialize)]
pub struct MidBlock {
    pub segment_id: i32,
    pub distance: f64,
    pub center: [f32; 2],
    pub across: [f32; 2],
    pub half_width: f64,
    pub lanes: Vec<(i32, f64)>,
}

/// A `"x,y"` dictionary key.
pub fn key(text: &str) -> glam::IVec2 {
    let (x, y) = text.split_once(',').expect("an x,y key");
    glam::IVec2::new(x.parse().expect("x"), y.parse().expect("y"))
}

impl Transit {
    /// The crosswalks state a simulation of the layout has: its mid-block crosswalks.
    pub fn crosswalks(&self) -> tf_sim::walking::Crosswalks {
        use tf_sim::ids::{LaneId, SegmentId};
        use tf_sim::walking::crosswalks::MidBlockCrossing;
        let mut crosswalks = tf_sim::walking::Crosswalks::new();
        crosswalks.set_mid_block(
            self.mid_block
                .iter()
                .map(|(k, m)| {
                    (
                        key(k),
                        MidBlockCrossing {
                            segment_id: SegmentId(m.segment_id),
                            distance: m.distance,
                            center: v(m.center),
                            across: v(m.across),
                            half_width: m.half_width,
                            lanes: m.lanes.iter().map(|&(l, d)| (LaneId(l), d)).collect(),
                        },
                    )
                })
                .collect(),
        );
        crosswalks
    }
}

#[derive(Deserialize)]
pub struct Lot {
    pub id: i32,
    pub cell: [i32; 2],
    #[serde(rename = "type")]
    pub lot_type: i32,
    pub plot: [f32; 4],
    pub access: Vec<AccessPoint>,
}

#[derive(Deserialize)]
pub struct AccessPoint {
    pub lane_id: i32,
    pub distance: f64,
    #[serde(default)]
    pub penalty: f64,
}

#[derive(Deserialize)]
pub struct GoldenRoute {
    pub lane_ids: Vec<i32>,
    pub start_distance: f64,
    pub end_distance: f64,
    pub travel_time: f64,
    /// Not in the routes of a bus line's dump.
    #[serde(default)]
    pub free_flow_time: f64,
}

#[derive(Deserialize)]
pub struct RouteSample {
    pub from_lane: i32,
    pub from_distance: f64,
    pub to_lane: i32,
    pub to_distance: f64,
    pub bus: bool,
    pub route: Option<GoldenRoute>,
}

#[derive(Deserialize)]
pub struct LotRouteSample {
    pub from_lot: i32,
    pub to_lot: i32,
    pub route: Option<GoldenRoute>,
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

/// Asserts a Rust road network is the golden one: same ids, links, geometry (1e-3 px)
/// and junction data. Reports every kind of mismatch with `label`.
pub fn assert_network_matches(rust: &tf_sim::roads::RoadNetwork, golden: &Network, label: &str) {
    use tf_sim::ids::{LaneId, NodeId, SegmentId};
    let close = |a: Vec2, b: [f32; 2]| a.distance(v(b)) < 1e-3;
    let ids = |list: &[i32]| list.to_vec();

    let rust_nodes: Vec<i32> = rust.nodes.keys().map(|id| id.0).collect();
    let golden_nodes: Vec<i32> = golden.nodes.iter().map(|n| n.id).collect();
    assert_eq!(rust_nodes, golden_nodes, "{label}: node ids (in order)");
    for g in &golden.nodes {
        let n = &rust.nodes[&NodeId(g.id)];
        let what = format!("{label}, node {}", g.id);
        assert!(close(n.position, g.position), "{what}: position");
        assert_eq!(n.elevation_layer, g.elevation_layer, "{what}: layer");
        assert_eq!(n.control as i32, g.control, "{what}: control");
        assert_eq!(
            n.signal_phase_count, g.signal_phase_count,
            "{what}: signal phase count"
        );
        assert_eq!(
            n.segment_ids.iter().map(|id| id.0).collect::<Vec<_>>(),
            ids(&g.segment_ids),
            "{what}: segment ids"
        );
        assert_eq!(
            n.connector_lane_ids
                .iter()
                .map(|id| id.0)
                .collect::<Vec<_>>(),
            ids(&g.connector_lane_ids),
            "{what}: connector ids"
        );
    }

    let rust_segments: Vec<i32> = rust.segments.keys().map(|id| id.0).collect();
    let golden_segments: Vec<i32> = golden.segments.iter().map(|s| s.id).collect();
    assert_eq!(
        rust_segments, golden_segments,
        "{label}: segment ids (in order)"
    );
    for g in &golden.segments {
        let s = &rust.segments[&SegmentId(g.id)];
        let what = format!("{label}, segment {}", g.id);
        assert_eq!(
            (s.start_node_id.0, s.end_node_id.0),
            (g.start_node_id, g.end_node_id),
            "{what}: nodes"
        );
        assert_eq!(s.road_type.id, g.road_type, "{what}: type");
        assert_eq!(s.elevation_layer, g.elevation_layer, "{what}: layer");
        assert_eq!(
            s.lane_ids.iter().map(|id| id.0).collect::<Vec<_>>(),
            ids(&g.lane_ids),
            "{what}: lane ids"
        );
        assert_eq!(
            s.polyline().len(),
            g.polyline.len(),
            "{what}: polyline size"
        );
        for (p, q) in s.polyline().iter().zip(&g.polyline) {
            assert!(close(*p, *q), "{what}: polyline point {p} vs {q:?}");
        }
        assert!(
            (s.length() - f64::from(g.length)).abs() < 1e-3,
            "{what}: length"
        );
        assert_eq!(
            s.centerline().point_count(),
            g.centerline.points.len(),
            "{what}: centerline points"
        );
    }

    let rust_lanes: Vec<i32> = rust.lanes.keys().map(|id| id.0).collect();
    let golden_lanes: Vec<i32> = golden.lanes.iter().map(|l| l.id).collect();
    assert_eq!(rust_lanes, golden_lanes, "{label}: lane ids (in order)");
    for g in &golden.lanes {
        let l = &rust.lanes[&LaneId(g.id)];
        let what = format!("{label}, lane {}", g.id);
        assert_eq!(
            l.segment_id.map_or(-1, |id| id.0),
            g.segment_id,
            "{what}: segment"
        );
        assert_eq!(
            (l.from_node_id.0, l.to_node_id.0),
            (g.from_node_id, g.to_node_id),
            "{what}: nodes"
        );
        assert_eq!(l.index_from_center, g.index_from_center, "{what}: index");
        assert_eq!(l.turn as i32, g.turn, "{what}: turn");
        assert!(
            (l.speed_limit - g.speed_limit).abs() < 1e-3,
            "{what}: speed {} vs {}",
            l.speed_limit,
            g.speed_limit
        );
        assert_eq!(l.bus_only, g.bus_only, "{what}: bus only");
        assert_eq!(
            l.feeder_lane_id.map_or(-1, |id| id.0),
            g.feeder_lane_id,
            "{what}: feeder"
        );
        assert_eq!(l.signal_phase, g.signal_phase, "{what}: signal phase");
        assert_eq!(l.is_straight, g.is_straight, "{what}: straight");
        assert!(
            (l.length - g.length).abs() < 1e-3,
            "{what}: length {} vs {}",
            l.length,
            g.length
        );
        assert_eq!(
            l.outgoing_lane_ids
                .iter()
                .map(|id| id.0)
                .collect::<Vec<_>>(),
            ids(&g.outgoing_lane_ids),
            "{what}: outgoing"
        );
        assert_eq!(
            l.conflicting_lane_ids
                .iter()
                .map(|id| id.0)
                .collect::<Vec<_>>(),
            ids(&g.conflicting_lane_ids),
            "{what}: conflicting"
        );
        assert_eq!(
            l.conflict_zones.len(),
            g.conflict_zones.len(),
            "{what}: conflict zone count"
        );
        for (other, zone) in &g.conflict_zones {
            let got = l.conflict_zones[&LaneId(other.parse().unwrap())];
            assert!(
                close(got, *zone),
                "{what}: conflict zone with {other}: {got} vs {zone:?}"
            );
        }
        let curve = l.curve();
        assert_eq!(
            curve.point_count(),
            g.curve.points.len(),
            "{what}: curve points"
        );
        for i in 0..curve.point_count() {
            assert!(
                close(curve.point_position(i), g.curve.points[i]),
                "{what}: curve point {i}"
            );
            assert!(
                close(curve.point_in(i), g.curve.ins[i]),
                "{what}: curve in {i}"
            );
            assert!(
                close(curve.point_out(i), g.curve.outs[i]),
                "{what}: curve out {i}"
            );
        }
    }
}

/// Compares two JSON values, numbers within 1e-3 (Godot writes every number as a float).
pub fn assert_json_matches(rust: &serde_json::Value, godot: &serde_json::Value, path: &str) {
    use serde_json::Value;
    match (rust, godot) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            assert!((a - b).abs() < 1e-3, "{path}: {a} vs Godot {b}");
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}: array length");
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                assert_json_matches(x, y, &format!("{path}[{i}]"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            let mut keys_a: Vec<_> = a.keys().collect();
            let mut keys_b: Vec<_> = b.keys().collect();
            keys_a.sort();
            keys_b.sort();
            assert_eq!(keys_a, keys_b, "{path}: keys");
            for (key, x) in a {
                assert_json_matches(x, &b[key], &format!("{path}.{key}"));
            }
        }
        _ => assert_eq!(rust, godot, "{path}"),
    }
}
