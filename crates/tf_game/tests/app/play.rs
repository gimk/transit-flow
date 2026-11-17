//! Play mode (pivot Phase 4): `--play` parsing, the zebra and speed tools, undo, the design
//! file, and the phases Design -> Run -> Report -> Design. All tests share the design
//! folder, so each one holds `save_lock`.

use crate::common::{Harness, save_lock};
use bevy::prelude::*;
use tf_game::capture::{Cli, Target, parse_args};
use tf_game::level::{LoadedLevel, PX_PER_M};
use tf_game::play::{PlayPhase, PlayState, PlayTool, file, pick};
use tf_game::street_runner::StreetRunner;
use tf_sim::geom::lanes::polyline_length;
use tf_sim::street::{Design, DesignCommand, Street};

const LEVEL: &str = "carrefour-ecole";

fn phase(harness: &Harness) -> PlayPhase {
    *harness.app.world().resource::<State<PlayPhase>>().get()
}

fn state(harness: &Harness) -> &PlayState {
    harness.app.world().resource::<PlayState>()
}

fn street(harness: &Harness) -> &Street {
    harness
        .app
        .world()
        .resource::<LoadedLevel>()
        .street
        .as_ref()
        .expect("the level is loaded")
}

fn runner(harness: &mut Harness) -> Mut<'_, StreetRunner> {
    harness.app.world_mut().resource_mut::<StreetRunner>()
}

/// The point `along` metres down a segment's centerline, on it.
fn centerline_point(street: &Street, segment: i32, along: f32) -> Vec2 {
    let line = &street.segments[&segment].centerline;
    let mut walked = 0.0;
    for pair in line.windows(2) {
        let length = pair[0].distance(pair[1]);
        if along <= walked + length {
            return pair[0] + (pair[1] - pair[0]) * ((along - walked) / length);
        }
        walked += length;
    }
    *line.last().expect("a line")
}

/// A spot (street metres) on a carriageway where a zebra can be placed, and its segment.
fn free_spot(street: &Street) -> (i32, Vec2) {
    for segment in street.segments.values() {
        let length = polyline_length(&segment.centerline) as f32;
        let mut along = segment.start_trim + 4.0;
        while along < length - segment.end_trim - 4.0 {
            let p = centerline_point(street, segment.id, along);
            if let Some(command) = pick::zebra_command(street, p)
                && matches!(command, DesignCommand::AddCrossing { .. })
                && street.clone().apply(&command).is_ok()
            {
                return (segment.id, p);
            }
            along += 3.0;
        }
    }
    panic!("no free spot");
}

fn click_street(harness: &mut Harness, metres: Vec2) {
    harness.click(metres * PX_PER_M, MouseButton::Left);
}

#[test]
fn play_arguments_parse() {
    assert_eq!(
        parse_args(["--play".to_string(), LEVEL.to_string()]),
        Ok(Cli::PlayLevel(LEVEL.into()))
    );
    assert_eq!(parse_args(Vec::<String>::new()), Ok(Cli::Play));
    let args = |list: &[&str]| parse_args(list.iter().map(|s| s.to_string()));
    assert!(args(&["--play", "nowhere"]).is_err());
    assert!(args(&["--play", LEVEL, "--scenario", "town"]).is_err());
    assert!(args(&["--play", LEVEL, "--run"]).is_err());
    let Ok(Cli::Capture(options)) = args(&[
        "--play",
        LEVEL,
        "--run",
        "--frames",
        "5",
        "--screenshot",
        "a.png",
    ]) else {
        panic!("a play screenshot")
    };
    assert!(options.play && options.run);
    assert_eq!(options.target, Target::Level(LEVEL.into()));
    // A framing zoom goes with a play screenshot; without a screenshot it is refused.
    let Ok(Cli::Capture(zoomed)) = args(&[
        "--play",
        LEVEL,
        "--frames",
        "5",
        "--screenshot",
        "a.png",
        "--zoom",
        "2",
    ]) else {
        panic!("a zoomed play screenshot")
    };
    assert_eq!(zoomed.zoom, Some(2.0));
    assert!(args(&["--play", LEVEL, "--zoom", "2"]).is_err());
}

