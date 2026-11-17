//! `MapCamera` (`view/map_camera.gd`): keyboard pan, middle-drag, wheel zoom toward the
//! cursor, trackpad pan and pinch, focus requests. Runs on real time, so it works while
//! the sim is paused.
//!
//! The camera's state is kept in sim coordinates (world pixels, y down, like Godot); a
//! system turns it into the Bevy transform (y up: `bevy_y = -sim_y`) and projection
//! scale (`1 / zoom`). Views convert with `sim_to_bevy` / `bevy_to_sim`.

use bevy::input::gestures::{PanGesture, PinchGesture};
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use tf_sim::grid;

use crate::events::CameraFocusRequested;
use crate::input::ActionInput;

/// World px to screen px. In Godot each was a power-of-two dot cell times 3 or 4 (so the
/// dot grid stayed 3-4 screen px apart); the dot display is dropped, but the steps are
/// kept: they are a comfortable progression and the same one players know.
pub const ZOOM_LEVELS: [f32; 9] = [0.25, 0.375, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0];
/// A pinch this much bigger (or smaller) moves one zoom level.
pub const PINCH_STEP: f32 = 1.15;
/// Screen pixels per second.
pub const PAN_SPEED: f32 = 900.0;
/// `InputEventPanGesture.delta` to screen pixels.
pub const PAN_GESTURE_SCALE: f32 = 16.0;

/// Sim position (y down) to the Bevy world (y up).
pub fn sim_to_bevy(p: Vec2) -> Vec2 {
    Vec2::new(p.x, -p.y)
}

pub fn bevy_to_sim(p: Vec2) -> Vec2 {
    Vec2::new(p.x, -p.y)
}

/// The index of the zoom level closest to `value` (in log space).
pub fn nearest_level(value: f32) -> usize {
    let dist = |i: usize| (ZOOM_LEVELS[i] / value).ln().abs();
    (0..ZOOM_LEVELS.len())
        .min_by(|a, b| dist(*a).total_cmp(&dist(*b)))
        .unwrap_or(0)
}

#[derive(Component, Clone, Debug)]
#[require(Camera2d)]
pub struct MapCamera {
    /// World position at the middle of the screen (sim pixels, y down).
    pub center: Vec2,
    pub zoom: f32,
    /// Size of the view in screen px (the window's logical size; tests set it).
    pub viewport: Vec2,
    pub pan_speed: f32,
    /// How far the current pinch has gone since the last zoom level.
    pinch: f32,
}

impl Default for MapCamera {
    fn default() -> Self {
        Self {
            center: {
                let r = grid::map_rect();
                r.position + r.size / 2.0
            },
            zoom: 1.0,
            viewport: Vec2::new(1280.0, 720.0),
            pan_speed: PAN_SPEED,
            pinch: 1.0,
        }
    }
}

impl MapCamera {
    /// The world point under a screen position (y down from the top-left corner).
    pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
        self.center + (screen - self.viewport / 2.0) / self.zoom
    }

    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        (world - self.center) * self.zoom + self.viewport / 2.0
    }

    pub fn move_by(&mut self, world_delta: Vec2) {
        self.center += world_delta;
        self.clamp_to_map();
    }

    pub fn focus_on(&mut self, world: Vec2) {
        self.center = world;
        self.clamp_to_map();
    }

    pub fn clamp_to_map(&mut self) {
        let rect = grid::map_rect();
        self.center = self.center.clamp(rect.position, rect.end());
    }

    /// Moves `steps` zoom levels in (out if negative), keeping the world point under
    /// `screen_pos` fixed on screen.
    pub fn zoom_by_levels(&mut self, steps: i32, screen_pos: Vec2) {
        let old = self.zoom;
        let level = (nearest_level(old) as i32 + steps).clamp(0, ZOOM_LEVELS.len() as i32 - 1);
        let new = ZOOM_LEVELS[level as usize];
        if (new - old).abs() <= 1e-6 * old {
            return;
        }
        let offset = screen_pos - self.viewport / 2.0;
        let under_cursor = self.center + offset / old;
        self.zoom = new;
        self.center = under_cursor - offset / new;
        self.clamp_to_map();
    }

    /// A pinch gesture of `factor` (1 = none) at `screen_pos`.
    pub fn pinch(&mut self, factor: f32, screen_pos: Vec2) {
        self.pinch *= factor;
        while self.pinch > PINCH_STEP {
            self.pinch /= PINCH_STEP;
            self.zoom_by_levels(1, screen_pos);
        }
        while self.pinch < 1.0 / PINCH_STEP {
            self.pinch *= PINCH_STEP;
            self.zoom_by_levels(-1, screen_pos);
        }
    }
}

