//! Play mode (pivot plan §1 "The loop of one level", Phase 4): `transit-flow --play <level>`
//! opens a level of the street game in the window. The player designs (`Design`), starts the
//! run, watches it, reads the report, and designs again.
//!
//! - `PlayMode` (a resource) holds the level being played and whether the run is threaded.
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

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};

use tf_sim::street::run::{EndReason, RunOptions, StreetRun, Summary};
use tf_sim::street::score::{StageReport, stage_options, stage_report, stars_for_design};
use tf_sim::street::{Design, DesignCommand, EditError, LevelFile, Street};

use crate::camera::WheelLock;
use crate::input::{Action, ActionInput, Pointer};
use crate::level::{LoadedLevel, PX_PER_M};
use crate::street_runner::{DEFAULT_SPEED_INDEX, StreetRunner};

pub mod file;
pub mod inspect;
pub mod markers;
pub mod pick;
pub mod preview;
pub mod signals;
pub mod tools;
pub mod ui;

pub use tools::PlayTool;

/// The cursor is over one of the play panels, so a click is for the panel and not the map.
#[derive(Resource, Default, Debug)]
pub struct PointerOverUi(pub bool);

/// The level being played.
#[derive(Resource, Clone, Debug)]
pub struct PlayMode {
    pub level_id: String,
    /// Step the run on a worker thread (off in tests and screenshots).
    pub threaded: bool,
}

#[derive(States, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlayPhase {
    #[default]
    Design,
    /// The stages below the chosen one are played headless on this design first.
    Check,
    Run,
    Report,
}

/// What the report panel shows once a run is over (or a lower stage failed its check).
#[derive(Clone, Debug, PartialEq)]
pub struct ReportData {
    /// The counts of the run; `None` when the report is of a stage played headless.
    pub summary: Option<Summary>,
    /// Verdict, score, per-mode stats, reasons and incidents of the stage.
    pub stage: StageReport,
    /// More lines under the report (a run stopped early, a lower stage that failed).
    pub extra: Vec<String>,
}

/// What `stars_for_design` answers: the stars won and the report of each stage played.
pub type CheckAnswer = Result<(usize, Vec<StageReport>), EditError>;

/// The headless run of the stages below the one asked for, on a worker thread: the answer
/// of `stars_for_design` comes through the channel.
#[derive(Debug)]
pub struct StageCheck {
    /// The stage the player asked for (the check plays the ones below it).
    pub stage: usize,
    rx: Mutex<Receiver<CheckAnswer>>,
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
    /// What a click under the pointer would do or why it would fail (`preview`): shown in
    /// the info panel while designing.
    pub hover_note: Option<String>,
    pub report: Option<ReportData>,
    /// The stage selected, from 1 (pivot plan §6: each star is a load stage).
    pub stage: usize,
    /// The stars this design has won: the stages passed, from the first, in a row. Any
    /// edit sets it to 0, because a star belongs to a design.
    pub stars: usize,
    /// The most stars any design of this level has won: it unlocks the stages.
    pub best_stars: usize,
    /// The check of the lower stages, while it runs (`PlayPhase::Check`).
    pub check: Option<StageCheck>,
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
            hover_note: None,
            report: None,
            stage: 1,
            stars: 0,
            best_stars: 0,
            check: None,
        }
    }

    /// How many stages the level has (at least 1: a level without stages plays as one).
    pub fn stage_count(&self) -> usize {
        self.level.stages.len().max(1)
    }

    /// A stage can be chosen when the one below it was passed once, by any design.
    pub fn is_unlocked(&self, stage: usize) -> bool {
        (1..=self.stage_count()).contains(&stage) && stage <= self.best_stars + 1
    }

    /// Chooses `stage` if it is unlocked; else says why not.
    pub fn select_stage(&mut self, stage: usize) {
        if stage == self.stage || !(1..=self.stage_count()).contains(&stage) {
            return;
        }
        if self.is_unlocked(stage) {
            self.stage = stage;
            self.message = None;
        } else {
            self.message = Some(format!(
                "Stage {stage} is locked: pass stage {} first.",
                stage - 1
            ));
        }
    }

    /// The design changed: its stars are gone (and the stage list is the same).
    fn design_changed(&mut self) {
        self.stars = 0;
    }

    /// Records that this design passed `stage`, and saves the stars.
    fn stage_passed(&mut self, stage: usize) {
        self.stars = self.stars.max(stage);
        self.best_stars = self.best_stars.max(self.stars);
        self.save_stars();
    }

    pub fn save_stars(&mut self) {
        let record = file::StarsFile::new(&self.design, self.stars, self.best_stars);
        if let Err(error) = file::save_stars(&self.level.id, &record) {
            self.message = Some(format!("Stars not saved: {error}"));
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
                self.design_changed();
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
                self.design_changed();
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
                self.design_changed();
                true
            }
            Err(error) => {
                self.message = Some(format!("Cannot redo: {error}."));
                false
            }
        }
    }
}