#[test]
fn play_mode_opens_the_level() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    assert_eq!(phase(&harness), PlayPhase::Design);
    assert_eq!(street(&harness).level_id, LEVEL);
    assert!(state(&harness).design.commands.is_empty());
    // The play tools work.
    harness.key(KeyCode::KeyC);
    assert_eq!(state(&harness).tool, Some(PlayTool::Zebra));
    harness.key(KeyCode::KeyC);
    assert_eq!(state(&harness).tool, None, "the key again drops the tool");
    harness.key(KeyCode::KeyV);
    assert_eq!(state(&harness).tool, Some(PlayTool::SpeedLimit));
    harness.key(KeyCode::Escape);
    assert_eq!(state(&harness).tool, None);
}

#[test]
fn a_zebra_click_adds_a_crossing_and_undo_takes_it_away() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let crossings = street(&harness).crossings.len();
    let (_, spot) = free_spot(street(&harness));
    harness.key(KeyCode::KeyC);
    click_street(&mut harness, spot);
    assert_eq!(street(&harness).crossings.len(), crossings + 1);
    assert_eq!(state(&harness).design.commands.len(), 1);
    // A click on the new crossing takes it away again (a second command).
    click_street(&mut harness, spot);
    assert_eq!(street(&harness).crossings.len(), crossings);
    assert_eq!(state(&harness).design.commands.len(), 2);
    // Undo twice: the crossing comes back, then goes.
    harness.key(KeyCode::Backspace);
    assert_eq!(street(&harness).crossings.len(), crossings + 1);
    harness.key(KeyCode::Backspace);
    assert_eq!(street(&harness).crossings.len(), crossings);
    assert!(state(&harness).design.commands.is_empty());
    assert_eq!(state(&harness).redo.len(), 2);
    // Nothing left to undo: nothing breaks.
    harness.key(KeyCode::Backspace);
    assert!(state(&harness).design.commands.is_empty());
}

#[test]
fn ctrl_z_and_ctrl_y_undo_and_redo() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let crossings = street(&harness).crossings.len();
    let (_, spot) = free_spot(street(&harness));
    harness.key(KeyCode::KeyC);
    click_street(&mut harness, spot);
    // Z alone does nothing; with Ctrl it undoes.
    harness.key(KeyCode::KeyZ);
    assert_eq!(street(&harness).crossings.len(), crossings + 1);
    let mut input = harness
        .app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>();
    input.press(KeyCode::ControlLeft);
    harness.key(KeyCode::KeyZ);
    assert_eq!(street(&harness).crossings.len(), crossings);
    harness.key(KeyCode::KeyY);
    assert_eq!(street(&harness).crossings.len(), crossings + 1);
    assert_eq!(state(&harness).design.commands.len(), 1);
    assert!(state(&harness).redo.is_empty());
}

#[test]
fn the_speed_tool_cycles_the_limit_of_the_clicked_segment() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let (segment, spot) = free_spot(street(&harness));
    let limit = |harness: &Harness| street(harness).segments[&segment].speed_limit_kmh;
    let first = limit(&harness);
    harness.key(KeyCode::KeyV);
    let mut seen = vec![first];
    for _ in 0..3 {
        click_street(&mut harness, spot);
        seen.push(limit(&harness));
    }
    assert_eq!(seen[1], pick::next_speed_limit(first));
    assert_eq!(seen[2], pick::next_speed_limit(seen[1]));
    assert_eq!(seen[3], first, "three clicks go all the way round");
    assert_eq!(state(&harness).design.commands.len(), 3);
}

#[test]
fn a_refused_edit_shows_its_error_and_is_not_kept() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let mut state = PlayState::new(tf_sim::street::LevelFile::load(LEVEL).expect("loads"));
    let mut street = state.build_street().expect("builds");
    let kept = state.edit(
        &mut street,
        DesignCommand::SetSpeedLimit {
            segment: 1,
            kmh: 37.0,
        },
    );
    assert!(!kept);
    assert!(state.design.commands.is_empty());
    assert!(
        state
            .message
            .as_deref()
            .is_some_and(|m| m.contains("Not possible"))
    );
    // In the app: a click with a tool on nothing says so and keeps nothing.
    harness.key(KeyCode::KeyC);
    click_street(&mut harness, Vec2::new(-400.0, -400.0));
    assert!(state_of(&harness).design.commands.is_empty());
    assert!(state_of(&harness).message.is_some());
}

fn state_of(harness: &Harness) -> &PlayState {
    state(harness)
}

