//! `GamePlugin`: the simulation runner, its messages and the keys that drive them. The
//! counterpart of the `Game` and `Events` autoloads.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::events::*;
use crate::game::{self, Announce};
use crate::runner::{RunnerEvent, SimHandle};

/// Keys (physical, as in `project.godot`'s `[input]`: `KeyCode` is the physical key).
/// M8.2 folds these into `KeyBindings`.
pub const KEY_PAUSE: KeyCode = KeyCode::Space;
pub const KEYS_SPEED: [KeyCode; 3] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3];
pub const KEY_QUICK_SAVE: KeyCode = KeyCode::F5;
pub const KEY_QUICK_LOAD: KeyCode = KeyCode::F9;
pub const KEY_STRESS_TRAFFIC: KeyCode = KeyCode::F6;
pub const KEY_BOTTLENECK: KeyCode = KeyCode::F7;
pub const KEY_TOWN: KeyCode = KeyCode::F8;

pub struct GamePlugin {
    /// Step the sim on a worker thread (see `SimHandle::new`).
    pub threaded: bool,
}

impl Default for GamePlugin {
    fn default() -> Self {
        Self {
            threaded: SimHandle::default_threaded(),
        }
    }
}

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        // A new game starts on an empty map with its highway (`Game._ready`).
        app.insert_resource(SimHandle::new(game::empty_map(), self.threaded))
            .add_plugins(EventsPlugin)
            .add_systems(PreUpdate, advance_sim)
            .add_systems(Update, (sim_keys, log_hours));
    }
}

/// Writers for every message the runner relays.
#[derive(SystemParam)]
struct Relay<'w> {
    reset: MessageWriter<'w, SimulationReset>,
    speed: MessageWriter<'w, SpeedChanged>,
    hour: MessageWriter<'w, HourChanged>,
    day: MessageWriter<'w, DayStarted>,
    roads: MessageWriter<'w, RoadNetworkChanged>,
    lots: MessageWriter<'w, LotsChanged>,
    zones: MessageWriter<'w, ZonesChanged>,
    externals: MessageWriter<'w, ExternalsChanged>,
}

/// `Game._process`: advances the sim by the frame's real time and relays what happened.
fn advance_sim(time: Res<Time<Real>>, mut handle: ResMut<SimHandle>, mut relay: Relay) {
    for event in handle.advance(time.delta_secs_f64()) {
        match event {
            RunnerEvent::Reset => {
                relay.reset.write(SimulationReset);
            }
            RunnerEvent::SpeedChanged {
                speed_index,
                multiplier,
            } => {
                relay.speed.write(SpeedChanged {
                    speed_index,
                    multiplier,
                });
            }
            RunnerEvent::HourChanged(hour) => {
                relay.hour.write(HourChanged(hour));
            }
            RunnerEvent::DayStarted(day) => {
                relay.day.write(DayStarted(day));
            }
            RunnerEvent::RoadNetworkChanged {
                node_ids,
                segment_ids,
            } => {
                relay.roads.write(RoadNetworkChanged {
                    node_ids,
                    segment_ids,
                });
            }
            RunnerEvent::LotsChanged => {
                relay.lots.write(LotsChanged);
            }
            RunnerEvent::ZonesChanged => {
                relay.zones.write(ZonesChanged);
            }
            RunnerEvent::ExternalsChanged => {
                relay.externals.write(ExternalsChanged);
            }
        }
    }
}

/// `Game._unhandled_input`'s sim keys.
fn sim_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut handle: ResMut<SimHandle>,
    mut announce: Announce,
) {
    if keys.just_pressed(KEY_PAUSE) {
        handle.toggle_pause();
    }
    for (i, &key) in KEYS_SPEED.iter().enumerate() {
        if keys.just_pressed(key) {
            handle.set_speed(i + 1);
        }
    }
    if keys.just_pressed(KEY_QUICK_SAVE) {
        game::quick_save(&mut handle, &mut announce);
    }
    if keys.just_pressed(KEY_QUICK_LOAD) {
        game::quick_load(&mut handle, &mut announce);
    }
    if keys.just_pressed(KEY_BOTTLENECK) {
        game::load_bottleneck_scenario(&mut handle, &mut announce);
    }
    if keys.just_pressed(KEY_TOWN) {
        game::load_town_scenario(&mut handle, &mut announce);
    }
    if keys.just_pressed(KEY_STRESS_TRAFFIC) {
        game::cycle_stress_traffic(&mut handle, &mut announce);
    }
}

/// Minimal sign of life until the views exist (M9): the time, once per game hour.
fn log_hours(mut hours: MessageReader<HourChanged>, handle: Res<SimHandle>) {
    for HourChanged(hour) in hours.read() {
        info!(
            "day {} {:02}:00, {} vehicles, flow {:.0}%",
            handle.view_to.day, hour, handle.view_to.vehicle_count, handle.view_to.flow_percent
        );
    }
}
