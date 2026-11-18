//! Phase 6, W6 part 1: the play tools of the bicycle design (X separator, N bicycle box,
//! G leading bicycle green) and the bicycle box in the street view. Each tool applies, is
//! refused with its reason, and undoes (and redoes). All tests share the design folder, so
//! each one holds `save_lock`.

use crate::common::{Harness, save_lock};
use bevy::prelude::*;
use tf_game::level::{LoadedLevel, PX_PER_M};
use tf_game::play::{PlayState, PlayTool, pick};
use tf_game::views::street::{bicycle_box_shapes, build_layers};
use tf_sim::geom::lanes::polyline_length;
use tf_sim::street::space::{Direction, Separator, StripUse};
use tf_sim::street::{DesignCommand, Street};

const LEVEL: &str = "carrefour-ecole";
/// The junction of the level and the segment the tests use (its approach has a bicycle lane
/// and a car lane; see `tf_sim/tests/street_bicycle.rs`).
const NODE: i32 = 0;
const SEGMENT: i32 = 1;

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

fn click(harness: &mut Harness, metres: Vec2) {
    harness.click(metres * PX_PER_M, MouseButton::Left);
}

/// The middle of the junction (street metres).
fn junction_centre(street: &Street) -> Vec2 {
    let points = &street.junctions[&NODE].polygon.points;
    points.iter().copied().sum::<Vec2>() / points.len() as f32
}

/// A point of the approach, 3 m before the junction.
fn approach_point(street: &Street) -> Vec2 {
    let segment = &street.segments[&SEGMENT];
    let length = polyline_length(&segment.centerline) as f32;
    let along = if segment.to == NODE {
        length - segment.end_trim - 3.0
    } else {
        segment.start_trim + 3.0
    };
    pick::point_at(&segment.centerline, along, 0.0)
}

/// A point on the line between strips `boundary - 1` and `boundary` of the segment.
fn boundary_point(street: &Street, boundary: usize) -> Vec2 {
    let segment = &street.segments[&SEGMENT];
    let along = segment.start_trim + 10.0;
    let offset = segment.profile.strip_span(boundary).0;
    pick::point_at(&segment.centerline, along, offset)
}

/// Makes an edit as a tool would (kept in the design, the street redrawn), to set up a
/// street the test starts from.
fn edit(harness: &mut Harness, command: DesignCommand) {
    harness
        .app
        .world_mut()
        .resource_scope(|world, mut state: Mut<PlayState>| {
            let mut loaded = world.resource_mut::<LoadedLevel>();
            let street = loaded.street.as_mut().expect("loaded");
            assert!(state.edit(street, command), "{:?}", state.message);
            loaded.set_changed();
        });
    harness.frames(1);
}

/// Puts the signals on the junction with the signals tool, and drops the tool.
fn signals(harness: &mut Harness) {
    let centre = junction_centre(street(harness));
    harness.key(KeyCode::KeyL);
    click(harness, centre);
    harness.key(KeyCode::Escape);
}

fn message(harness: &Harness) -> String {
    state(harness).message.clone().unwrap_or_default()
}

fn undo(harness: &mut Harness) {
    harness.key(KeyCode::Backspace);
}

fn redo(harness: &mut Harness) -> bool {
    harness
        .app
        .world_mut()
        .resource_scope(|world, mut state: Mut<PlayState>| {
            let mut loaded = world.resource_mut::<LoadedLevel>();
            let street = loaded.street.as_mut().expect("loaded");
            state.redo(street)
        })
}

#[test]
fn the_keys_select_the_bicycle_tools() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    for (key, tool) in [
        (KeyCode::KeyX, PlayTool::Separator),
        (KeyCode::KeyN, PlayTool::BicycleBox),
        (KeyCode::KeyG, PlayTool::BicycleLead),
    ] {
        harness.key(key);
        assert_eq!(state(&harness).tool, Some(tool));
        harness.key(key);
        assert_eq!(state(&harness).tool, None, "the key again drops the tool");
    }
}

#[test]
fn the_separator_tool_cycles_a_boundary_and_undo_takes_it_back() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let spot = boundary_point(street(&harness), 3);
    let hit = pick::boundary_at(street(&harness), spot).expect("a boundary there");
    assert_eq!((hit.segment, hit.boundary), (SEGMENT, 3));
    let first = pick::separator_on(street(&harness), &hit);
    harness.key(KeyCode::KeyX);
    click(&mut harness, spot);
    let second = pick::next_separator(first);
    let strips = |harness: &Harness| street(harness).segments[&SEGMENT].profile.strips.clone();
    assert_eq!(strips(&harness)[2].right, second);
    assert_eq!(strips(&harness)[3].left, second, "both strips agree");
    assert_eq!(state(&harness).design.commands.len(), 1);
    assert!(
        message(&harness).contains("separator"),
        "{}",
        message(&harness)
    );
    // Undo and redo.
    undo(&mut harness);
    assert_eq!(strips(&harness)[2].right, first);
    assert!(state(&harness).design.commands.is_empty());
    assert!(redo(&mut harness));
    assert_eq!(strips(&harness)[2].right, second);
    // A click that finds no boundary does nothing and tells so.
    click(&mut harness, Vec2::new(-500.0, -500.0));
    assert_eq!(state(&harness).design.commands.len(), 1);
    assert!(message(&harness).contains("between two strips"));
}