#[test]
fn enter_starts_the_run_saves_the_design_and_the_end_opens_the_report() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let (_, spot) = free_spot(street(&harness));
    harness.key(KeyCode::KeyC);
    click_street(&mut harness, spot);
    harness.key(KeyCode::Enter);
    harness.frames(2);
    assert_eq!(phase(&harness), PlayPhase::Run);
    assert!(runner(&mut harness).is_running());
    // The design was written when the run started, and reads back the same.
    let saved = file::load_design(LEVEL).expect("readable").expect("a file");
    assert_eq!(saved, state(&harness).design);
    assert_eq!(saved.commands.len(), 1);
    // No edits during the run: a click changes nothing.
    harness.key(KeyCode::KeyC);
    click_street(&mut harness, spot);
    assert_eq!(state(&harness).design.commands.len(), 1);
    // Space pauses and resumes; the digits set the speed (4 is x8).
    harness.key(KeyCode::Space);
    assert_eq!(runner(&mut harness).speed_index(), 0);
    harness.key(KeyCode::Space);
    assert_eq!(runner(&mut harness).speed_index(), 1);
    harness.key(KeyCode::Digit4);
    assert_eq!(runner(&mut harness).speed_index(), 4);
    // The run ends: the report opens with the summary.
    runner(&mut harness).get_mut().expect("a run").run_to_end();
    harness.frames(3);
    assert_eq!(phase(&harness), PlayPhase::Report);
    let report = state(&harness).report.as_ref().expect("a report");
    assert!(
        report
            .summary
            .as_ref()
            .expect("a run summary")
            .ended
            .is_some()
    );
    assert!(report.summary.as_ref().expect("a run summary").spawned > 0);
    // Enter: back to the design, the same design, the run gone.
    harness.key(KeyCode::Enter);
    harness.frames(2);
    assert_eq!(phase(&harness), PlayPhase::Design);
    assert!(!runner(&mut harness).is_running());
    assert_eq!(state(&harness).design.commands.len(), 1);
    assert!(state(&harness).report.is_none());
}

#[test]
fn enter_during_the_run_stops_it_early() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    harness.key(KeyCode::Enter);
    harness.frames(2);
    assert_eq!(phase(&harness), PlayPhase::Run);
    harness.key(KeyCode::Enter);
    harness.frames(2);
    assert_eq!(phase(&harness), PlayPhase::Report);
    let report = state(&harness).report.as_ref().expect("a report");
    assert_eq!(
        report.summary.as_ref().expect("a summary").ended,
        None,
        "stopped"
    );
    assert!(!report.stage.passed, "a stopped run never passes");
}

#[test]
fn the_design_file_is_read_when_play_mode_opens() {
    let _lock = save_lock();
    let (_, spot) = {
        let harness = Harness::play(LEVEL);
        free_spot(street(&harness))
    };
    let mut harness = Harness::play(LEVEL);
    harness.key(KeyCode::KeyC);
    click_street(&mut harness, spot);
    harness.key(KeyCode::Enter);
    harness.frames(2);
    let crossings = street(&harness).crossings.len();
    drop(harness);
    // A new session on the same folder starts with the saved design.
    let harness = Harness::play(LEVEL);
    assert_eq!(state(&harness).design.commands.len(), 1);
    assert_eq!(street(&harness).crossings.len(), crossings);
    assert!(state(&harness).message.is_none());
}

#[test]
fn a_bad_design_file_starts_empty_and_says_why() {
    let _lock = save_lock();
    let path = file::design_path(LEVEL);
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("folder");
    std::fs::write(&path, "this is not json").expect("written");
    let harness = Harness::play(LEVEL);
    assert!(state(&harness).design.commands.is_empty());
    let message = state(&harness).message.clone().expect("a message");
    assert!(message.contains("Design file not used"), "{message}");
    // A design for another level, or one that does not replay, is refused the same way.
    let other = Design::new("elsewhere");
    std::fs::write(&path, other.to_json()).expect("written");
    let harness = Harness::play(LEVEL);
    assert!(
        state(&harness)
            .message
            .as_deref()
            .is_some_and(|m| m.contains("another") || m.contains("elsewhere"))
    );
    let mut broken = Design::new(LEVEL);
    broken
        .commands
        .push(DesignCommand::RemoveCrossing { id: 9999 });
    std::fs::write(&path, broken.to_json()).expect("written");
    let harness = Harness::play(LEVEL);
    assert!(state(&harness).design.commands.is_empty());
    assert!(state(&harness).message.is_some());
}