/// Where the cursor is, or the middle of the screen (zoom keeps the centre then).
fn cursor_or_middle(window: Option<&Window>, camera: &MapCamera) -> Vec2 {
    window
        .and_then(Window::cursor_position)
        .unwrap_or(camera.viewport / 2.0)
}

/// `_process`: pan by keys.
fn pan_by_keys(time: Res<Time<Real>>, actions: ActionInput, mut cameras: Query<&mut MapCamera>) {
    let direction = actions.pan_vector();
    if direction == Vec2::ZERO {
        return;
    }
    for mut camera in &mut cameras {
        let delta = direction * camera.pan_speed * time.delta_secs() / camera.zoom;
        camera.move_by(delta);
    }
}

/// `_unhandled_input`: wheel, middle-drag, pinch and pan gestures.
#[allow(clippy::too_many_arguments)]
/// While set, the wheel does not zoom (a tool uses it). Play mode sets it.
#[derive(Resource, Default, Debug)]
pub struct WheelLock(pub bool);

#[allow(clippy::too_many_arguments)]
fn pointer_gestures(
    lock: Res<WheelLock>,
    mut wheel: MessageReader<MouseWheel>,
    mut motion: MessageReader<MouseMotion>,
    mut pinch: MessageReader<PinchGesture>,
    mut pan: MessageReader<PanGesture>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    mut cameras: Query<&mut MapCamera>,
) {
    let window = window.as_deref().copied();
    let drag: Vec2 = motion.read().map(|m| m.delta).sum();
    for mut camera in &mut cameras {
        let cursor = cursor_or_middle(window, &camera);
        for event in wheel.read() {
            match event.unit {
                // A wheel notch is one zoom level (a pressed wheel button in Godot).
                MouseScrollUnit::Line => {
                    if lock.0 {
                        continue;
                    }
                    let steps = event.y.signum() as i32 * (event.y != 0.0) as i32;
                    camera.zoom_by_levels(steps, cursor);
                }
                // Trackpad two-finger scroll arrives as pixels: it pans (macOS).
                MouseScrollUnit::Pixel => {
                    let delta = Vec2::new(-event.x, -event.y) / camera.zoom;
                    camera.move_by(delta);
                }
            }
        }
        if mouse.pressed(MouseButton::Middle) && drag != Vec2::ZERO {
            let delta = -drag / camera.zoom;
            camera.move_by(delta);
        }
        for PinchGesture(delta) in pinch.read() {
            camera.pinch(1.0 + delta, cursor);
        }
        for PanGesture(delta) in pan.read() {
            let world = *delta * PAN_GESTURE_SCALE / camera.zoom;
            camera.move_by(world);
        }
    }
}

fn focus_requests(
    mut requests: MessageReader<CameraFocusRequested>,
    mut cameras: Query<&mut MapCamera>,
) {
    for CameraFocusRequested(position) in requests.read() {
        for mut camera in &mut cameras {
            camera.focus_on(*position);
        }
    }
}

fn track_viewport(
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    mut cameras: Query<&mut MapCamera>,
) {
    let Some(window) = window else { return };
    for mut camera in &mut cameras {
        if camera.viewport != window.size() {
            camera.viewport = window.size();
        }
    }
}

/// Copies the camera state to the Bevy transform and projection.
fn apply_camera(mut cameras: Query<(&MapCamera, &mut Transform, &mut Projection)>) {
    for (camera, mut transform, mut projection) in &mut cameras {
        let position = sim_to_bevy(camera.center);
        transform.translation.x = position.x;
        transform.translation.y = position.y;
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scale = 1.0 / camera.zoom;
        }
    }
}

pub struct MapCameraPlugin;

impl Plugin for MapCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WheelLock>().add_systems(
            Update,
            (
                track_viewport,
                focus_requests,
                pointer_gestures,
                pan_by_keys,
                apply_camera,
            )
                .chain(),
        );
    }
}
