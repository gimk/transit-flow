//! Play mode (pivot plan §1 "The loop of one level", Phase 4): `transit-flow --play <level>`
//! opens a level of the street game in the window. The player designs (`Design`), starts the
//! run, watches it, reads the report, and designs again.
//!
//! - `PlayMode` (a resource, present only in play mode) switches the old city builder off:
//!   its keys, tools and status line carry the run condition `not_playing`. Its sim stays
//!   paused and empty.
//! - `PlayPhase` (a Bevy state): `Design` (edit the street, tools of `tools`), `Run` (the
//!   `StreetRunner` steps the run; no edits), `Report` (the run is over; the summary and,
//!   later, the score). `Enter` goes on in every phase.
//! - `PlayState` holds the level file, the `Design` and its redo stack, the active tool,
//!   the last message and the report. Edits go through `Street::apply` on the street of
//!   `LoadedLevel` (which the street view redraws); an edit the street refuses is not kept
//!   and its error is shown. Undo rebuilds the street from the level and the commands left.
//! - The design is saved to `<data dir>/designs/<level id>.json` when a run starts, and
//!   read when play mode opens (`file`).
//!
//! Where the next brief plugs in: new tools in `tools::PlayTool`; the score and the stars
//! in `ReportData::extra` (lines shown under the summary).

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use tf_sim::Simulation;
use tf_sim::street::run::{EndReason, RunOptions, StreetRun, Summary};
use tf_sim::street::{Design, DesignCommand, LevelFile, Street};

use crate::input::{Action, ActionInput, Pointer};
use crate::level::{LoadedLevel, PX_PER_M};
use crate::runner::SimHandle;
use crate::street_runner::{DEFAULT_SPEED_INDEX, StreetRunner};

pub mod file;
pub mod markers;
pub mod pick;
pub mod tools;
pub mod ui;

pub use tools::PlayTool;

/// The level being played; the resource exists only in play mode.
#[derive(Resource, Clone, Debug)]
pub struct PlayMode {
    pub level_id: String,
    /// Step the run on a worker thread (off in tests and screenshots).
    pub threaded: bool,
}

/// The run condition of the old game's systems: they sleep in play mode.
pub fn not_playing(mode: Option<Res<PlayMode>>) -> bool {
    mode.is_none()
}

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlayPhase {
    #[default]
    Design,
    Run,
    Report,
}

/// What the report panel shows once a run is over.
#[derive(Clone, Debug, PartialEq)]
pub struct ReportData {
    pub summary: Summary,
    /// More lines under the summary: the score and the star verdict go here (next brief).
    pub extra: Vec<String>,
}

#[derive(Resource, Debug)]
pub struct PlayState {
    pub level: LevelFile,
    pub design: Design,
    /// Commands taken back by undo, the last one first to come back.
    pub redo: Vec<DesignCommand>,
    pub tool: Option<PlayTool>,
    /// The last thing to tell the player: an edit refused, a file problem, a result.
    pub message: Option<String>,
    pub report: Option<ReportData>,
}

impl PlayState {
    /// A fresh state with an empty design.
    pub fn new(level: LevelFile) -> Self {
        let design = Design::new(&level.id);
        Self {
            level,
            design,
            redo: Vec::new(),
            tool: None,
            message: None,
            report: None,
        }
    }

    /// The street of `design` on the level; `None` if replaying it fails.
    pub fn build_street(&self) -> Result<Street, String> {
        Street::from_level_and_design(&self.level, &self.design).map_err(|error| error.to_string())
    }

    /// Applies `command` to `street`; kept in the design only if the street accepts it.
    /// Returns whether it was kept (the message tells the player either way).
    pub fn edit(&mut self, street: &mut Street, command: DesignCommand) -> bool {
        match street.apply(&command) {
            Ok(_) => {
                self.message = Some(PlayTool::done_text(&command));
                self.design.commands.push(command);
                self.redo.clear();
                true
            }
            Err(error) => {
                self.message = Some(format!("Not possible here: {error}."));
                false
            }
        }
    }