/// Opens the level: reads the design file and the stars (a bad file is told and ignored),
/// and builds the street.
fn start_play(mode: Res<PlayMode>, mut commands: Commands, mut loaded: ResMut<LoadedLevel>) {
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
    // The stars of the saved design count only if it is the design that won them.
    match file::load_stars(&mode.level_id) {
        Ok(Some(record)) => {
            state.best_stars = record.best.min(state.stage_count());
            if record.key == file::design_key(&state.design) {
                state.stars = record.stars.min(state.stage_count());
            }
            state.stage = (state.stars + 1).min(state.stage_count());
        }
        Ok(None) => {}
        Err(error) => state.message = Some(format!("Stars file not used: {error}")),
    }
    commands.insert_resource(state);
}

/// Starts the run of the selected stage on the current design.
fn start_stage_run(
    state: &mut PlayState,
    mode: &PlayMode,
    runner: &mut StreetRunner,
    next: &mut NextState<PlayPhase>,
) {
    let options = stage_options(&state.level, state.stage)
        .unwrap_or_else(|| RunOptions::for_level(&state.level));
    match StreetRun::new(&state.level, &state.design, options) {
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

/// Uses the answer of the check of the lower stages: if they all pass, the selected stage
/// runs; if one fails, its report is shown (the design breaks there).
fn apply_check(
    state: &mut PlayState,
    mode: &PlayMode,
    runner: &mut StreetRunner,
    next: &mut NextState<PlayPhase>,
    answer: CheckAnswer,
) {
    let wanted = state.stage;
    let (stars, mut reports) = match answer {
        Ok(answer) => answer,
        Err(error) => {
            state.message = Some(format!("The stages cannot be checked: {error}"));
            next.set(PlayPhase::Design);
            return;
        }
    };
    state.stars = stars;
    state.best_stars = state.best_stars.max(stars);
    state.save_stars();
    if stars + 1 >= wanted {
        start_stage_run(state, mode, runner, next);
        return;
    }
    // A lower stage failed: its report is the answer, and that stage is the one to work on.
    let failed = reports.pop().expect("a failed stage has a report");
    state.stage = failed.stage;
    state.report = Some(ReportData {
        summary: None,
        stage: failed,
        extra: vec![format!(
            "You asked for stage {wanted}, but this design does not pass stage {}.",
            state.stage
        )],
    });
    next.set(PlayPhase::Report);
}

/// Starts the run of the selected stage of the current design: saves the files first. If
/// the stages below it are not known to pass for THIS design, they are played headless
/// first (on a worker thread when `mode.threaded`; the phase is `Check` meanwhile).
pub fn begin_run(
    state: &mut PlayState,
    mode: &PlayMode,
    runner: &mut StreetRunner,
    next: &mut NextState<PlayPhase>,
) {
    if let Err(error) = file::save_design(&state.design) {
        state.message = Some(format!("Design not saved: {error}"));
    }
    state.save_stars();
    if state.stage <= 1 || state.stars + 1 >= state.stage {
        start_stage_run(state, mode, runner, next);
        return;
    }
    let below = state.stage - 1;
    if mode.threaded {
        let (level, design) = (state.level.clone(), state.design.clone());
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("stage-check".into())
            .spawn(move || {
                // The player may have left: a closed channel is fine.
                let _ = tx.send(stars_for_design(&level, &design, below));
            })
            .expect("spawn the stage check");
        state.check = Some(StageCheck {
            stage: state.stage,
            rx: Mutex::new(rx),
        });
        next.set(PlayPhase::Check);
    } else {
        let answer = stars_for_design(&state.level, &state.design, below);
        apply_check(state, mode, runner, next, answer);
    }
}

/// Check phase: waits for the worker's answer.
fn check_input(
    mode: Res<PlayMode>,
    mut state: ResMut<PlayState>,
    mut runner: ResMut<StreetRunner>,
    mut next: ResMut<NextState<PlayPhase>>,
) {
    let Some(check) = &state.check else {
        next.set(PlayPhase::Design);
        return;
    };
    let received = check.rx.lock().expect("only this system locks").try_recv();
    match received {
        Ok(answer) => {
            state.check = None;
            apply_check(&mut state, &mode, &mut runner, &mut next, answer);
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            state.check = None;
            state.message = Some("The stage check stopped without an answer.".into());
            next.set(PlayPhase::Design);
        }
    }
}

/// Ends the run (it is over, or the player stopped it): the report is made, and a passed
/// stage counts as a star of this design.
fn end_run(state: &mut PlayState, runner: &mut StreetRunner, next: &mut NextState<PlayPhase>) {
    let stage = state.stage;
    let (summary, mut report, finished) = {
        let run = runner.get_mut().expect("a run is going when it ends");
        (
            run.summary(),
            stage_report(run, &state.level, stage),
            run.finished(),
        )
    };
    let mut extra = Vec::new();
    if !finished {
        // A run stopped early proves nothing.
        report.passed = false;
        extra.push("The run was stopped early: it does not count.".to_string());
    }
    // A stopped run stays where it is.
    runner.set_speed(0);
    if report.passed {
        state.stage_passed(stage);
    }
    state.report = Some(ReportData {
        summary: Some(summary),
        stage: report,
        extra,
    });
    next.set(PlayPhase::Report);
}

/// Makes an edit with `make` on the street: kept in the design only if the street accepts it
/// (then the level is marked changed, so the street view redraws; a refused try is not a
/// change). `nothing` is told to the player when `make` finds nothing to do.
fn edit_with(
    state: &mut PlayState,
    loaded: &mut ResMut<LoadedLevel>,
    nothing: &str,
    make: impl FnOnce(&Street) -> Option<DesignCommand>,
) {
    let kept = loaded
        .bypass_change_detection()
        .street
        .as_mut()
        .is_some_and(|street| match make(street) {
            Some(command) => state.edit(street, command),
            None => {
                if !nothing.is_empty() {
                    state.message = Some(nothing.into());
                }
                false
            }
        });
    if kept {
        loaded.set_changed();
    }
}

/// Design phase: tools, undo and redo, and the start of the run.
#[allow(clippy::too_many_arguments)]
fn design_input(
    actions: ActionInput,
    keys: Res<ButtonInput<KeyCode>>,
    pointer: Res<Pointer>,
    mode: Res<PlayMode>,
    over_ui: Res<PointerOverUi>,
    mut wheel: MessageReader<MouseWheel>,
    mut drag_start: ResMut<preview::DragStart>,
    mut state: ResMut<PlayState>,
    mut loaded: ResMut<LoadedLevel>,
    mut runner: ResMut<StreetRunner>,
    mut next: ResMut<NextState<PlayPhase>>,
) {
    if actions.just_pressed(Action::ToolCancel) {
        state.tool = None;
    }
    // [ and ] or PageDown and PageUp: the stage (only an unlocked one).
    if keys.any_just_pressed([KeyCode::BracketRight, KeyCode::PageUp]) {
        let next_stage = state.stage + 1;
        state.select_stage(next_stage);
    }
    if keys.any_just_pressed([KeyCode::BracketLeft, KeyCode::PageDown]) && state.stage > 1 {
        let previous = state.stage - 1;
        state.select_stage(previous);
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

    let at = pointer.world / PX_PER_M;
    let usable = pointer.in_window && !over_ui.0;
    if let Some(tool) = state.tool
        && usable
    {
        if tool.is_drag() {
            if pointer.left.just_pressed {
                drag_start.0 = Some(at);
            }
            if pointer.left.just_released
                && let Some(start) = drag_start.0.take()
            {
                edit_with(&mut state, &mut loaded, tool.nothing_text(), |street| {
                    tool.release(street, start, at)
                });
            }
        } else if pointer.left.just_pressed {
            edit_with(&mut state, &mut loaded, tool.nothing_text(), |street| {
                tool.click(street, at)
            });
        }
    }
    if !pointer.left.pressed && drag_start.0.is_some() {
        drag_start.0 = None;
    }

    // The wheel and +/- change the green time of the junction under the pointer, when the
    // signals tool is on (the camera does not zoom for that wheel: `WheelLock`).
    let mut steps = 0;
    for event in wheel.read() {
        if event.unit == MouseScrollUnit::Line {
            steps += event.y.signum() as i32 * i32::from(event.y != 0.0);
        }
    }
    steps += i32::from(keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd));
    steps -=
        i32::from(keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract));
    if state.tool == Some(PlayTool::Signals)
        && usable
        && steps != 0
        && loaded
            .street
            .as_ref()
            .is_some_and(|street| pick::signalled_junction_at(street, at).is_some())
    {
        edit_with(&mut state, &mut loaded, "", |street| {
            pick::green_command(street, at, steps)
        });
    }

    if actions.just_pressed(Action::PlayConfirm) {
        begin_run(&mut state, &mode, &mut runner, &mut next);
    }
}

/// The camera must not zoom with the wheel that changes a green time: it is locked while
/// the signals tool is on and the pointer is over a junction under signals. (One frame behind
/// the pointer, which does not matter.)
fn update_wheel_lock(
    phase: Res<State<PlayPhase>>,
    state: Res<PlayState>,
    pointer: Res<Pointer>,
    over_ui: Res<PointerOverUi>,
    loaded: Res<LoadedLevel>,
    mut lock: ResMut<WheelLock>,
) {
    let locked = *phase.get() == PlayPhase::Design
        && state.tool == Some(PlayTool::Signals)
        && pointer.in_window
        && !over_ui.0
        && loaded.street.as_ref().is_some_and(|street| {
            pick::signalled_junction_at(street, pointer.world / PX_PER_M).is_some()
        });
    if lock.0 != locked {
        lock.0 = locked;
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
        // After a pass the next stage is selected; after a fail the same one stays.
        if state
            .report
            .as_ref()
            .is_some_and(|report| report.stage.passed)
        {
            let next_stage = (state.stage + 1).min(state.stage_count());
            state.select_stage(next_stage);
        }
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
        .init_resource::<PointerOverUi>()
        .init_resource::<preview::DragStart>()
        .init_resource::<preview::Preview>()
        .insert_resource(ClearColor(crate::views::palette::OUTSIDE))
        .init_state::<PlayPhase>()
        .add_plugins((
            ui::PlayUiPlugin,
            markers::MarkersPlugin,
            signals::SignalMarksPlugin,
            inspect::InspectPlugin,
        ))
        .add_systems(Startup, start_play)
        .add_systems(
            Update,
            (
                ui::update_pointer_over_ui,
                update_wheel_lock,
                design_input.run_if(in_state(PlayPhase::Design)),
                preview::update_preview,
                check_input.run_if(in_state(PlayPhase::Check)),
                run_input.run_if(in_state(PlayPhase::Run)),
                report_input.run_if(in_state(PlayPhase::Report)),
            )
                .chain()
                .run_if(resource_exists::<PlayState>),
        );
    }
}