/// Offset of a kerb of a segment (street metres from its centerline), left or right.
fn kerb_offset(street: &Street, segment: i32, left: bool) -> f32 {
    let (l, r) = tf_game::views::street::carriageway(&street.segments[&segment].profile)
        .expect("a carriageway");
    if left { l } else { r }
}

/// A point of the segment at `along` metres, `offset` metres to the right of its centerline.
fn segment_point(street: &Street, segment: i32, along: f32, offset: f32) -> Vec2 {
    pick::point_at(&street.segments[&segment].centerline, along, offset)
}

#[test]
fn a_drag_with_the_bollard_tool_places_a_row_and_a_click_removes_it() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let (segment, _) = free_spot(street(&harness));
    let along = street(&harness).segments[&segment].start_trim + 5.0;
    let obstacles = street(&harness).obstacles.len();
    let kerb = kerb_offset(street(&harness), segment, false);
    let a = segment_point(street(&harness), segment, along, kerb + 0.3);
    let b = segment_point(street(&harness), segment, along + 6.2, kerb + 0.3);
    harness.key(KeyCode::KeyB);
    assert_eq!(state(&harness).tool, Some(PlayTool::Bollards));
    harness.drag(a * PX_PER_M, b * PX_PER_M);
    let rows: Vec<_> = street(&harness).bollard_rows.values().cloned().collect();
    assert_eq!(rows.len(), 1, "{:?}", state(&harness).message);
    assert_eq!(rows[0].segment, segment);
    assert_eq!(rows[0].side, tf_sim::street::design::Side::Right);
    assert_eq!((rows[0].from * 2.0).fract(), 0.0, "snapped to 0.5 m");
    assert!((rows[0].to - rows[0].from - 6.0).abs() <= 1.0);
    assert!(
        street(&harness).obstacles.len() > obstacles,
        "dots in obstacles"
    );
    // A click on the row takes it away; a click elsewhere on the kerb does nothing.
    let on_row = segment_point(
        street(&harness),
        segment,
        (rows[0].from + rows[0].to) * 0.5,
        kerb,
    );
    harness.click(on_row * PX_PER_M, MouseButton::Left);
    assert!(street(&harness).bollard_rows.is_empty());
    assert_eq!(street(&harness).obstacles.len(), obstacles);
    assert_eq!(state(&harness).design.commands.len(), 2);
    // Undo brings the row back.
    harness.key(KeyCode::Backspace);
    assert_eq!(street(&harness).bollard_rows.len(), 1);
}

#[test]
fn a_drag_across_two_segments_places_nothing() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    harness.key(KeyCode::KeyB);
    // A short drag in the open: nothing under it.
    harness.drag(Vec2::new(-4000.0, -4000.0), Vec2::new(-3900.0, -4000.0));
    assert!(state(&harness).design.commands.is_empty());
    assert!(state(&harness).message.is_some());
}

#[test]
fn the_signals_tool_cycles_the_control_and_the_wheel_changes_the_green() {
    use tf_sim::street::design::SignalTemplate;
    use tf_sim::street::level::JunctionControl;
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let node = *street(&harness)
        .junctions
        .keys()
        .next()
        .expect("a junction");
    let centre = street(&harness).nodes[&node].position;
    let control = |h: &Harness| {
        let j = &street(h).junctions[&node];
        (j.control, pick::template_of(j))
    };
    let start = control(&harness);
    assert_eq!(start.0, JunctionControl::PriorityToRight);
    harness.key(KeyCode::KeyL);
    harness.click(centre * PX_PER_M, MouseButton::Left);
    assert_eq!(
        control(&harness),
        (
            JunctionControl::Signals,
            Some(SignalTemplate::TwoPhase { green_s: 20.0 })
        )
    );
    harness.click(centre * PX_PER_M, MouseButton::Left);
    assert_eq!(
        control(&harness),
        (
            JunctionControl::Signals,
            Some(SignalTemplate::TwoPhaseWithWalk {
                green_s: 20.0,
                walk_s: 10.0
            })
        )
    );
    // The wheel (and +/-) change the green by 5 s, held between 10 and 60.
    harness
        .app
        .world_mut()
        .write_message(bevy::input::mouse::MouseWheel {
            unit: bevy::input::mouse::MouseScrollUnit::Line,
            x: 0.0,
            y: 1.0,
            phase: bevy::input::touch::TouchPhase::Moved,
            window: Entity::PLACEHOLDER,
        });
    harness.frames(2);
    assert!(
        harness
            .app
            .world()
            .resource::<tf_game::camera::WheelLock>()
            .0,
        "the camera does not zoom over a signalled junction"
    );
    let green = |h: &Harness| match control(h).1 {
        Some(SignalTemplate::TwoPhaseWithWalk { green_s, .. }) => green_s,
        other => panic!("{other:?}"),
    };
    assert_eq!(green(&harness), 25.0);
    harness.key(KeyCode::Equal);
    assert_eq!(green(&harness), 30.0);
    for _ in 0..12 {
        harness.key(KeyCode::Minus);
    }
    assert_eq!(green(&harness), 10.0);
    for _ in 0..12 {
        harness.key(KeyCode::Equal);
    }
    assert_eq!(green(&harness), 60.0);
    // One more click: back to priority to the right.
    harness.click(centre * PX_PER_M, MouseButton::Left);
    assert_eq!(control(&harness).0, JunctionControl::PriorityToRight);
    // Away from a junction the wheel is the camera's.
    harness.move_to(Vec2::new(-4000.0, -4000.0));
    harness.frames(2);
    assert!(
        !harness
            .app
            .world()
            .resource::<tf_game::camera::WheelLock>()
            .0
    );
}