    /// Takes the last command back and returns the street without it.
    pub fn undo(&mut self) -> Option<Street> {
        let command = self.design.commands.pop()?;
        match self.build_street() {
            Ok(street) => {
                self.message = Some("Undone.".into());
                self.redo.push(command);
                Some(street)
            }
            Err(error) => {
                // Cannot happen for a prefix of a design that built; put it back.
                self.design.commands.push(command);
                self.message = Some(format!("Cannot undo: {error}"));
                None
            }
        }
    }

    /// Brings the last undone command back; `None` (with a message) if the street
    /// refuses it now.
    pub fn redo(&mut self, street: &mut Street) -> bool {
        let Some(command) = self.redo.pop() else {
            return false;
        };
        match street.apply(&command) {
            Ok(_) => {
                self.message = Some(format!("Redone. {}", PlayTool::done_text(&command)));
                self.design.commands.push(command);
                true
            }
            Err(error) => {
                self.message = Some(format!("Cannot redo: {error}."));
                false
            }
        }
    }
}

/// Opens the level: reads the design file (a bad one is told and ignored), builds the
/// street, and puts the old game to sleep.
fn start_play(
    mode: Res<PlayMode>,
    mut commands: Commands,
    mut loaded: ResMut<LoadedLevel>,
    mut handle: ResMut<SimHandle>,
) {
    let level = LevelFile::load(&mode.level_id).expect("play mode is given a known level");
    let mut state = PlayState::new(level);
    match file::load_design(&mode.level_id) {
        Ok(Some(design)) => state.design = design,
        Ok(None) => {}
        Err(error) => state.message = Some(format!("Design file not used: {error}")),
    }
    let street = state.build_street().or_else(|error| {
        // A design that no longer replays on the level: start empty and say why.
        state.message = Some(format!("Design file not used: {error}"));
        state.design = Design::new(&state.level.id);
        state.build_street()
    });
    loaded.street = Some(street.expect("an empty design builds on a shipped level"));
    // The old city builder: an empty map, paused.
    handle.replace(Simulation::new());
    handle.set_speed(0);
    commands.insert_resource(state);
}

/// Starts the run of the current design: saves the file, builds the run, hands it to the
/// runner. A level that does not build stays in `Design` with the reason shown.
pub fn begin_run(
    state: &mut PlayState,
    mode: &PlayMode,
    runner: &mut StreetRunner,
    next: &mut NextState<PlayPhase>,
) {
    if let Err(error) = file::save_design(&state.design) {
        state.message = Some(format!("Design not saved: {error}"));
    }
    match StreetRun::new(
        &state.level,
        &state.design,
        RunOptions::for_level(&state.level),
    ) {
        Ok(run) => {
            runner.start(run, mode.threaded);
            runner.set_speed(DEFAULT_SPEED_INDEX);
            state.report = None;
            state.tool = None;
            next.set(PlayPhase::Run);
        }
        Err(error) => state.message = Some(format!("The run cannot start: {error}")),
    }
}

/// Ends the run (it is over, or the player stopped it): the summary goes to the report.
fn end_run(state: &mut PlayState, runner: &mut StreetRunner, next: &mut NextState<PlayPhase>) {
    let summary = runner
        .get_mut()
        .map(|run| run.summary())
        .expect("a run is going when it ends");
    // A stopped run stays where it is.
    runner.set_speed(0);
    state.report = Some(ReportData {
        summary,
        extra: Vec::new(),
    });
    next.set(PlayPhase::Report);
}

