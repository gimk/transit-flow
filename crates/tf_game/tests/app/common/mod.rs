//! The headless app the integration tests drive: the game plugins with the sim stepped
//! inline (no worker thread), keys, the pointer and mouse buttons sent as the window would
//! send them, and the status line read back as text. The test modules of `app` use it
//! through `crate::common`.
//!
//! All tests run as threads of one process, so process-global state is shared here:
//! [`data_dir`] sets `TRANSIT_FLOW_DATA_DIR` once, and [`save_lock`] serializes the tests
//! that write the quicksave in it.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use tf_game::input::Pointer;
use tf_game::plugin::GamePlugin;
use tf_game::runner::SimHandle;
use tf_game::status::StatusLineText;
use tf_game::tools::ToolManager;
use tf_game::views::ViewsPlugin;
use tf_game::views::grid::ShowGrid;
use tf_sim::Simulation;
use tf_sim::grid;
use tf_sim::roads::RoadType;

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

/// Held for a whole test that saves or loads the quicksave through the app: those tests
/// share [`data_dir`] and the file name `quicksave.json`. Starts from an empty folder.
pub fn save_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    // A failed test poisons the lock; the next test can still use it.
    let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let dir = data_dir();
    let _ = std::fs::remove_dir_all(dir);
    guard
}

pub fn local() -> &'static RoadType {
    RoadType::load_by_id("local_2lane").unwrap()
}

/// A grid point a few cells away from the map centre.
pub fn grid_point(dx: i32, dy: i32) -> Vec2 {
    grid::snap_to_point(grid::map_rect().center()) + Vec2::new(dx as f32, dy as f32) * 64.0
}

impl Harness {
    pub fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::input::InputPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<ColorMaterial>()
        .add_plugins((GamePlugin { threaded: false }, ViewsPlugin));
        app.update();
        let mut harness = Self { app };
        // Start from an empty map without the highway.
        harness.sim(|sim| *sim = tf_sim::Simulation::new());
        harness.frames(2);
        harness
    }

    /// The app in play mode on `level_id` (`--play`): the old game asleep, the level loaded.
    /// Reads the design file of the level, so a test that uses it holds `save_lock`.
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
            GamePlugin { threaded: false },
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

    /// Runs `f` on the simulation (waits for the worker, if any).
    pub fn sim<R>(&mut self, f: impl FnOnce(&mut tf_sim::Simulation) -> R) -> R {
        let mut handle = self.app.world_mut().resource_mut::<SimHandle>();
        f(handle.get_mut())
    }

    /// Builds a road directly, like a scenario does.
    pub fn road(&mut self, from: Vec2, to: Vec2) {
        self.sim(|sim| {
            sim.call(|sim, out| {
                sim.roads.add_road(from, to, local(), 0, out);
            })
        });
        self.frames(2);
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

    pub fn active_tool(&self) -> Option<&'static str> {
        self.app.world().resource::<ToolManager>().active_id()
    }

    pub fn tool_variant(&self) -> Option<String> {
        self.app.world().resource::<ToolManager>().variant()
    }

    pub fn hint(&self) -> String {
        self.app
            .world()
            .resource::<ToolManager>()
            .hint()
            .to_string()
    }

    pub fn grid_shown(&self) -> bool {
        self.app.world().resource::<ShowGrid>().0
    }

    pub fn segments(&mut self) -> usize {
        self.sim(|sim| sim.roads.segments.len())
    }

    /// Messages of type `M` written since the last call (reads them all).
    pub fn messages<M: Message + Clone>(&mut self) -> Vec<M> {
        let cursor = self.app.world_mut().resource_mut::<Messages<M>>();
        cursor.iter_current_update_messages().cloned().collect()
    }

    /// Takes the simulation out, runs `f` on it (stepping, changing the clock ...) and puts
    /// it back, which also resyncs the snapshots the status line reads.
    pub fn run_sim(&mut self, f: impl FnOnce(&mut Simulation)) {
        let mut handle = self.app.world_mut().resource_mut::<SimHandle>();
        let mut sim = std::mem::replace(handle.get_mut(), Simulation::new());
        f(&mut sim);
        handle.replace(sim);
        self.frames(2);
    }

    /// Presses and releases `code`, collecting the messages of type `M` written while the
    /// keys were handled (the frames after each state change).
    pub fn press_collect<M: Message + Clone>(&mut self, code: KeyCode) -> Vec<M> {
        let mut collected = Vec::new();
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
            collected.extend(self.messages::<M>());
        }
        self.frames(1);
        collected.extend(self.messages::<M>());
        collected
    }

    /// The text of the status line.
    pub fn status(&mut self) -> String {
        let world = self.app.world_mut();
        let mut lines = world.query_filtered::<&Text, With<StatusLineText>>();
        let line = lines.single(world).expect("one status line");
        line.0.clone()
    }

    /// The tool manager's active tool, for tests that check the status line's tool.
    pub fn tools(&self) -> &ToolManager {
        self.app.world().resource::<ToolManager>()
    }
}