#[test]
fn a_signalled_junction_shows_its_lights_in_the_run() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let node = *street(&harness)
        .junctions
        .keys()
        .next()
        .expect("a junction");
    let centre = street(&harness).nodes[&node].position;
    harness.key(KeyCode::KeyL);
    harness.click(centre * PX_PER_M, MouseButton::Left);
    harness.key(KeyCode::Enter);
    harness.frames(3);
    assert_eq!(phase(&harness), PlayPhase::Run);
    let lights = runner(&mut harness).view_to().lights.clone();
    assert!(lights.len() >= 3, "an approach per arm: {lights:?}");
    assert!(lights.iter().all(|l| l.node == node));
}

fn state_mut(harness: &mut Harness) -> Mut<'_, PlayState> {
    harness.app.world_mut().resource_mut::<PlayState>()
}

/// Plays the empty design through to the end of the run and opens the report.
fn finish_run(harness: &mut Harness) {
    runner(harness).get_mut().expect("a run").run_to_end();
    harness.frames(3);
    assert_eq!(phase(harness), PlayPhase::Report);
}

#[test]
fn a_passed_stage_wins_a_star_unlocks_the_next_and_is_saved_with_the_design() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    assert_eq!((state(&harness).stage, state(&harness).stars), (1, 0));
    // Stage 2 is locked until stage 1 is passed.
    harness.key(KeyCode::BracketRight);
    assert_eq!(state(&harness).stage, 1);
    assert!(
        state(&harness)
            .message
            .as_deref()
            .is_some_and(|m| m.contains("locked"))
    );
    harness.key(KeyCode::Enter);
    harness.frames(2);
    assert_eq!(phase(&harness), PlayPhase::Run);
    finish_run(&mut harness);
    let report = state(&harness).report.clone().expect("a report");
    assert_eq!(report.stage.stage, 1);
    assert!(
        report.stage.passed,
        "the empty design passes stage 1: {:?}",
        report.stage
    );
    assert_eq!((state(&harness).stars, state(&harness).best_stars), (1, 1));
    // Enter: back to the design on the next stage.
    harness.key(KeyCode::Enter);
    harness.frames(2);
    assert_eq!(phase(&harness), PlayPhase::Design);
    assert_eq!(state(&harness).stage, 2);
    // The stars were saved next to the design, and a new session finds them.
    let saved = file::load_stars(LEVEL).expect("readable").expect("a file");
    assert_eq!((saved.stars, saved.best), (1, 1));
    let session = Harness::play(LEVEL);
    assert_eq!((state(&session).stars, state(&session).best_stars), (1, 1));
    assert_eq!(state(&session).stage, 2);
    drop(session);
    // A different design keeps the unlock but not the stars.
    let mut other = Design::new(LEVEL);
    other.commands.push(DesignCommand::SetSpeedLimit {
        segment: 1,
        kmh: 20.0,
    });
    std::fs::write(file::design_path(LEVEL), other.to_json()).expect("written");
    let session = Harness::play(LEVEL);
    assert_eq!((state(&session).stars, state(&session).best_stars), (0, 1));
}

