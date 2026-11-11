//! `RoadNetwork` against the Godot build's reference data: every golden layout loaded
//! from its save, and the recorded sequence of edits replayed command by command.

mod common;

use common::golden::{self, EditStep, Layout, assert_json_matches, assert_network_matches};
use glam::Vec2;
use serde_json::Value;
use tf_sim::events::{Outbox, SimEvent};
use tf_sim::ids::NodeId;
use tf_sim::roads::{JunctionControl, RoadNetwork, RoadType};

#[test]
fn every_layout_loads_into_godots_network() {
    for name in golden::LAYOUTS {
        let layout: Layout = golden::read(name);
        let roads = RoadNetwork::from_dict(&layout.save["roads"]);
        assert_network_matches(&roads, &layout.roads, name);
        assert_json_matches(
            &roads.to_dict(),
            &layout.save["roads"],
            &format!("{name} save"),
        );
    }
}

fn point(value: &Value) -> Vec2 {
    Vec2::new(
        value[0].as_f64().unwrap() as f32,
        value[1].as_f64().unwrap() as f32,
    ) * 64.0
}

fn road_type(value: &Value) -> &'static RoadType {
    RoadType::load_by_id(value.as_str().unwrap()).unwrap()
}

/// Runs one recorded command; returns its result as Godot reported it.
fn run(roads: &mut RoadNetwork, step: &EditStep, out: &mut Outbox) -> Value {
    let a = &step.args;
    let ids = |ids: Vec<tf_sim::ids::SegmentId>| {
        Value::from(ids.iter().map(|id| id.0).collect::<Vec<_>>())
    };
    match step.command.as_str() {
        "add_road" => Value::from(
            roads
                .add_road(point(&a[0]), point(&a[1]), road_type(&a[2]), 0, out)
                .map_or(-1, |id| id.0),
        ),
        "add_road_pieces" => {
            ids(roads.add_road_pieces(point(&a[0]), point(&a[1]), road_type(&a[2]), 0, out))
        }
        "add_curved_road" => ids(roads.add_curved_road(
            point(&a[0]),
            point(&a[1]),
            point(&a[2]),
            road_type(&a[3]),
            0,
            out,
        )),
        "set_node_control" => {
            let node = roads.find_node_at(point(&a[0]), 0).unwrap_or(NodeId(-1));
            let control = match a[1].as_i64().unwrap() {
                1 => JunctionControl::TrafficLight,
                2 => JunctionControl::TrafficLightProtectedLeft,
                _ => JunctionControl::None,
            };
            Value::from(roads.set_node_control(node, control, out))
        }
        "remove_segment_at" => {
            let segment = roads.find_segment_at(point(&a[0]), 4.0, 0);
            Value::from(segment.is_some_and(|id| roads.remove_segment(id, out)))
        }
        other => panic!("unknown command {other}"),
    }
}

#[test]
fn recorded_edits_build_godots_network_step_by_step() {
    let steps: Vec<EditStep> = golden::read("edits");
    let mut roads = RoadNetwork::new();
    for (i, step) in steps.iter().enumerate() {
        let label = format!("step {i} ({} {})", step.command, step.args);
        let mut out = Outbox::new();
        let result = run(&mut roads, step, &mut out);
        assert_json_matches(&result, &step.result, &format!("{label} result"));
        let changed = match out.as_slice() {
            [] => Value::Array(vec![]),
            [SimEvent::RoadsChanged(change)] => serde_json::json!([
                change.node_ids.iter().map(|id| id.0).collect::<Vec<_>>(),
                change.segment_ids.iter().map(|id| id.0).collect::<Vec<_>>(),
            ]),
            other => panic!("{label}: unexpected events {other:?}"),
        };
        assert_json_matches(&changed, &step.changed, &format!("{label} changed"));
        assert_network_matches(&roads, &step.roads, &label);
    }
}
