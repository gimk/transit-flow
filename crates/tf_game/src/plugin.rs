//! `GamePlugin`: the simulation runner, its messages and the keys that drive them. The
//! counterpart of the `Game` and `Events` autoloads.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::camera::MapCameraPlugin;
use crate::events::*;
use crate::game::{self, Announce};
use crate::input::{Action, ActionInput, GameInputPlugin};
use crate::level::LevelPlugin;
use crate::play::not_playing;
use crate::runner::{RunnerEvent, SimHandle};
use crate::status::StatusLinePlugin;
use crate::street_runner::StreetRunPlugin;
use crate::tools::ToolsPlugin;

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
            .add_plugins((
                EventsPlugin,
                GameInputPlugin,
                MapCameraPlugin,
                ToolsPlugin,
                StatusLinePlugin,
                LevelPlugin,
                StreetRunPlugin,
            ))
            .add_systems(PreUpdate, advance_sim)
            .add_systems(Update, sim_keys.run_if(not_playing));
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
    actions: ActionInput,
    mut handle: ResMut<SimHandle>,
    mut announce: Announce,
    mut overlays: MessageWriter<DebugOverlayToggleRequested>,
) {
    use Action::*;
    if actions.just_pressed(SimTogglePause) {
        handle.toggle_pause();
    }
    for (i, action) in [SimSpeed1, SimSpeed2, SimSpeed3].into_iter().enumerate() {
        if actions.just_pressed(action) {
            handle.set_speed(i + 1);
        }
    }
    // The overlays answer these with `DebugOverlayChanged` (M9).
    for (action, id) in [
        (ToggleLateness, "lateness"),
        (ToggleLaneOverlay, "lanes"),
        (ToggleHeatmap, "heatmap"),
    ] {
        if actions.just_pressed(action) {
            overlays.write(DebugOverlayToggleRequested(id.into()));
        }
    }
    if actions.just_pressed(QuickSave) {
        game::quick_save(&mut handle, &mut announce);
    }
    if actions.just_pressed(QuickLoad) {
        game::quick_load(&mut handle, &mut announce);
    }
    if actions.just_pressed(DebugLoadScenario) {
        game::load_bottleneck_scenario(&mut handle, &mut announce);
    }
    if actions.just_pressed(DebugLoadBusLanesScenario) {
        game::load_bottleneck_bus_lanes_scenario(&mut handle, &mut announce);
    }
    if actions.just_pressed(DebugLoadTown) {
        game::load_town_scenario(&mut handle, &mut announce);
    }
    if actions.just_pressed(DebugNewEmptyMap) {
        game::new_empty_game(&mut handle, &mut announce);
    }
    if actions.just_pressed(DebugRushHour) {
        game::rush_hour_now(&mut handle, &mut announce);
    }
    if actions.just_pressed(DebugStressTraffic) {
        game::cycle_stress_traffic(&mut handle, &mut announce);
    }
}
