//! The game-level commands of `core/game.gd` that aren't about stepping: saving and
//! loading, new maps, scenarios, debug traffic. Plain functions over `SimHandle`, so tools
//! and tests can call them; messages for the player (notifications, camera focus) go
//! through `Announce`.

use std::path::{Path, PathBuf};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use glam::Vec2;
use serde::Serialize;
use tf_sim::Simulation;
use tf_sim::debug::scenarios;
use tf_sim::grid;

use crate::events::{CameraFocusRequested, NotificationPosted};
use crate::runner::SimHandle;

/// `Game.STRESS_TRAFFIC_STEPS`: vehicle counts the F6 debug key cycles through.
pub const STRESS_TRAFFIC_STEPS: [usize; 5] = [0, 250, 500, 1000, 2000];
pub const QUICKSAVE_FILE: &str = "quicksave.json";

/// Messages for the player.
#[derive(SystemParam)]
pub struct Announce<'w> {
    notifications: MessageWriter<'w, NotificationPosted>,
    camera: MessageWriter<'w, CameraFocusRequested>,
}

impl Announce<'_> {
    pub fn notify(&mut self, text: impl Into<String>) {
        self.notifications.write(NotificationPosted(text.into()));
    }

    pub fn focus(&mut self, world_position: Vec2) {
        self.camera.write(CameraFocusRequested(world_position));
    }
}

/// Where saves go (Godot's `user://`): the platform's per-user data folder,
/// `$TRANSIT_FLOW_DATA_DIR` if set (tests).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("TRANSIT_FLOW_DATA_DIR") {
        return dir.into();
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home().map(|home| home.join("Library/Application Support"))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|home| home.join(".local/share")))
    };
    base.unwrap_or_else(|| PathBuf::from("."))
        .join("transit-flow")
}

pub fn quicksave_path() -> PathBuf {
    data_dir().join(QUICKSAVE_FILE)
}

#[derive(Debug)]
pub enum SaveError {
    NotFound,
    Io(std::io::Error),
    Parse,
    /// Wrong save version or unreadable contents.
    Invalid,
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "no save found"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Parse => write!(f, "not valid JSON"),
            Self::Invalid => write!(f, "unsupported save"),
        }
    }
}

/// Writes the sim to `path` as `Simulation::to_dict` JSON: Godot's format (sorted keys,
/// tab indent), so either build loads the other's saves.
pub fn save_game(handle: &mut SimHandle, path: &Path) -> Result<(), SaveError> {
    let data = handle.get_mut().to_dict();
    let mut bytes = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
    data.serialize(&mut serializer)
        .map_err(|_| SaveError::Invalid)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(SaveError::Io)?;
    }
    // Written beside the target and renamed, so a crash never leaves half a save.
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, bytes).map_err(SaveError::Io)?;
    std::fs::rename(&temp, path).map_err(SaveError::Io)
}

/// Replaces the running sim with the one saved at `path`.
pub fn load_game(handle: &mut SimHandle, path: &Path) -> Result<(), SaveError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(SaveError::NotFound);
        }
        Err(error) => return Err(SaveError::Io(error)),
    };
    let data = serde_json::from_str(&text).map_err(|_| SaveError::Parse)?;
    let sim = Simulation::from_dict(&data).ok_or(SaveError::Invalid)?;
    handle.replace(sim);
    Ok(())
}

/// Saves to the quicksave slot and tells the player how it went.
pub fn quick_save(handle: &mut SimHandle, announce: &mut Announce) {
    match save_game(handle, &quicksave_path()) {
        Ok(()) => announce.notify("Game saved"),
        Err(error) => announce.notify(format!("Save failed ({error})")),
    }
}

pub fn quick_load(handle: &mut SimHandle, announce: &mut Announce) {
    match load_game(handle, &quicksave_path()) {
        Ok(()) => announce.notify("Game loaded"),
        Err(error) => announce.notify(format!("Load failed ({error})")),
    }
}

/// An empty map with its highway connection (what a new game starts from).
pub fn empty_map() -> Simulation {
    let mut sim = Simulation::new();
    scenarios::add_highway(&mut sim);
    sim
}

/// Starts over on an empty map.
pub fn new_empty_game(handle: &mut SimHandle, announce: &mut Announce) {
    handle.replace(empty_map());
    let entry = handle.get_mut().externals.points.first().copied();
    if let Some(entry) = entry {
        announce.focus(entry);
    }
    announce.notify("New empty map");
}

/// Debug: everyone at home with a job leaves for work right away.
pub fn rush_hour_now(handle: &mut SimHandle, announce: &mut Announce) {
    handle.get_mut().citizens.depart_everyone_now(0.0);
    announce.notify("Rush hour: everyone at home leaves for work");
}

/// Steps the debug stress traffic through `STRESS_TRAFFIC_STEPS` (0, 250, 500 ...).
pub fn cycle_stress_traffic(handle: &mut SimHandle, announce: &mut Announce) {
    let stress = &mut handle.get_mut().stress;
    let at = STRESS_TRAFFIC_STEPS
        .iter()
        .position(|&count| count == stress.target_count);
    let step = at.map_or(0, |i| i + 1) % STRESS_TRAFFIC_STEPS.len();
    stress.target_count = STRESS_TRAFFIC_STEPS[step];
    let target = stress.target_count;
    announce.notify(format!("Stress traffic: {target} vehicles"));
}

/// Replaces the game with the "Bottleneck" playtest scenario (F7) and centres on it.
pub fn load_bottleneck_scenario(handle: &mut SimHandle, announce: &mut Announce) {
    let mut sim = Simulation::new();
    let origin = grid::world_to_cell(grid::map_rect().center());
    let info = scenarios::build_bottleneck(&mut sim, origin);
    handle.replace(sim);
    announce.focus(info.junction);
    announce.notify("Scenario: Bottleneck. Relieve the jammed main road (F4 shows congestion)");
}

/// The Bottleneck with its main road given bus lanes and a bus line along it.
pub fn load_bottleneck_bus_lanes_scenario(handle: &mut SimHandle, announce: &mut Announce) {
    let mut sim = Simulation::new();
    let origin = grid::world_to_cell(grid::map_rect().center());
    let info = scenarios::build_bottleneck(&mut sim, origin);
    scenarios::add_bottleneck_bus_lanes(&mut sim, origin);
    handle.replace(sim);
    announce.focus(info.junction);
    announce.notify("Scenario: Bottleneck with bus lanes. Compare Flow with plain F7");
}

/// Replaces the game with the "Town" scenario (F8): a complete small city that lives
/// through a whole day, starting just before the morning rush.
pub fn load_town_scenario(handle: &mut SimHandle, announce: &mut Announce) {
    let mut sim = Simulation::new();
    let info = scenarios::build_town(&mut sim);
    handle.replace(sim);
    announce.focus(info.center);
    announce.notify("Scenario: Town. Watch a full day; F4 shows congestion, L toggles lights");
}