#[test]
fn an_edit_takes_the_stars_back() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    state_mut(&mut harness).stars = 2;
    state_mut(&mut harness).best_stars = 2;
    let (_, spot) = free_spot(street(&harness));
    harness.key(KeyCode::KeyC);
    click_street(&mut harness, spot);
    assert_eq!(state(&harness).stars, 0, "stars belong to a design");
    assert_eq!(state(&harness).best_stars, 2, "the unlock stays");
    // The stage list is still open: stage 3 can be chosen.
    harness.key(KeyCode::Escape);
    harness.key(KeyCode::PageUp);
    harness.key(KeyCode::PageUp);
    assert_eq!(state(&harness).stage, 3);
    harness.key(KeyCode::BracketLeft);
    assert_eq!(state(&harness).stage, 2);
}

#[test]
fn a_higher_stage_first_plays_the_lower_ones_on_this_design() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    state_mut(&mut harness).best_stars = 1;
    state_mut(&mut harness).stage = 2;
    assert_eq!(state(&harness).stars, 0, "stage 1 is not known to pass");
    harness.key(KeyCode::Enter);
    harness.frames(2);
    // Stage 1 passed headless (the empty design), so stage 2 runs.
    assert_eq!(phase(&harness), PlayPhase::Run);
    assert_eq!(state(&harness).stars, 1);
    assert!(runner(&mut harness).is_running());
    // The run is stage 2's: more traffic than stage 1's.
    let scale = state(&harness).level.stages[1].demand_scale;
    assert!(scale > state(&harness).level.stages[0].demand_scale);
}

#[test]
fn a_lower_stage_that_fails_shows_its_report_and_holds_the_stage() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    // Nobody can pass stage 1 now.
    state_mut(&mut harness).level.stages[0].pass_score = 1000.0;
    state_mut(&mut harness).best_stars = 1;
    state_mut(&mut harness).stage = 2;
    harness.key(KeyCode::Enter);
    harness.frames(3);
    assert_eq!(phase(&harness), PlayPhase::Report);
    assert!(!runner(&mut harness).is_running(), "no run was started");
    let report = state(&harness).report.clone().expect("a report");
    assert_eq!(report.stage.stage, 1);
    assert!(!report.stage.passed);
    assert!(report.summary.is_none(), "it was played headless");
    assert_eq!(state(&harness).stage, 1, "the stage the design breaks at");
    assert_eq!(state(&harness).stars, 0);
    // Enter: back to the design, on the same (failed) stage.
    harness.key(KeyCode::Enter);
    harness.frames(2);
    assert_eq!(phase(&harness), PlayPhase::Design);
    assert_eq!(state(&harness).stage, 1);
}

#[test]
fn the_check_runs_on_a_worker_thread_and_the_report_follows() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    harness
        .app
        .world_mut()
        .resource_mut::<tf_game::play::PlayMode>()
        .threaded = true;
    state_mut(&mut harness).level.stages[0].pass_score = 1000.0;
    state_mut(&mut harness).best_stars = 1;
    state_mut(&mut harness).stage = 2;
    harness.key(KeyCode::Enter);
    let started = std::time::Instant::now();
    let mut saw_check = false;
    while phase(&harness) != PlayPhase::Report {
        saw_check |= phase(&harness) == PlayPhase::Check;
        assert!(
            started.elapsed() < std::time::Duration::from_secs(300),
            "never answered"
        );
        harness.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(saw_check, "the Check phase was shown while it ran");
    assert_eq!(
        state(&harness)
            .report
            .as_ref()
            .expect("a report")
            .stage
            .stage,
        1
    );
    assert!(state(&harness).check.is_none());
}

fn count<T: Component>(harness: &mut Harness) -> usize {
    harness
        .app
        .world_mut()
        .query::<&T>()
        .iter(harness.app.world())
        .count()
}

