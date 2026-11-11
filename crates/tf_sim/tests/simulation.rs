//! Port of `tests/unit/test_simulation.gd`, plus the save round trip over every golden
//! layout: Godot's `Simulation.to_dict()` of each layout, loaded and saved by the Rust
//! build, comes out the same.

mod common;

use common::golden::{self, Layout, assert_json_matches};
use common::roads::local;
use glam::Vec2;
use serde_json::json;
use tf_sim::Simulation;

#[test]
fn one_second_of_ticks_advances_clock_by_one_sim_second() {
    let mut sim = Simulation::new();
    for _ in 0..Simulation::TICK_RATE {
        sim.step();
    }
    assert_eq!(sim.tick_count, u64::from(Simulation::TICK_RATE));
    assert!((sim.sim_time() - 1.0).abs() < 0.0001);
    assert_eq!(sim.clock.format_time(), "06:05");
}

#[test]
fn save_round_trip_through_json() {
    let mut sim = Simulation::new();
    sim.call(|sim, out| {
        sim.roads
            .add_road(Vec2::ZERO, Vec2::new(256.0, 0.0), local(), 0, out);
    });
    for _ in 0..37 {
        sim.step();
    }
    let saved = sim.to_dict();
    let text = saved.to_string();
    let loaded = Simulation::from_dict(&serde_json::from_str(&text).unwrap())
        .expect("a save of this version loads");
    assert_eq!(loaded.to_dict(), saved);
    assert_eq!(loaded.clock.format_time(), sim.clock.format_time());
    assert_eq!(loaded.tick_count, 37);
}

#[test]
fn rejects_unknown_save_version() {
    assert!(Simulation::from_dict(&json!({"version": 999})).is_none());
    assert!(Simulation::from_dict(&json!({})).is_none());
}

/// Every golden layout (Godot's own `Simulation.to_dict()`), loaded and saved again.
#[test]
fn golden_saves_load_and_save_back_unchanged() {
    for name in golden::LAYOUTS {
        let layout: Layout = golden::read(name);
        let loaded = Simulation::from_dict(&layout.save).expect("Godot's save loads");
        assert_json_matches(&loaded.to_dict(), &layout.save, name);
        // And through the text of our own save.
        let again = Simulation::from_dict(&loaded.to_dict()).expect("our save loads");
        assert_json_matches(
            &again.to_dict(),
            &loaded.to_dict(),
            &format!("{name}: second round"),
        );
    }
}

/// A road made of other roads' pieces: a save from the middle of a game keeps the ids the
/// counters will hand out next.
#[test]
fn saved_counters_continue_after_a_load() {
    let mut sim = Simulation::new();
    sim.call(|sim, out| {
        sim.roads
            .add_road(Vec2::ZERO, Vec2::new(256.0, 0.0), local(), 0, out);
    });
    let loaded = Simulation::from_dict(&sim.to_dict()).unwrap();
    assert_eq!(
        loaded.roads.to_dict()["next_segment_id"],
        sim.roads.to_dict()["next_segment_id"]
    );
}
