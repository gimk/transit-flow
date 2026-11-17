//! The play mode screen, in plain `bevy_ui` text on flat panels (plan open point 6): an
//! info panel top left (the level and its history while designing, the clock and counts
//! while running), a key help line at the bottom, and a report panel in the middle once a
//! run is over. The texts are built by pure functions (tested below) and written to the
//! `Text` entities only when they change.

use bevy::prelude::*;
use bevy::text::{Font, FontSource};
use bevy::window::PrimaryWindow;
use tf_sim::street::run::{TICK_RATE, WalkSummary};

use super::{PlayPhase, PlayState, PlayTool, ReportData, end_text};
use crate::street_runner::{STREET_SPEED_MULTIPLIERS, StreetRunner};

const FONT_SIZE: f32 = 14.0;
const PANEL: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);
const TEXT: Color = Color::srgb(0.93, 0.93, 0.93);

/// On every play panel: a click over one is for the panel, not the map (`PointerOverUi`).
#[derive(Component)]
pub struct PlayPanel;

/// The font of the play screen: Fira Mono with the accented letters (the default font has
/// none). Licence next to the file, `assets/fonts/FiraMono-LICENSE` (SIL OFL).
#[derive(Resource, Default)]
pub struct PlayFont(pub Handle<Font>);

const FONT_BYTES: &[u8] = include_bytes!("../../assets/fonts/FiraMono-Medium.ttf");

/// The text of the info panel (top left).
#[derive(Component)]
pub struct InfoText;
/// The text of the key help line (bottom).
#[derive(Component)]
pub struct HelpText;
/// The panel of the report (hidden outside `Report`), and its text.
#[derive(Component)]
pub struct ReportPanel;
#[derive(Component)]
pub struct ReportText;