#[test]
fn the_separator_tool_shows_the_reason_when_the_street_refuses() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    // A bicycle track next to the car lane: it keeps a kerb, bollards or a planter.
    edit(
        &mut harness,
        DesignCommand::SetStripUse {
            segment: SEGMENT,
            strip: 3,
            use_: StripUse::BikeTrack,
            direction: Direction::Forward,
        },
    );
    let spot = boundary_point(street(&harness), 3);
    let hit = pick::boundary_at(street(&harness), spot).expect("a boundary there");
    assert_eq!(pick::separator_on(street(&harness), &hit), Separator::Kerb);
    harness.key(KeyCode::KeyX);
    click(&mut harness, spot); // kerb -> bollards
    click(&mut harness, spot); // bollards -> planter
    let kept = state(&harness).design.commands.len();
    click(&mut harness, spot); // planter -> none: refused
    assert_eq!(state(&harness).design.commands.len(), kept, "nothing kept");
    assert!(
        message(&harness).contains("Not possible here"),
        "{}",
        message(&harness)
    );
    let strips = &street(&harness).segments[&SEGMENT].profile.strips;
    assert_eq!(
        strips[2].right,
        Separator::Planter,
        "the street is unchanged"
    );
}

#[test]
fn the_box_tool_adds_and_removes_a_bicycle_box_and_the_view_draws_it() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    signals(&mut harness);
    let spot = approach_point(street(&harness));
    let layers = |harness: &Harness| {
        build_layers(street(harness))
            .iter()
            .map(|(shape, _)| shape.vertex_count())
            .collect::<Vec<_>>()
    };
    let before = layers(&harness);
    assert!(street(&harness).bicycle_boxes.is_empty());
    harness.key(KeyCode::KeyN);
    click(&mut harness, spot);
    let commands = state(&harness).design.commands.len();
    let bicycle_box = street(&harness)
        .bicycle_box(NODE, SEGMENT)
        .copied()
        .expect("a box");
    assert_eq!(bicycle_box.depth, pick::DEFAULT_BOX_DEPTH_M);
    // The view: a flat area per motor lane, and a stop line at the back of it.
    let (areas, lines) = bicycle_box_shapes(street(&harness), NODE, SEGMENT);
    assert!(!areas.is_empty() && areas.len() == lines.len());
    let after = layers(&harness);
    assert!(after[1] > before[1], "surfaces get the box");
    assert!(after[2] > before[2], "markings get the stop line");
    // A second click removes it; undo brings it back; redo removes it again.
    click(&mut harness, spot);
    assert!(street(&harness).bicycle_boxes.is_empty());
    undo(&mut harness);
    assert!(street(&harness).bicycle_box(NODE, SEGMENT).is_some());
    undo(&mut harness);
    assert!(street(&harness).bicycle_boxes.is_empty());
    assert_eq!(state(&harness).design.commands.len(), commands - 1);
    assert!(redo(&mut harness));
    assert!(street(&harness).bicycle_box(NODE, SEGMENT).is_some());
}

#[test]
fn the_box_tool_shows_the_reason_without_signals() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let spot = approach_point(street(&harness));
    harness.key(KeyCode::KeyN);
    click(&mut harness, spot);
    assert!(street(&harness).bicycle_boxes.is_empty());
    assert!(state(&harness).design.commands.is_empty());
    let text = message(&harness);
    assert!(text.contains("Not possible here"), "{text}");
    assert!(text.contains("signal"), "{text}");
    // The ghost says the same while the pointer is there.
    assert!(
        state(&harness)
            .hover_note
            .as_deref()
            .is_some_and(|note| note.contains("signal")),
        "{:?}",
        state(&harness).hover_note
    );
}

fn lead(harness: &Harness) -> f32 {
    pick::lead_of(&street(harness).junctions[&NODE])
}

#[test]
fn the_lead_tool_switches_the_leading_green_and_the_wheel_keys_change_it() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    signals(&mut harness);
    let centre = junction_centre(street(&harness));
    assert_eq!(lead(&harness), 0.0);
    harness.key(KeyCode::KeyG);
    click(&mut harness, centre);
    assert_eq!(
        lead(&harness),
        tf_sim::street::signals::DEFAULT_BICYCLE_LEAD_S
    );
    // The signal marks show a second light at every approach.
    assert!(
        pick::approach_marks(street(&harness))
            .iter()
            .all(|mark| mark.bicycle_lead_s > 0.0)
    );
    // `+` and `-` step it by a second, inside 0 to 6 s.
    harness.key(KeyCode::Equal);
    assert_eq!(lead(&harness), 5.0);
    harness.key(KeyCode::Equal);
    harness.key(KeyCode::Equal);
    assert_eq!(lead(&harness), 6.0, "held to the most");
    harness.key(KeyCode::Minus);
    assert_eq!(lead(&harness), 5.0);
    // A click takes it away; undo brings it back.
    click(&mut harness, centre);
    assert_eq!(lead(&harness), 0.0);
    undo(&mut harness);
    assert_eq!(lead(&harness), 5.0);
    assert!(redo(&mut harness) || state(&harness).redo.is_empty());
}

#[test]
fn the_lead_tool_shows_the_reason_without_a_signal_plan() {
    let _lock = save_lock();
    let mut harness = Harness::play(LEVEL);
    let centre = junction_centre(street(&harness));
    harness.key(KeyCode::KeyG);
    click(&mut harness, centre);
    assert_eq!(lead(&harness), 0.0);
    assert!(state(&harness).design.commands.is_empty());
    let text = message(&harness);
    assert!(text.contains("Not possible here"), "{text}");
    assert!(text.contains("signal plan"), "{text}");
}
