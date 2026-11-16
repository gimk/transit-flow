//! The runner: stepping through the worker thread, interpolated snapshots, events, and
//! quicksave round trips.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::prelude::*;
use std::time::{Duration, Instant};
use tf_game::events::{
    HourChanged, NotificationPosted, RoadNetworkChanged, SimulationReset, SpeedChanged,
};
use tf_game::game::{self, QUICKSAVE_FILE};
use tf_game::plugin::GamePlugin;
use tf_game::runner::{RunnerEvent, SimHandle};
use tf_sim::Simulation;
use tf_sim::debug::scenarios;

const FRAME: f64 = 1.0 / 60.0;

fn town() -> Simulation {
    let mut sim = Simulation::new();
    scenarios::build_town(&mut sim);
    sim
}

/// Runs frames until the render clock passes `until` sim seconds, checking the view on
/// every frame. Returns all events.
fn run_until(handle: &mut SimHandle, until: f64) -> Vec<RunnerEvent> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut events = Vec::new();
    let mut last_from = handle.view_from.time;
    while handle.render_time() < until {
        assert!(Instant::now() < deadline, "the sim stopped advancing");
        events.extend(handle.advance(FRAME));
        assert!(
            (0.0..=1.0).contains(&handle.view_alpha),
            "alpha {}",
            handle.view_alpha
        );
        assert!(handle.view_from.time <= handle.view_to.time);
        assert!(handle.view_from.time >= last_from, "view_from goes back");
        last_from = handle.view_from.time;
        assert!(
            handle.render_time() >= handle.view_from.time
                && handle.render_time() <= handle.view_to.time.max(handle.view_from.time) + 1e-9
        );
        std::thread::sleep(Duration::from_micros(200));
    }
    events
}

#[test]
fn the_threaded_runner_steps_and_interpolates() {
    let mut handle = SimHandle::new(town(), true);
    assert!(handle.is_threaded());
    let events = run_until(&mut handle, 3.0);
    assert!(handle.view_from.time >= 2.5 && handle.view_to.time >= handle.view_from.time);
    assert!(
        handle.view_to.time - handle.view_from.time <= 0.05 + 1e-9,
        "one tick apart"
    );
    assert!(events.contains(&RunnerEvent::Reset));
    // The tool side: get_mut waits for the batch and gives the sim back, in sync.
    let ticks = handle.get_mut().tick_count;
    assert!(f64::from(ticks as u32) * Simulation::TICK_DT >= handle.view_from.time);
    // Paused: the render clock stands still and no batch starts.
    handle.set_speed(0);
    let frozen = handle.render_time();
    for _ in 0..10 {
        handle.advance(FRAME);
    }
    assert_eq!(handle.render_time(), frozen);
    assert!(handle.is_paused());
}

#[test]
fn the_inline_runner_matches_a_plain_stepped_sim() {
    let mut handle = SimHandle::new(town(), false);
    run_until(&mut handle, 5.0);
    let ticks = handle.get_mut().tick_count;
    let mut plain = town();
    for _ in 0..ticks {
        plain.step();
    }
    assert_eq!(handle.get_mut().to_dict(), plain.to_dict());
    assert_eq!(
        handle.view_to.vehicle_count,
        plain
            .vehicles
            .vehicles
            .len()
            .min(handle.view_to.vehicle_count)
    );
}

#[test]
fn speed_scales_the_render_clock() {
    let mut handle = SimHandle::new(town(), false);
    handle.set_speed(3);
    assert_eq!(handle.multiplier(), 4);
    run_until(&mut handle, 4.0);
    // 4x: about 1 s of real time (60 frames) per 4 sim seconds.
    handle.set_speed(1);
    let start = handle.render_time();
    for _ in 0..60 {
        handle.advance(FRAME);
    }
    assert!((handle.render_time() - start - 1.0).abs() < 0.1);
    handle.toggle_pause();
    assert_eq!(handle.speed_index(), 0);
    handle.toggle_pause();
    assert_eq!(handle.speed_index(), 1, "resumes at the last running speed");
}

#[test]
fn the_runner_relays_hours_and_road_edits() {
    let mut handle = SimHandle::new(Simulation::new(), false);
    handle.advance(FRAME);
    let before = handle.roads().version;
    handle.get_mut().call(|sim, out| {
        sim.roads.add_road(
            glam::Vec2::new(0.0, 64.0),
            glam::Vec2::new(640.0, 64.0),
            tf_sim::roads::RoadType::load_by_id("local_2lane").unwrap(),
            0,
            out,
        );
    });
    let events = handle.advance(FRAME);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, RunnerEvent::RoadNetworkChanged { .. }))
    );
    let roads = handle.roads();
    assert!(roads.version > before);
    assert!(!roads.roads.segments.is_empty());
    // A game hour is 60 game minutes = at 1 game minute per... run long enough.
    let events = run_until(&mut handle, 60.0);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, RunnerEvent::HourChanged(_)))
    );
}

