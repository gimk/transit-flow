//! The headless app the integration tests drive: the game plugins (the run steps inline, no
//! worker thread, unless a test asks for one), keys, the pointer and mouse buttons sent as
//! the window would send them. The test modules of `app` use it through `crate::common`.
//!
//! All tests run as threads of one process, so process-global state is shared here:
//! [`data_dir`] sets `TRANSIT_FLOW_DATA_DIR` once, and [`save_lock`] serializes the tests
//! that write the design files in it.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use tf_game::input::Pointer;
use tf_game::plugin::GamePlugin;
use tf_game::views::ViewsPlugin;

pub struct Harness {
    pub app: App,
}

/// The one save folder of the test process: a temp dir made and exported as
/// `TRANSIT_FLOW_DATA_DIR` on the first call. `set_var` runs exactly once, inside the
/// `OnceLock`, before any test that reads the variable gets its path.
pub fn data_dir() -> &'static Path {
    static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let name = format!("transit-flow-test-data-{}", std::process::id());
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        // Safe enough: this is the only place that sets the variable, and the tests that
        // read it hold `save_lock`, so no other test thread reads it at the same time.
        unsafe { std::env::set_var("TRANSIT_FLOW_DATA_DIR", &dir) };
        dir
    })
}

/// Held for a whole test that reads or writes the design files through the app: those tests
/// share [`data_dir`]. Starts from an empty folder.
pub fn save_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    // A failed test poisons the lock; the next test can still use it.
    let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let dir = data_dir();
    let _ = std::fs::remove_dir_all(dir);
    guard
}

impl Harness {
    /// The shell and the views, without play mode.
    pub fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::input::InputPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<ColorMaterial>()
        .add_plugins((GamePlugin, ViewsPlugin));
        app.update();
        let mut harness = Self { app };
        harness.frames(2);
        harness
    }

    /// The app in play mode on `level_id` (`--play`), the level loaded. Reads the design
    /// file of the level, so a test that uses it holds `save_lock`.
    pub fn play(level_id: &str) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::input::InputPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<ColorMaterial>()
        .add_plugins((
            GamePlugin,
            ViewsPlugin,
            tf_game::play::PlayPlugin {
                level: level_id.into(),
                threaded: false,
            },
        ));
        let mut harness = Self { app };
        harness.frames(3);
        harness
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update();
        }
    }

    pub fn key(&mut self, code: KeyCode) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            self.app.world_mut().write_message(KeyboardInput {
                key_code: code,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            self.frames(1);
        }
    }

    pub fn move_to(&mut self, world: Vec2) {
        self.app
            .world_mut()
            .resource_mut::<Pointer>()
            .set_world(world);
        self.frames(1);
    }

    pub fn button(&mut self, button: MouseButton, state: ButtonState) {
        self.app.world_mut().write_message(MouseButtonInput {
            button,
            state,
            window: Entity::PLACEHOLDER,
        });
        self.frames(1);
    }

    pub fn left(&mut self, state: ButtonState) {
        self.button(MouseButton::Left, state);
    }

    pub fn drag(&mut self, from: Vec2, to: Vec2) {
        self.move_to(from);
        self.left(ButtonState::Pressed);
        self.move_to(to);
        self.left(ButtonState::Released);
        self.frames(2);
    }

    pub fn click(&mut self, at: Vec2, button: MouseButton) {
        self.move_to(at);
        self.button(button, ButtonState::Pressed);
        self.button(button, ButtonState::Released);
    }

    /// Messages of type `M` written since the last call (reads them all).
    pub fn messages<M: Message + Clone>(&mut self) -> Vec<M> {
        let cursor = self.app.world_mut().resource_mut::<Messages<M>>();
        cursor.iter_current_update_messages().cloned().collect()
    }
}
