//! Play mode (pivot Phase 4): `--play` parsing, the zebra and speed tools, undo, the design
//! file, and the phases Design -> Run -> Report -> Design. All tests share the design
//! folder, so each one holds `save_lock`.

use crate::common::{Harness, save_lock};
use bevy::prelude::*;
use tf_game::capture::{Cli, Target, parse_args};
use tf_game::level::{LoadedLevel, PX_PER_M};
use tf_game::play::{PlayPhase, PlayState, PlayTool, file, pick};
use tf_game::street_runner::StreetRunner;
use tf_sim::roads::lane_builder::polyline_length;
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
}

#[test]
fn play_mode_opens_the_level_and_the_old_game_sleeps() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    assert_eq!(phase(&harness), PlayPhase::Design);
    assert_eq!(street(&harness).level_id, LEVEL);
    assert!(state(&harness).design.commands.is_empty());
    // The old tools and keys do nothing.
    harness.key(KeyCode::KeyR);
    assert_eq!(harness.active_tool(), None);
    harness.key(KeyCode::F7);
    assert_eq!(harness.segments(), 0, "no scenario loaded in play mode");
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
    assert!(report.summary.ended.is_some());
    assert!(report.summary.spawned > 0);
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
    assert_eq!(report.summary.ended, None, "stopped, not finished");
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