/// `m:ss` of a tick count.
pub fn clock_text(tick: u64) -> String {
    let seconds = tick / u64::from(TICK_RATE);
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub fn speed_text(speed_index: usize) -> String {
    match STREET_SPEED_MULTIPLIERS[speed_index.min(STREET_SPEED_MULTIPLIERS.len() - 1)] {
        0 => "paused".into(),
        n => format!("x{n}"),
    }
}

/// Design phase: the level, its history, the design and the active tool.
pub fn design_text(state: &PlayState) -> String {
    let level = &state.level;
    let mut text = format!("{} - {}\n", level.name, level.place);
    if !level.history.is_empty() {
        text += &format!("{}\n", level.history);
    }
    text += &format!(
        "\nDESIGN: {} edit(s){}\n",
        state.design.commands.len(),
        if state.redo.is_empty() {
            String::new()
        } else {
            format!(", {} to redo", state.redo.len())
        }
    );
    match state.tool {
        Some(tool) => text += &format!("Tool: {} - {}\n", tool.label(), tool.hint()),
        None => text += "Tool: none (pick one below)\n",
    }
    if let Some(message) = &state.message {
        text += &format!("\n{message}\n");
    }
    text += "\nPress Enter to start the run.";
    text
}

/// Run phase: the slice and clock, the counts, the speed.
pub fn run_text(runner: &StreetRunner) -> String {
    let tally = runner.tally();
    let view = runner.view_to();
    format!(
        "RUN - {} - {}\nSpeed {}\n\nOn the street: {} vehicles, {} on foot\nArrived: {} vehicles, {} on foot\nGave up: {}\nConflicts: {} ({} serious)\nInjuries: {}   Deaths: {}\n\nPress Enter to stop the run.",
        tally.slice.as_deref().unwrap_or("-"),
        clock_text(view.tick),
        speed_text(runner.speed_index()),
        view.agents.len(),
        view.walkers.len(),
        tally.arrived,
        tally.walkers_arrived,
        tally.gave_up,
        tally.conflicts(),
        tally.conflicts_serious,
        tally.injuries,
        tally.deaths,
    )
}

fn walkers_text(w: &WalkSummary) -> String {
    format!(
        "On foot: {} came, {} arrived, {} gave up, {} still walking\n  mean delay {:.1} s, mean wait at kerbs {:.1} s\n  contacts {} ({} light, {} serious injuries, {} deaths)",
        w.spawned,
        w.arrived,
        w.gave_up,
        w.still_walking,
        w.mean_delay_s,
        w.mean_kerb_wait_s,
        w.contacts,
        w.light_injuries,
        w.serious_injuries,
        w.deaths,
    )
}

/// The report: the summary of the run, then the lines other parts add (score, stars).
pub fn report_text(report: &ReportData) -> String {
    let s = &report.summary;
    let mut text = format!(
        "REPORT - {} ({})\n\nVehicles: {} came, {} arrived, {} gave up\n  mean delay {:.1} s\nConflicts: {} light, {} serious\nContacts: {} ({} light, {} serious injuries, {} deaths)\n\n{}\n",
        end_text(s.ended),
        clock_text(s.ticks),
        s.spawned,
        s.arrived,
        s.gave_up,
        s.mean_delay_s,
        s.conflicts_light,
        s.conflicts_serious,
        s.contacts,
        s.light_injuries,
        s.serious_injuries,
        s.deaths,
        walkers_text(&s.walkers),
    );
    // The score and the star verdict (next brief) are lines in `extra`.
    for line in &report.extra {
        text += &format!("\n{line}");
    }
    text += "\n\nPress Enter to design again.";
    text
}

/// The key help line of a phase.
pub fn help_text(phase: PlayPhase) -> String {
    match phase {
        PlayPhase::Design => {
            let tools: Vec<String> = PlayTool::ALL
                .iter()
                .map(|tool| format!("{} {}", tool.key_name(), tool.label()))
                .collect();
            format!(
                "{} | Esc no tool | Backspace or Ctrl+Z undo | Ctrl+Y redo | Enter start | WASD move, wheel zoom",
                tools.join(" | ")
            )
        }
        PlayPhase::Run => {
            "Space pause | 1 2 3 4: x1 x2 x4 x8 | Enter stop the run | WASD move, wheel zoom".into()
        }
        PlayPhase::Report => "Enter: back to the design | WASD move, wheel zoom".into(),
    }
}

fn panel(top: Val, left: Val, width: Val) -> Node {
    Node {
        position_type: PositionType::Absolute,
        top,
        left,
        width,
        padding: UiRect::all(Val::Px(10.0)),
        ..default()
    }
}

fn text_bundle(marker: impl Component, font: &Handle<Font>) -> impl Bundle {
    (
        marker,
        Text::new(""),
        TextFont {
            font: FontSource::Handle(font.clone()),
            font_size: FontSize::Px(FONT_SIZE),
            ..default()
        },
        TextColor(TEXT),
    )
}

fn load_font(mut commands: Commands, fonts: Option<ResMut<Assets<Font>>>) {
    // Without the text plugin (tests) there is no font asset to add.
    let handle = fonts.map_or_else(Handle::default, |mut fonts| {
        fonts.add(Font::from_bytes(FONT_BYTES.to_vec()))
    });
    commands.insert_resource(PlayFont(handle));
}

fn spawn_play_ui(mut commands: Commands, font: Res<PlayFont>) {
    let font = &font.0;
    commands.spawn((
        PlayPanel,
        panel(Val::Px(8.0), Val::Px(8.0), Val::Px(420.0)),
        BackgroundColor(PANEL),
        children![text_bundle(InfoText, font)],
    ));
    commands.spawn((
        PlayPanel,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(0.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(PANEL),
        children![text_bundle(HelpText, font)],
    ));
    commands.spawn((
        ReportPanel,
        PlayPanel,
        Node {
            display: Display::None,
            ..panel(Val::Percent(15.0), Val::Percent(28.0), Val::Percent(44.0))
        },
        BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.92)),
        children![text_bundle(ReportText, font)],
    ));
}

