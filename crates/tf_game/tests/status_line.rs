//! The status line (M10.3): the day and time, speed, Flow, traffic, the active tool with its
//! variant and hint, and the last notification, as one text line.

mod common;

use bevy::prelude::*;
use common::Harness;
use tf_game::capture::advance_to_hour;
use tf_game::events::NotificationPosted;
use tf_game::status::StatusNotice;

#[test]
fn status_line_shows_the_day_time_speed_and_flow() {
    let mut h = Harness::new();
    let line = h.status();
    assert!(line.starts_with("Day 1, 06:00"), "{line}");
    assert!(line.contains("1x"), "{line}");
    assert!(line.contains("Flow -"), "no trip has ended yet: {line}");
    assert!(line.contains("0 vehicles"), "{line}");
    assert!(line.contains("tick "), "{line}");

    h.key(KeyCode::Space);
    assert!(h.status().contains("paused"), "{}", h.status());
    h.key(KeyCode::Digit2);
    assert!(h.status().contains("2x"), "{}", h.status());
}

#[test]
fn status_line_names_the_active_tool_its_variant_and_hint() {
    let mut h = Harness::new();
    h.key(KeyCode::KeyR);
    let variant = h.tools().variant().expect("the road tool has a road type");
    let hint = h.tools().hint().to_string();
    assert!(!hint.is_empty(), "the road tool has a hint");
    let line = h.status();
    assert!(line.contains(&format!("Road ({variant})")), "{line}");
    assert!(line.contains(&hint.replace('·', "|")), "{line}");
    assert!(line.contains("Flow"), "{line}");
    assert!(line.starts_with("Day 1, 06:00"), "{line}");

    h.key(KeyCode::Escape);
    assert!(!h.status().contains("Road ("), "{}", h.status());
}

/// The day comes from the same snapshot as the time, so it changes exactly at 00:00. (The
/// hourly log read the day from a snapshot that could still be the old day.)
#[test]
fn the_day_changes_at_midnight_in_the_status_line() {
    let mut h = Harness::new();
    h.run_sim(|sim| advance_to_hour(sim, 23.0 + 59.0 / 60.0));
    assert!(h.status().starts_with("Day 1, 23:59"), "{}", h.status());

    h.run_sim(|sim| advance_to_hour(sim, 0.0));
    assert!(h.status().starts_with("Day 2, 00:00"), "{}", h.status());

    h.run_sim(|sim| advance_to_hour(sim, 0.5));
    assert!(h.status().starts_with("Day 2, 00:30"), "{}", h.status());
}

#[test]
fn a_notification_shows_on_the_line_and_then_clears() {
    let mut h = Harness::new();
    h.app
        .world_mut()
        .write_message(NotificationPosted("Test note".into()));
    h.frames(1);
    assert!(h.status().contains("> Test note"), "{}", h.status());

    // Godot's notifications last NOTIFICATION_SECONDS; push the timer out instead of waiting.
    h.app
        .world_mut()
        .resource_mut::<StatusNotice>()
        .seconds_left = 0.0;
    h.frames(1);
    assert!(!h.status().contains("Test note"), "{}", h.status());
}