#[test]
fn replacing_the_simulation_resets_the_view() {
    let mut handle = SimHandle::new(town(), true);
    run_until(&mut handle, 1.0);
    handle.replace(Simulation::new());
    assert_eq!(handle.view_from.time, 0.0);
    assert_eq!(handle.render_time(), 0.0);
    assert!(handle.advance(FRAME).contains(&RunnerEvent::Reset));
    assert!(handle.get_mut().tick_count <= 4);
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("transit-flow-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn save_then_load_round_trips() {
    let dir = temp_dir("roundtrip");
    let path = dir.join(QUICKSAVE_FILE);
    let mut handle = SimHandle::new(town(), true);
    run_until(&mut handle, 2.0);
    let saved = handle.get_mut().to_dict();
    game::save_game(&mut handle, &path).unwrap();
    // Start something else, then load.
    handle.replace(Simulation::new());
    assert_ne!(handle.get_mut().to_dict(), saved);
    game::load_game(&mut handle, &path).unwrap();
    assert_eq!(handle.get_mut().to_dict(), saved);
    assert!(handle.advance(FRAME).contains(&RunnerEvent::Reset));
    // Not found / not a save.
    assert!(matches!(
        game::load_game(&mut handle, &dir.join("nothing.json")),
        Err(game::SaveError::NotFound)
    ));
    std::fs::write(dir.join("bad.json"), "{nope").unwrap();
    assert!(matches!(
        game::load_game(&mut handle, &dir.join("bad.json")),
        Err(game::SaveError::Parse)
    ));
    std::fs::write(dir.join("old.json"), "{\"version\": 0}").unwrap();
    assert!(matches!(
        game::load_game(&mut handle, &dir.join("old.json")),
        Err(game::SaveError::Invalid)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The whole app wiring: keys F5 / F9 / space / 1-3 drive the runner, and messages come
/// out the other side.
#[test]
fn keys_drive_the_game_through_the_plugin() {
    // The shared save folder; the lock keeps the other quicksave test out of it.
    let _lock = crate::common::save_lock();
    let dir = crate::common::data_dir();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::input::InputPlugin))
        .add_plugins(GamePlugin { threaded: true });
    app.update();
    let reset = |app: &mut App| {
        app.world_mut()
            .resource_mut::<Messages<SimulationReset>>()
            .drain()
            .count()
    };
    assert_eq!(reset(&mut app), 1, "the new game announces itself");

    let press = |app: &mut App, key: KeyCode| {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code: key,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        }
    };

    press(&mut app, KeyCode::Digit3);
    assert_eq!(app.world().resource::<SimHandle>().multiplier(), 4);
    press(&mut app, KeyCode::Space);
    assert!(app.world().resource::<SimHandle>().is_paused());
    assert_eq!(
        app.world_mut()
            .resource_mut::<Messages<SpeedChanged>>()
            .drain()
            .count(),
        2
    );

    // Give the empty map a road, save, change the map, load.
    app.world_mut()
        .resource_mut::<SimHandle>()
        .get_mut()
        .call(|sim, out| {
            sim.roads.add_road(
                glam::Vec2::new(0.0, 640.0),
                glam::Vec2::new(640.0, 640.0),
                tf_sim::roads::RoadType::load_by_id("local_2lane").unwrap(),
                0,
                out,
            );
        });
    app.update();
    assert!(
        app.world_mut()
            .resource_mut::<Messages<RoadNetworkChanged>>()
            .drain()
            .count()
            > 0
    );
    let saved = app
        .world_mut()
        .resource_mut::<SimHandle>()
        .get_mut()
        .to_dict();
    press(&mut app, KeyCode::F5);
    assert!(dir.join(QUICKSAVE_FILE).exists());
    app.world_mut()
        .resource_mut::<SimHandle>()
        .replace(Simulation::new());
    app.update();
    assert_ne!(
        app.world_mut()
            .resource_mut::<SimHandle>()
            .get_mut()
            .to_dict(),
        saved
    );
    press(&mut app, KeyCode::F9);
    assert_eq!(
        app.world_mut()
            .resource_mut::<SimHandle>()
            .get_mut()
            .to_dict(),
        saved
    );
    let notes: Vec<String> = app
        .world_mut()
        .resource_mut::<Messages<NotificationPosted>>()
        .drain()
        .map(|n| n.0)
        .collect();
    assert_eq!(notes, ["Game saved", "Game loaded"]);
    assert!(
        app.world_mut()
            .resource_mut::<Messages<HourChanged>>()
            .drain()
            .count()
            == 0
    );
    let _ = std::fs::remove_dir_all(dir);
}
