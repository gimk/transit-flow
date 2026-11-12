//! The debug keys (M10.3): every function of Godot's debug panel (`ui/debug_menu.gd`) has a
//! key, and each one has its effect. This ports the intent of
//! `test_debug_menu_buttons_drive_the_debug_functions`. Godot's day/night and dot display
//! toggles are not in the Bevy build (plan §1), so they have no key.

mod common;

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowMode};
use common::Harness;
use tf_game::events::DebugOverlayChanged;
use tf_game::game::{self, STRESS_TRAFFIC_STEPS};
use tf_game::views::moving::ShowLateness;
use tf_game::views::overlays::{Heatmap, LaneOverlay};

#[test]
fn f2_f3_f4_switch_the_overlays() {
    let mut h = Harness::new();
    let changed = h.press_collect::<DebugOverlayChanged>(KeyCode::F4);
    assert!(changed.iter().any(|c| c.overlay_id == "heatmap" && c.shown));
    assert!(h.app.world().resource::<Heatmap>().shown);
    h.key(KeyCode::F4);
    assert!(!h.app.world().resource::<Heatmap>().shown);

    h.key(KeyCode::F3);
    assert!(h.app.world().resource::<LaneOverlay>().shown);

    h.key(KeyCode::F2);
    assert!(h.app.world().resource::<ShowLateness>().0);
}

#[test]
fn f5_saves_and_f9_loads_the_quicksave() {
    let dir = std::env::temp_dir().join(format!("transit-flow-debug-keys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // Safe: this is the only test in the file that reads TRANSIT_FLOW_DATA_DIR.
    unsafe { std::env::set_var("TRANSIT_FLOW_DATA_DIR", &dir) };

    let mut h = Harness::new();
    h.key(KeyCode::F8);
    h.key(KeyCode::F5);
    assert!(game::quicksave_path().exists());
    assert!(h.status().contains("> Game saved"), "{}", h.status());

    h.key(KeyCode::F7);
    assert_ne!(
        h.sim(|sim| sim.externals.active_points(&sim.roads).len()),
        2,
        "the Bottleneck is not the Town"
    );
    h.key(KeyCode::F9);
    assert!(h.status().contains("> Game loaded"), "{}", h.status());
    assert_eq!(
        h.sim(|sim| sim.externals.active_points(&sim.roads).len()),
        2,
        "the Town is back"
    );
}

#[test]
fn f6_cycles_the_stress_traffic_steps() {
    let mut h = Harness::new();
    h.key(KeyCode::F6);
    assert_eq!(
        h.sim(|sim| sim.stress.target_count),
        STRESS_TRAFFIC_STEPS[1]
    );
    assert!(
        h.status().contains("> Stress traffic: 250 vehicles"),
        "{}",
        h.status()
    );
    h.key(KeyCode::F6);
    assert_eq!(
        h.sim(|sim| sim.stress.target_count),
        STRESS_TRAFFIC_STEPS[2]
    );
}

#[test]
fn f7_loads_the_bottleneck_and_zero_loads_it_with_bus_lanes() {
    let mut h = Harness::new();
    h.key(KeyCode::F7);
    assert_eq!(h.sim(|sim| sim.citizens.citizens.len()), 56);
    assert!(
        h.status().contains("> Scenario: Bottleneck."),
        "{}",
        h.status()
    );

    let mut h = Harness::new();
    h.key(KeyCode::Digit0);
    assert_eq!(
        h.sim(|sim| sim.transit.lines.len()),
        1,
        "the bus line along the main road"
    );
    assert!(
        h.status().contains("Bottleneck with bus lanes"),
        "{}",
        h.status()
    );
}

#[test]
fn f8_loads_the_town() {
    let mut h = Harness::new();
    h.key(KeyCode::F8);
    assert_eq!(
        h.sim(|sim| sim.externals.active_points(&sim.roads).len()),
        2
    );
    assert!(h.status().contains("> Scenario: Town."), "{}", h.status());
}

/// F10 (and the fallback P, for keyboards where macOS keeps F10) makes everyone at home leave.
#[test]
fn f10_and_p_start_rush_hour() {
    for key in [KeyCode::F10, KeyCode::KeyP] {
        let mut h = Harness::new();
        h.key(KeyCode::F8);
        let at_home = |h: &mut Harness| {
            h.sim(|sim| {
                sim.citizens
                    .citizens
                    .values()
                    .filter(|c| c.state == tf_sim::city::citizen::State::AtHome)
                    .count()
            })
        };
        let before = at_home(&mut h);
        assert!(before > 0, "the Town starts with people at home");
        h.key(key);
        assert!(h.status().contains("> Rush hour"), "{}", h.status());
        h.run_sim(|sim| (0..20).for_each(|_| sim.step()));
        assert!(at_home(&mut h) < before, "some of them have left");
    }
}

#[test]
fn f11_toggles_fullscreen() {
    let mut h = Harness::new();
    let window = h
        .app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    h.key(KeyCode::F11);
    let mode = h.app.world().get::<Window>(window).unwrap().mode;
    assert!(matches!(mode, WindowMode::Fullscreen(..)), "{mode:?}");
    h.key(KeyCode::F11);
    let mode = h.app.world().get::<Window>(window).unwrap().mode;
    assert!(matches!(mode, WindowMode::Windowed), "{mode:?}");
}

/// F12 (and the fallback N) starts a new empty map: only the highway stub is left.
#[test]
fn f12_and_n_start_a_new_empty_map() {
    for key in [KeyCode::F12, KeyCode::KeyN] {
        let mut h = Harness::new();
        h.key(KeyCode::F7);
        assert!(h.segments() > 1);
        h.key(key);
        assert_eq!(h.segments(), 1, "only the highway stub");
        assert_eq!(h.sim(|sim| sim.externals.points.len()), 1);
        assert!(h.status().contains("> New empty map"), "{}", h.status());
    }
}
