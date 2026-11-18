//! Input: `KeyBindings` (actions bound to physical keys) and `Pointer` (the cursor in world
//! space, which the play tools read instead of the window so tests can drive them).
//! `KeyCode` is the physical key, so WASD = ZQSD on AZERTY.

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::MapCamera;

/// Every input action of the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    CameraPanLeft,
    CameraPanRight,
    CameraPanUp,
    CameraPanDown,
    /// Space: pause the run.
    SimTogglePause,
    /// Keys 1 to 3: run speed x1, x2, x4.
    SimSpeed1,
    SimSpeed2,
    SimSpeed3,
    /// Esc: drop the active tool.
    ToolCancel,
    /// The play tools: C zebra, V speed limit, B bollards, L signals, X separator,
    /// N bicycle box, G leading bicycle green.
    ToolZebra,
    ToolSpeedLimit,
    ToolBollards,
    ToolSignals,
    ToolSeparator,
    ToolBicycleBox,
    ToolBicycleLead,
    ToggleFullscreen,
    /// Enter starts the run, ends it early, and leaves the report.
    PlayConfirm,
    /// Backspace. Ctrl+Z and Ctrl+Y are read from the raw keys by the play system: `Z` alone
    /// must stay unbound.
    PlayUndo,
    /// Key 4: run speed x8.
    PlaySpeed4,
}

impl Action {
    pub const ALL: [Action; 20] = [
        Action::CameraPanLeft,
        Action::CameraPanRight,
        Action::CameraPanUp,
        Action::CameraPanDown,
        Action::SimTogglePause,
        Action::SimSpeed1,
        Action::SimSpeed2,
        Action::SimSpeed3,
        Action::ToolCancel,
        Action::ToolZebra,
        Action::ToolSpeedLimit,
        Action::ToolBollards,
        Action::ToolSignals,
        Action::ToolSeparator,
        Action::ToolBicycleBox,
        Action::ToolBicycleLead,
        Action::ToggleFullscreen,
        Action::PlayConfirm,
        Action::PlayUndo,
        Action::PlaySpeed4,
    ];
}

/// Actions to physical keys.
#[derive(Resource, Clone, Debug)]
pub struct KeyBindings {
    keys: HashMap<Action, Vec<KeyCode>>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        use Action::*;
        use KeyCode::*;
        let table: [(Action, &[KeyCode]); 20] = [
            (CameraPanLeft, &[KeyA, ArrowLeft]),
            (CameraPanRight, &[KeyD, ArrowRight]),
            (CameraPanUp, &[KeyW, ArrowUp]),
            (CameraPanDown, &[KeyS, ArrowDown]),
            (SimTogglePause, &[Space]),
            (SimSpeed1, &[Digit1]),
            (SimSpeed2, &[Digit2]),
            (SimSpeed3, &[Digit3]),
            (ToolCancel, &[Escape]),
            (ToolZebra, &[KeyC]),
            (ToolSpeedLimit, &[KeyV]),
            (ToolBollards, &[KeyB]),
            (ToolSignals, &[KeyL]),
            (ToolSeparator, &[KeyX]),
            (ToolBicycleBox, &[KeyN]),
            (ToolBicycleLead, &[KeyG]),
            (ToggleFullscreen, &[F11]),
            (PlayConfirm, &[Enter, NumpadEnter]),
            (PlayUndo, &[Backspace]),
            (PlaySpeed4, &[Digit4]),
        ];
        Self {
            keys: table.into_iter().map(|(a, k)| (a, k.to_vec())).collect(),
        }
    }
}

impl KeyBindings {
    pub fn keys(&self, action: Action) -> &[KeyCode] {
        self.keys.get(&action).map_or(&[], Vec::as_slice)
    }

    /// Replaces the keys of an action.
    pub fn rebind(&mut self, action: Action, keys: Vec<KeyCode>) {
        self.keys.insert(action, keys);
    }

    /// The actions a key triggers.
    pub fn actions_for(&self, key: KeyCode) -> Vec<Action> {
        Action::ALL
            .into_iter()
            .filter(|a| self.keys(*a).contains(&key))
            .collect()
    }

    pub fn just_pressed(&self, input: &ButtonInput<KeyCode>, action: Action) -> bool {
        self.keys(action).iter().any(|k| input.just_pressed(*k))
    }

