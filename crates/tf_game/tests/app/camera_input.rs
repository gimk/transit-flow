//! Key bindings, the stepped zoom, panning, focus requests and the pointer. These cover the
//! camera's behaviour.

use std::time::Duration;

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use tf_game::camera::{MAP_SIZE, MapCamera, ZOOM_LEVELS, bevy_to_sim, nearest_level, sim_to_bevy};
use tf_game::events::{CameraFocusRequested, EventsPlugin};
use tf_game::input::{Action, GameInputPlugin, KeyBindings, Pointer};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, EventsPlugin))
        .add_plugins((GameInputPlugin, tf_game::camera::MapCameraPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
    app.world_mut().spawn(MapCamera::default());
    app.update();
    app
}

fn camera(app: &mut App) -> MapCamera {
    app.world_mut()
        .query::<&MapCamera>()
        .single(app.world())
        .unwrap()
        .clone()
}

fn set_camera(app: &mut App, f: impl FnOnce(&mut MapCamera)) {
    let mut q = app.world_mut().query::<&mut MapCamera>();
    let mut cam = q.single_mut(app.world_mut()).unwrap();
    f(&mut cam);
}

fn key(app: &mut App, code: KeyCode, state: ButtonState) {
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
}

#[test]
fn bindings_are_physical_keys() {
    let b = KeyBindings::default();
    assert_eq!(
        b.keys(Action::CameraPanLeft),
        &[KeyCode::KeyA, KeyCode::ArrowLeft]
    );
    assert_eq!(b.keys(Action::ToolZebra), &[KeyCode::KeyC]);
    assert_eq!(b.keys(Action::ToolCancel), &[KeyCode::Escape]);
    assert_eq!(b.keys(Action::ToggleFullscreen), &[KeyCode::F11]);
    // Every action has a key, and no key triggers two actions.
    for action in Action::ALL {
        assert!(!b.keys(action).is_empty(), "{action:?} has no key");
        for k in b.keys(action) {
            assert_eq!(b.actions_for(*k), vec![action], "{k:?} is shared");
        }
    }
}

#[test]
fn rebinding_changes_what_triggers_an_action() {
    let mut b = KeyBindings::default();
    b.rebind(Action::ToolZebra, vec![KeyCode::KeyZ]);
    assert!(b.actions_for(KeyCode::KeyC).is_empty());
    assert_eq!(b.actions_for(KeyCode::KeyZ), vec![Action::ToolZebra]);
}

#[test]
fn zoom_steps_clamp_at_both_ends() {
    let mut cam = MapCamera::default();
    let middle = cam.viewport / 2.0;
    for _ in 0..20 {
        cam.zoom_by_levels(1, middle);
    }
    assert_eq!(cam.zoom, *ZOOM_LEVELS.last().unwrap());
    for _ in 0..20 {
        cam.zoom_by_levels(-1, middle);
    }
    assert_eq!(cam.zoom, ZOOM_LEVELS[0]);
    cam.zoom_by_levels(1, middle);
    assert_eq!(cam.zoom, ZOOM_LEVELS[1]);
    assert_eq!(nearest_level(0.9), 4);
}

#[test]
fn zoom_keeps_the_point_under_the_cursor_fixed() {
    let mut cam = MapCamera::default();
    let cursor = Vec2::new(1000.0, 200.0);
    let before = cam.screen_to_world(cursor);
    cam.zoom_by_levels(2, cursor);
    assert_eq!(cam.zoom, ZOOM_LEVELS[6]);
    assert!(cam.screen_to_world(cursor).distance(before) < 0.01);
    assert!(cam.world_to_screen(before).distance(cursor) < 0.01);
}

#[test]
fn pinch_moves_one_level_per_step() {
    let mut cam = MapCamera::default();
    let middle = cam.viewport / 2.0;
    cam.pinch(1.1, middle);
    assert_eq!(cam.zoom, 1.0, "a small pinch is not a step yet");
    cam.pinch(1.1, middle);
    assert_eq!(cam.zoom, 1.5);
    cam.pinch(0.5, middle);
    assert!(cam.zoom < 1.0);
}

#[test]
fn the_camera_stays_on_the_map() {
    let mut cam = MapCamera::default();
    cam.move_by(Vec2::splat(1e7));
    assert_eq!(cam.center, Vec2::splat(MAP_SIZE));
    cam.focus_on(Vec2::splat(-50.0));
    assert_eq!(cam.center, Vec2::ZERO);
}

#[test]
fn starts_in_the_middle_of_the_map() {
    assert_eq!(MapCamera::default().center, Vec2::new(4096.0, 4096.0));
}

#[test]
fn keys_pan_at_pan_speed_whatever_the_layout() {
    let mut app = app();
    let start = camera(&mut app).center;
    key(&mut app, KeyCode::KeyD, ButtonState::Pressed);
    app.update();
    app.update();
    let moved = camera(&mut app).center - start;
    // Two frames of 100 ms at 900 px/s on screen, zoom 1.
    assert!((moved.x - 180.0).abs() < 1.0, "{moved}");
    assert_eq!(moved.y, 0.0);
    key(&mut app, KeyCode::KeyD, ButtonState::Released);
    key(&mut app, KeyCode::ArrowUp, ButtonState::Pressed);
    app.update();
    app.update();
    let moved = camera(&mut app).center - start;
    assert!(moved.y < -90.0, "up is -y in the sim: {moved}");
}

#[test]
fn panning_is_slower_in_world_units_when_zoomed_in() {
    let mut app = app();
    set_camera(&mut app, |c| c.zoom = 4.0);
    let start = camera(&mut app).center;
    key(&mut app, KeyCode::KeyS, ButtonState::Pressed);
    app.update();
    app.update();
    let moved = camera(&mut app).center - start;
    assert!((moved.y - 45.0).abs() < 1.0, "{moved}");
}

#[test]
fn diagonal_panning_is_not_faster() {
    let mut app = app();
    let start = camera(&mut app).center;
    key(&mut app, KeyCode::KeyD, ButtonState::Pressed);
    key(&mut app, KeyCode::KeyS, ButtonState::Pressed);
    app.update();
    app.update();
    let moved = camera(&mut app).center - start;
    assert!((moved.length() - 180.0).abs() < 1.0, "{moved}");
}

#[test]
fn a_wheel_notch_zooms_and_trackpad_pixels_pan() {
    let mut app = app();
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: 1.0,
        window: Entity::PLACEHOLDER,
        phase: TouchPhase::Moved,
    });
    app.update();
    assert_eq!(camera(&mut app).zoom, 1.5);
    let before = camera(&mut app).center;
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Pixel,
        x: 0.0,
        y: -30.0,
        window: Entity::PLACEHOLDER,
        phase: TouchPhase::Moved,
    });
    app.update();
    let moved = camera(&mut app).center - before;
    assert!((moved.y - 20.0).abs() < 0.01, "{moved}");
    assert_eq!(camera(&mut app).zoom, 1.5);
}