/// Design phase: tools, undo and redo, and the start of the run.
#[allow(clippy::too_many_arguments)]
fn design_input(
    actions: ActionInput,
    keys: Res<ButtonInput<KeyCode>>,
    pointer: Res<Pointer>,
    mode: Res<PlayMode>,
    mut state: ResMut<PlayState>,
    mut loaded: ResMut<LoadedLevel>,
    mut runner: ResMut<StreetRunner>,
    mut next: ResMut<NextState<PlayPhase>>,
) {
    if actions.just_pressed(Action::ToolCancel) {
        state.tool = None;
    }
    for tool in PlayTool::ALL {
        if actions.just_pressed(tool.action()) {
            state.tool = if state.tool == Some(tool) {
                None
            } else {
                Some(tool)
            };
            state.message = None;
        }
    }

    let ctrl = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let undo = actions.just_pressed(Action::PlayUndo) || (ctrl && keys.just_pressed(KeyCode::KeyZ));
    let redo = ctrl && keys.just_pressed(KeyCode::KeyY);
    if undo && let Some(street) = state.undo() {
        loaded.street = Some(street);
    } else if redo {
        // Only a kept edit marks the level changed (the street view redraws on that, not
        // on every try), so the street is reached without change detection.
        let kept = loaded
            .bypass_change_detection()
            .street
            .as_mut()
            .is_some_and(|street| state.redo(street));
        if kept {
            loaded.set_changed();
        }
    }

    if pointer.left.just_pressed
        && pointer.in_window
        && let Some(tool) = state.tool
    {
        let at = pointer.world / PX_PER_M;
        let kept = loaded
            .bypass_change_detection()
            .street
            .as_mut()
            .is_some_and(|street| match tool.click(street, at) {
                Some(command) => state.edit(street, command),
                None => {
                    state.message = Some("Nothing to change there.".into());
                    false
                }
            });
        if kept {
            loaded.set_changed();
        }
    }

    if actions.just_pressed(Action::PlayConfirm) {
        begin_run(&mut state, &mode, &mut runner, &mut next);
    }
}

/// Run phase: pause and speed; `Enter` stops early; the end of the run opens the report.
fn run_input(
    actions: ActionInput,
    mut state: ResMut<PlayState>,
    mut runner: ResMut<StreetRunner>,
    mut next: ResMut<NextState<PlayPhase>>,
) {
    if actions.just_pressed(Action::SimTogglePause) {
        runner.toggle_pause();
    }
    for (i, action) in [
        Action::SimSpeed1,
        Action::SimSpeed2,
        Action::SimSpeed3,
        Action::PlaySpeed4,
    ]
    .into_iter()
    .enumerate()
    {
        if actions.just_pressed(action) {
            runner.set_speed(i + 1);
        }
    }
    if actions.just_pressed(Action::PlayConfirm) || runner.run_ended() {
        end_run(&mut state, &mut runner, &mut next);
    }
}

/// Report phase: `Enter` goes back to the design (the same design; the marks stay).
fn report_input(
    actions: ActionInput,
    mut runner: ResMut<StreetRunner>,
    mut state: ResMut<PlayState>,
    mut next: ResMut<NextState<PlayPhase>>,
) {
    if actions.just_pressed(Action::PlayConfirm) {
        runner.stop();
        state.report = None;
        state.message = None;
        next.set(PlayPhase::Design);
    }
}

/// The end of a run in words, for the report.
pub fn end_text(ended: Option<EndReason>) -> &'static str {
    match ended {
        Some(EndReason::Completed) => "the day is over",
        Some(EndReason::Death) => "a death stopped the run",
        None => "stopped early",
    }
}

pub struct PlayPlugin {
    pub level: String,
    pub threaded: bool,
}

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<StatesPlugin>() {
            app.add_plugins(StatesPlugin);
        }
        app.insert_resource(PlayMode {
            level_id: self.level.clone(),
            threaded: self.threaded,
        })
        .init_state::<PlayPhase>()
        .add_plugins((ui::PlayUiPlugin, markers::MarkersPlugin))
        .add_systems(Startup, start_play)
        .add_systems(
            Update,
            (
                design_input.run_if(in_state(PlayPhase::Design)),
                run_input.run_if(in_state(PlayPhase::Run)),
                report_input.run_if(in_state(PlayPhase::Report)),
            )
                .run_if(resource_exists::<PlayState>),
        );
    }
}