/// Whether the cursor is over a play panel (their screen rectangles).
pub fn update_pointer_over_ui(
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<PlayPanel>>,
    mut over: ResMut<super::PointerOverUi>,
) {
    let hit = window.is_some_and(|window| {
        window.cursor_position().is_some_and(|cursor| {
            let physical = cursor * window.scale_factor();
            panels
                .iter()
                .any(|(node, transform)| node.contains_point(*transform, physical))
        })
    });
    if over.0 != hit {
        over.0 = hit;
    }
}

fn set_text(text: &mut Text, value: String) {
    if text.0 != value {
        text.0 = value;
    }
}

#[allow(clippy::type_complexity)]
fn update_play_ui(
    phase: Res<State<PlayPhase>>,
    state: Res<PlayState>,
    runner: Res<StreetRunner>,
    mut info: Query<&mut Text, (With<InfoText>, Without<HelpText>, Without<ReportText>)>,
    mut help: Query<&mut Text, (With<HelpText>, Without<InfoText>, Without<ReportText>)>,
    mut report: Query<&mut Text, (With<ReportText>, Without<InfoText>, Without<HelpText>)>,
    mut panels: Query<&mut Node, With<ReportPanel>>,
) {
    let phase = *phase.get();
    let info_text = match phase {
        PlayPhase::Design => design_text(&state),
        PlayPhase::Run => run_text(&runner),
        // The report has its own panel; this one only names the level.
        PlayPhase::Report => format!("{} - {}", state.level.name, state.level.place),
    };
    for mut text in &mut info {
        set_text(&mut text, info_text.clone());
    }
    for mut text in &mut help {
        set_text(&mut text, help_text(phase));
    }
    let report_text = match (&state.report, phase) {
        (Some(report), PlayPhase::Report) => report_text(report),
        _ => String::new(),
    };
    for mut text in &mut report {
        set_text(&mut text, report_text.clone());
    }
    let display = if phase == PlayPhase::Report {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut panels {
        if node.display != display {
            node.display = display;
        }
    }
}

pub struct PlayUiPlugin;

impl Plugin for PlayUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (load_font, spawn_play_ui).chain())
            .add_systems(Update, update_play_ui.run_if(resource_exists::<PlayState>));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tf_sim::street::LevelFile;
    use tf_sim::street::run::{EndReason, Summary};

    #[test]
    fn the_clock_reads_minutes_and_seconds() {
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(20 * 75), "1:15");
    }

    #[test]
    fn the_design_text_names_the_level_the_tool_and_the_last_message() {
        let level = LevelFile::load("carrefour-ecole").expect("loads");
        let mut state = PlayState::new(level.clone());
        state.tool = Some(PlayTool::Zebra);
        state.message = Some("Not possible here.".into());
        let text = design_text(&state);
        assert!(text.contains(&level.name) && text.contains(&level.place));
        assert!(text.contains("Zebra crossing") && text.contains("Not possible here."));
        assert!(text.contains("0 edit(s)"));
    }

    #[test]
    fn the_report_shows_the_summary_and_the_extra_lines() {
        let level = LevelFile::load("carrefour-ecole").expect("loads");
        let mut run = tf_sim::street::run::StreetRun::new(
            &level,
            &tf_sim::street::Design::new(&level.id),
            tf_sim::street::run::RunOptions::for_level(&level),
        )
        .expect("builds");
        let summary: Summary = run.summary();
        assert_eq!(summary.ended, None);
        run.run_to_end();
        let summary = run.summary();
        assert_eq!(summary.ended, Some(EndReason::Completed));
        let report = ReportData {
            summary,
            extra: vec!["Score: 42".into()],
        };
        let text = report_text(&report);
        assert!(text.contains("the day is over") && text.contains("Score: 42"));
        assert!(text.contains("Vehicles:") && text.contains("On foot:"));
    }
}