#[test]
fn a_zebra_hover_shows_a_ghost_and_changes_nothing() {
    use tf_game::play::preview::PreviewShape;
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let crossings = street(&harness).crossings.len();
    let (_, spot) = free_spot(street(&harness));
    // No tool: no ghost.
    harness.move_to(spot * PX_PER_M);
    assert_eq!(count::<PreviewShape>(&mut harness), 0);
    harness.key(KeyCode::KeyC);
    harness.move_to(spot * PX_PER_M);
    assert_eq!(
        count::<PreviewShape>(&mut harness),
        1,
        "a ghost on the road"
    );
    assert_eq!(
        street(&harness).crossings.len(),
        crossings,
        "a hover edits nothing"
    );
    assert!(state(&harness).design.commands.is_empty());
    // Away from any road: the ghost goes.
    harness.move_to(Vec2::new(-500.0, -500.0) * PX_PER_M);
    assert_eq!(count::<PreviewShape>(&mut harness), 0);
    assert!(state(&harness).hover_note.is_none());
    // Back on the road, then drop the tool: the ghost goes too.
    harness.move_to(spot * PX_PER_M);
    assert_eq!(count::<PreviewShape>(&mut harness), 1);
    harness.key(KeyCode::Escape);
    assert_eq!(count::<PreviewShape>(&mut harness), 0);
}

#[test]
fn the_speed_sign_shows_the_new_limit_after_the_speed_tool() {
    use tf_game::views::speed_signs::{SpeedSigns, sign_spots};
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let (segment, spot) = free_spot(street(&harness));
    let signs_mesh = |harness: &mut Harness| {
        let handle = harness
            .app
            .world_mut()
            .query_filtered::<&Mesh2d, With<SpeedSigns>>()
            .single(harness.app.world())
            .expect("one sign mesh")
            .0
            .clone();
        let meshes = harness.app.world().resource::<Assets<Mesh>>();
        format!(
            "{:?}",
            meshes
                .get(&handle)
                .expect("the mesh")
                .attribute(Mesh::ATTRIBUTE_POSITION)
        )
    };
    let before = signs_mesh(&mut harness);
    let limit = street(&harness).segments[&segment].speed_limit_kmh;
    harness.key(KeyCode::KeyV);
    click_street(&mut harness, spot);
    harness.frames(2);
    let next = pick::next_speed_limit(limit);
    let spots = sign_spots(street(&harness));
    assert!(spots.iter().any(|s| s.segment == segment && s.kmh == next));
    assert!(
        spots
            .iter()
            .filter(|s| s.segment == segment)
            .all(|s| s.kmh == next)
    );
    assert_ne!(signs_mesh(&mut harness), before, "the sign was redrawn");
}

/// The text of the mesh of every speed marking (empty when there is none).
fn sign_mesh_text(harness: &mut Harness) -> String {
    use tf_game::views::speed_signs::SpeedSigns;
    let handles: Vec<Handle<Mesh>> = harness
        .app
        .world_mut()
        .query_filtered::<&Mesh2d, With<SpeedSigns>>()
        .iter(harness.app.world())
        .map(|mesh| mesh.0.clone())
        .collect();
    let meshes = harness.app.world().resource::<Assets<Mesh>>();
    handles
        .iter()
        .map(|handle| {
            format!(
                "{:?}",
                meshes
                    .get(handle)
                    .expect("the mesh")
                    .attribute(Mesh::ATTRIBUTE_POSITION)
            )
        })
        .collect()
}

#[test]
fn the_speed_ghost_hides_the_painted_marking_of_its_segment_only_while_it_shows() {
    use tf_game::play::preview::PreviewShape;
    use tf_game::views::speed_signs::{HiddenSegmentSigns, sign_spots};
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let (segment, spot) = free_spot(street(&harness));
    let painted = sign_mesh_text(&mut harness);
    assert!(!painted.is_empty(), "the level has markings");
    let own = sign_spots(street(&harness))
        .iter()
        .filter(|s| s.segment == segment)
        .count();
    assert!(own > 0);

    // The speed tool, hovering the segment: its painted marking is hidden (the ghost shows
    // the next limit in its place).
    harness.key(KeyCode::KeyV);
    harness.move_to(spot * PX_PER_M);
    harness.frames(2);
    assert_eq!(count::<PreviewShape>(&mut harness), 1);
    assert_eq!(
        harness.app.world().resource::<HiddenSegmentSigns>().0,
        Some(segment)
    );
    assert_ne!(
        sign_mesh_text(&mut harness),
        painted,
        "the marking is hidden"
    );

    // Off the segment (the pointer leaves, or the tool goes): the marking is back, as it was.
    harness.key(KeyCode::Escape);
    harness.frames(2);
    assert_eq!(count::<PreviewShape>(&mut harness), 0);
    assert_eq!(harness.app.world().resource::<HiddenSegmentSigns>().0, None);
    assert_eq!(sign_mesh_text(&mut harness), painted, "the marking is back");
}