    pub fn pressed(&self, input: &ButtonInput<KeyCode>, action: Action) -> bool {
        self.keys(action).iter().any(|k| input.pressed(*k))
    }
}

/// What systems read instead of raw keys: `actions.just_pressed(Action::PlayConfirm)`.
#[derive(SystemParam)]
pub struct ActionInput<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    bindings: Res<'w, KeyBindings>,
}

impl ActionInput<'_> {
    pub fn just_pressed(&self, action: Action) -> bool {
        self.bindings.just_pressed(&self.keys, action)
    }

    pub fn pressed(&self, action: Action) -> bool {
        self.bindings.pressed(&self.keys, action)
    }

    /// `Input.get_vector`: x right, y down (screen directions), each axis -1, 0 or 1,
    /// then normalised so diagonals are not faster.
    pub fn pan_vector(&self) -> Vec2 {
        let axis = |neg, pos| self.pressed(pos) as i32 as f32 - self.pressed(neg) as i32 as f32;
        Vec2::new(
            axis(Action::CameraPanLeft, Action::CameraPanRight),
            axis(Action::CameraPanUp, Action::CameraPanDown),
        )
        .normalize_or_zero()
    }
}

/// The pointer in world space (sim pixels, y down). Updated every frame from the cursor;
/// tools read this, never the window. Tests set it with `set_world`.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Pointer {
    /// Cursor position in the sim's world pixels.
    pub world: Vec2,
    pub left: ButtonState3,
    pub right: ButtonState3,
    pub middle: ButtonState3,
    /// The cursor is inside the window.
    pub in_window: bool,
}

/// Held / just pressed / just released, for one mouse button.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ButtonState3 {
    pub pressed: bool,
    pub just_pressed: bool,
    pub just_released: bool,
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            world: Vec2::ZERO,
            left: default(),
            right: default(),
            middle: default(),
            in_window: false,
        }
    }
}

impl Pointer {
    /// Puts the pointer at a world position.
    pub fn set_world(&mut self, world: Vec2) {
        self.world = world;
        self.in_window = true;
    }

    pub fn button(&self, button: MouseButton) -> ButtonState3 {
        match button {
            MouseButton::Left => self.left,
            MouseButton::Right => self.right,
            MouseButton::Middle => self.middle,
            _ => default(),
        }
    }
}

fn state_of(input: &ButtonInput<MouseButton>, button: MouseButton) -> ButtonState3 {
    ButtonState3 {
        pressed: input.pressed(button),
        just_pressed: input.just_pressed(button),
        just_released: input.just_released(button),
    }
}

/// Refreshes the pointer. The cursor and viewport come from the primary window when
/// there is one; without a window (tests) only the buttons are read and `world` stays as
/// set.
pub fn update_pointer(
    mut pointer: ResMut<Pointer>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    camera: Option<Single<&MapCamera>>,
) {
    pointer.left = state_of(&mouse, MouseButton::Left);
    pointer.right = state_of(&mouse, MouseButton::Right);
    pointer.middle = state_of(&mouse, MouseButton::Middle);
    let (Some(window), Some(camera)) = (window, camera) else {
        return;
    };
    match window.cursor_position() {
        Some(cursor) => {
            let world = camera.screen_to_world(cursor);
            pointer.set_world(world);
        }
        None => pointer.in_window = false,
    }
}

/// F11: windowed <-> exclusive fullscreen (`Game.toggle_fullscreen`).
pub fn toggle_fullscreen(
    actions: ActionInput,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    use bevy::window::{MonitorSelection, VideoModeSelection, WindowMode};
    if !actions.just_pressed(Action::ToggleFullscreen) {
        return;
    }
    for mut window in &mut windows {
        window.mode = match window.mode {
            WindowMode::Windowed => {
                WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current)
            }
            _ => WindowMode::Windowed,
        };
    }
}

/// `KeyBindings`, `Pointer` and their systems.
pub struct GameInputPlugin;

impl Plugin for GameInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeyBindings>()
            .init_resource::<Pointer>()
            .add_systems(PreUpdate, update_pointer.after(bevy::input::InputSystems))
            .add_systems(Update, toggle_fullscreen);
    }
}