#[test]
fn a_focus_request_centres_the_camera() {
    let mut app = app();
    app.world_mut()
        .write_message(CameraFocusRequested(Vec2::new(1000.0, 2000.0)));
    app.update();
    assert_eq!(camera(&mut app).center, Vec2::new(1000.0, 2000.0));
    app.world_mut()
        .write_message(CameraFocusRequested(Vec2::new(-5.0, 99999.0)));
    app.update();
    assert_eq!(camera(&mut app).center, Vec2::new(0.0, 8192.0), "clamped");
}

#[test]
fn the_bevy_transform_and_projection_follow_the_camera() {
    let mut app = app();
    set_camera(&mut app, |c| {
        c.focus_on(Vec2::new(1000.0, 2000.0));
        c.zoom = 2.0;
    });
    app.update();
    let mut q = app.world_mut().query::<(&Transform, &Projection)>();
    let (t, p) = q.single(app.world()).unwrap();
    assert_eq!(
        t.translation.truncate(),
        sim_to_bevy(Vec2::new(1000.0, 2000.0))
    );
    assert_eq!(
        bevy_to_sim(t.translation.truncate()),
        Vec2::new(1000.0, 2000.0)
    );
    let Projection::Orthographic(o) = p else {
        panic!()
    };
    assert_eq!(o.scale, 0.5);
}

#[test]
fn pointer_world_from_a_known_camera() {
    let mut cam = MapCamera::default();
    cam.center = Vec2::new(1000.0, 500.0);
    cam.zoom = 2.0;
    cam.viewport = Vec2::new(800.0, 600.0);
    // The middle of the screen is the camera centre; 100 px right is 50 world px.
    assert_eq!(cam.screen_to_world(Vec2::new(400.0, 300.0)), cam.center);
    let world = cam.screen_to_world(Vec2::new(500.0, 300.0));
    assert_eq!(world, Vec2::new(1050.0, 500.0));
    let mut pointer = Pointer::default();
    pointer.set_world(world);
    assert!(pointer.in_window);
}

#[test]
fn pointer_buttons_follow_the_mouse() {
    let mut app = app();
    app.world_mut()
        .write_message(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window: Entity::PLACEHOLDER,
        });
    app.update();
    let p = app.world().resource::<Pointer>();
    assert!(p.left.pressed && p.left.just_pressed && !p.right.pressed);
    // Tests drive the position directly; without a window the update leaves it.
    app.world_mut()
        .resource_mut::<Pointer>()
        .set_world(Vec2::new(70.0, 10.0));
    app.update();
    assert_eq!(
        app.world().resource::<Pointer>().world,
        Vec2::new(70.0, 10.0)
    );
}