#[test]
fn a_click_on_an_incident_line_centres_the_map_on_its_mark_and_keeps_the_zoom() {
    use tf_game::events::CameraFocusRequested;
    use tf_game::level::level_to_world;
    use tf_game::play::ui::{IncidentClicked, IncidentRow};
    use tf_sim::street::ids::AgentId;
    use tf_sim::street::score::{Incident, IncidentKind};
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    harness.key(KeyCode::Enter);
    harness.frames(2);
    finish_run(&mut harness);
    // Two incidents of the stage, inside the level (metres).
    let marks = [Vec2::new(30.0, 45.0), Vec2::new(60.0, 20.0)];
    let incidents: Vec<Incident> = marks
        .iter()
        .enumerate()
        .map(|(i, at)| Incident {
            tick: 100 * (i as u64 + 1),
            time_s: 5.0 * (i as f64 + 1.0),
            kind: IncidentKind::LightConflict,
            position: *at,
            agents: [AgentId(1), AgentId(2)],
            causes: Vec::new(),
            text: format!("Test conflict {i}"),
        })
        .collect();
    state_mut(&mut harness)
        .report
        .as_mut()
        .expect("a report")
        .stage
        .incidents = incidents;
    harness.frames(2);

    // The list shows the two lines, and no more rows than that.
    let rows: Vec<(usize, bool)> = harness
        .app
        .world_mut()
        .query::<(&IncidentRow, &Node)>()
        .iter(harness.app.world())
        .map(|(row, node)| (row.0, node.display != Display::None))
        .collect();
    assert_eq!(rows.iter().filter(|(_, shown)| *shown).count(), 2);
    assert!(rows.iter().any(|(i, shown)| *i == 1 && *shown));

    // The click on line 1 (its hit test needs a window cursor, so the test sends the click
    // it would make). The map is asked to centre on the mark; the zoom is not touched (the
    // camera's own system does the moving, and the harness has no camera).
    harness.app.world_mut().write_message(IncidentClicked(1));
    harness.frames(1);
    let asked = harness.messages::<CameraFocusRequested>();
    assert_eq!(asked, vec![CameraFocusRequested(level_to_world(marks[1]))]);
}

#[test]
fn hovering_an_agent_in_a_run_finds_it_and_the_card_reads_its_traits() {
    use tf_game::play::inspect::{Hover, Hovered, car_pose_at, card_text, walker_pos_at};
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    harness.key(KeyCode::Enter);
    harness.frames(2);
    assert_eq!(phase(&harness), PlayPhase::Run);
    // Run on until a car is on the street, then freeze the picture.
    let mut found = false;
    for _ in 0..4000 {
        let mut runner = runner(&mut harness);
        runner.advance(1.0);
        if !runner.view_to().agents.is_empty() {
            runner.set_speed(0);
            found = true;
            break;
        }
    }
    assert!(found, "a car came in");
    let (id, drawn_at) = {
        let runner = harness.app.world().resource::<StreetRunner>();
        let pose = &runner.view_to().agents[0];
        let (pos, _) = car_pose_at(runner.view_from(), pose, runner.view_alpha());
        (pose.id, pos)
    };
    harness.move_to(drawn_at * PX_PER_M);
    let hovered = harness.app.world().resource::<Hover>().0;
    assert!(hovered.is_some(), "the pointer is on a car");
    if let Some(Hovered::Car(hit)) = hovered {
        // Several cars may stand together: the one found is on the pointer.
        let runner = harness.app.world().resource::<StreetRunner>();
        let pose = &runner.view_to().agents[runner.view_to().index_of(hit).expect("in view")];
        let (pos, _) = car_pose_at(runner.view_from(), pose, runner.view_alpha());
        assert!(pos.distance(drawn_at) < 5.0);
        let _ = id;
    }
    let text = {
        let runner = harness.app.world().resource::<StreetRunner>();
        card_text(hovered.expect("hovered"), runner, Some(street(&harness))).expect("a card")
    };
    assert!(text.contains("Speed") && text.contains("State:"), "{text}");
    if matches!(hovered, Some(Hovered::Car(_))) {
        assert!(
            text.contains("Compliance"),
            "the traits came with the spawn: {text}"
        );
    }
    // Away from every agent: nothing is hovered.
    harness.move_to(Vec2::new(-500.0, -500.0) * PX_PER_M);
    assert_eq!(harness.app.world().resource::<Hover>().0, None);
    let _ = walker_pos_at;
}
