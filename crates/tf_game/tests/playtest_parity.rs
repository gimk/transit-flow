//! R4: the scripted playtest of `docs/IMPLEMENTATION_PLAN.md` ("How to playtest"), played
//! through the Bevy build's real input path (keys as messages, `Pointer`, mouse buttons)
//! and compared, step by step, with Godot's run of the same input
//! (`migration/probes/playtest_trace.gd` -> `migration/golden/playtest_trace.json`).
//! After each step (Godot: `_finish`) the save (`Simulation::to_dict`, numbers within 1e-3),
//! the active tool and variant, and the metrics (vehicles, pedestrians, waiting departures,
//! arrivals, failures, Flow, trip count, time) must be equal, and at the end every vehicle.
//! The sim is stepped by hand with the game paused, so both runs see the same ticks.

mod common;

use bevy::input::ButtonState;
use bevy::prelude::*;
use common::Harness;
use serde::Deserialize;
use serde_json::Value;
use tf_sim::events::SimEvent;

#[derive(Deserialize)]
struct Trace {
    steps: Vec<Step>,
    vehicles: Vec<(i64, i64, f64, f64)>,
}

#[derive(Deserialize)]
struct Step {
    name: String,
    /// `["action" | "game", name]` or `["mouse", button, pressed, x, y]`.
    events: Vec<Vec<Value>>,
    ticks: usize,
    tool: (String, String),
    save: Value,
    metrics: Metrics,
}

#[derive(Deserialize)]
struct Metrics {
    vehicles: usize,
    pedestrians: usize,
    waiting: usize,
    arrivals: usize,
    failed: usize,
    flow: Option<i64>,
    trips: usize,
    time: f64,
}

fn key_of(action: &str) -> KeyCode {
    match action {
        "sim_toggle_pause" => KeyCode::Space,
        "debug_load_scenario" => KeyCode::F7,
        "debug_stress_traffic" => KeyCode::F6,
        "debug_load_town" => KeyCode::F8,
        "debug_rush_hour" => KeyCode::F10,
        "debug_bus_lanes" => KeyCode::Digit0,
        "debug_new_map" => KeyCode::F12,
        "tool_road" => KeyCode::KeyR,
        "road_curve_mode" => KeyCode::KeyV,
        "tool_cancel" => KeyCode::Escape,
        "tool_bulldoze" => KeyCode::KeyB,
        "tool_residential" => KeyCode::KeyH,
        "tool_commercial" => KeyCode::KeyC,
        "tool_industrial" => KeyCode::KeyI,
        "tool_dezone" => KeyCode::KeyX,
        "tool_signals" => KeyCode::KeyL,
        "tool_bus" => KeyCode::KeyU,
        other => panic!("no key for action {other}"),
    }
}

/// Numbers within 1e-3 (Godot writes every number as a float).
fn assert_json_matches(rust: &Value, godot: &Value, path: &str) {
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

#[test]
fn the_playtest_matches_godot_step_by_step() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migration/golden/playtest_trace.json"
    );
    let trace: Trace = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();

    let mut h = Harness::new();
    let (mut arrivals, mut failed) = (0, 0);
    for step in &trace.steps {
        for event in &step.events {
            match event[0].as_str().unwrap() {
                "action" | "game" => h.key(key_of(event[1].as_str().unwrap())),
                "mouse" => {
                    let button = event[1].as_i64().unwrap();
                    let at = Vec2::new(
                        event[3].as_f64().unwrap() as f32,
                        event[4].as_f64().unwrap() as f32,
                    );
                    h.move_to(at);
                    if button != 0 {
                        let state = if event[2].as_bool().unwrap() {
                            ButtonState::Pressed
                        } else {
                            ButtonState::Released
                        };
                        h.button(
                            if button == 1 {
                                MouseButton::Left
                            } else {
                                MouseButton::Right
                            },
                            state,
                        );
                    }
                }
                other => panic!("event {other}"),
            }
        }
        // As the probe's `_finish`: a couple of frames, then the ticks.
        h.frames(2);
        let ticks = step.ticks;
        let (now_arrivals, now_failed, metrics, save) = h.sim(|sim| {
            let (mut arrived, mut failed) = (0, 0);
            for _ in 0..ticks {
                sim.step();
                for event in sim.take_view_events() {
                    match event {
                        SimEvent::VehicleArrived(_) => arrived += 1,
                        SimEvent::VehicleFailed(_) => failed += 1,
                        _ => {}
                    }
                }
            }
            let now = sim.sim_time();
            let flow = sim.flow.flow_percent(now);
            let metrics = (
                sim.vehicles.vehicles.len(),
                sim.pedestrians.pedestrians.len(),
                sim.vehicles.waiting_departures().len(),
                (flow >= 0.0).then(|| (flow * 1e6).round() as i64),
                sim.flow.trip_count(now),
                now,
            );
            (arrived, failed, metrics, sim.to_dict())
        });
        arrivals += now_arrivals;
        failed += now_failed;

        let name = &step.name;
        let tool = (
            h.active_tool().unwrap_or("").to_string(),
            h.tool_variant().unwrap_or_default(),
        );
        assert_eq!(tool, step.tool, "{name}: active tool and variant");
        assert_json_matches(&save, &step.save, &format!("{name}: save"));
        let m = &step.metrics;
        assert_eq!(
            (metrics.0, metrics.1, metrics.2),
            (m.vehicles, m.pedestrians, m.waiting),
            "{name}: vehicles, pedestrians, waiting departures"
        );
        assert_eq!(
            (arrivals, failed),
            (m.arrivals, m.failed),
            "{name}: arrivals, failures"
        );
        assert_eq!(
            (metrics.3, metrics.4),
            (m.flow, m.trips),
            "{name}: Flow, trips"
        );
        assert!((metrics.5 - m.time).abs() < 1e-3, "{name}: time");
    }

    h.sim(|sim| {
        assert_eq!(sim.vehicles.vehicles.len(), trace.vehicles.len());
        for (v, g) in sim.vehicles.vehicles.values().zip(&trace.vehicles) {
            assert_eq!(
                (i64::from(v.id.0), v.lane.map(|l| i64::from(l.0))),
                (g.0, Some(g.1)),
                "vehicle, lane"
            );
            let round3 = |x: f64| (x * 1000.0).round() / 1000.0;
            assert!(
                (round3(v.distance) - g.2).abs() <= 0.002 && (round3(v.speed) - g.3).abs() <= 0.002,
                "vehicle {} at {:?}, Godot {:?}",
                g.0,
                (v.distance, v.speed),
                (g.2, g.3)
            );
        }
    });
}

/// Godot's saves (every step of the playtest, so roads of all types, curves, lights, zones,
/// lots and bus lines) load in the Rust build and save back the same: a quicksave made by
/// either build opens in the other.
#[test]
fn godot_saves_load_and_save_back_the_same() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migration/golden/playtest_trace.json"
    );
    let trace: Trace = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for step in &trace.steps {
        let sim = tf_sim::Simulation::from_dict(&step.save)
            .unwrap_or_else(|| panic!("{}: the save loads", step.name));
        assert_json_matches(
            &sim.to_dict(),
            &step.save,
            &format!("{}: round trip", step.name),
        );
    }
}
