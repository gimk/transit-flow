//! The status line (plan §2.3, M10.3): one line of plain text across the top of the window
//! with the day and time, speed, Flow, traffic, the active tool and its hint, and the last
//! notification. It replaces Godot's top bar, city label, palette hint and notifications
//! (`ui/hud.gd`); there is no palette and no debug panel.
//!
//! The text is rebuilt only when something on it changes (a new tick snapshot, the tool, its
//! variant or hint, the speed, the notification), and the `Text` component is only written
//! when the string really differs.

use std::sync::Arc;

use bevy::prelude::*;

use crate::events::NotificationPosted;
use crate::runner::SimHandle;
use crate::snapshot::ViewSnapshot;
use crate::tools::ToolManager;

/// How long a notification stays (`Hud.NOTIFICATION_SECONDS`). Godot faded it out over 0.4 s
/// after that; here the line just drops it.
pub const NOTIFICATION_SECONDS: f32 = 2.0;
const FONT_SIZE: f32 = 13.0;
/// A dark band behind the text, so it reads over the map.
const BAND: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
const TEXT: Color = Color::srgb(0.93, 0.93, 0.93);

/// The one `Text` of the status line.
#[derive(Component, Debug)]
pub struct StatusLineText;

/// The notification on the line and the time it has left.
#[derive(Resource, Default, Debug)]
pub struct StatusNotice {
    pub text: Option<String>,
    pub seconds_left: f32,
}

/// What the line was last built from, so it is only rebuilt when one of these changes.
#[derive(Default)]
struct Shown {
    view: Option<Arc<ViewSnapshot>>,
    tool: Option<&'static str>,
    variant: Option<String>,
    hint: String,
    speed_index: usize,
    lagging: bool,
    notice: Option<String>,
}

pub struct StatusLinePlugin;

impl Plugin for StatusLinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StatusNotice>()
            .add_systems(Startup, spawn_status_line)
            // After the frame's messages and tool changes, so the line shows them at once.
            .add_systems(PostUpdate, update_status_line);
    }
}

fn spawn_status_line(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(BAND),
        children![(
            StatusLineText,
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(FONT_SIZE),
                ..default()
            },
            TextColor(TEXT),
        )],
    ));
}

fn update_status_line(
    time: Res<Time<Real>>,
    handle: Res<SimHandle>,
    tools: Res<ToolManager>,
    mut notice: ResMut<StatusNotice>,
    mut posted: MessageReader<NotificationPosted>,
    mut shown: Local<Shown>,
    mut lines: Query<&mut Text, With<StatusLineText>>,
) {
    for NotificationPosted(text) in posted.read() {
        notice.text = Some(text.clone());
        notice.seconds_left = NOTIFICATION_SECONDS;
    }
    if notice.text.is_some() {
        notice.seconds_left -= time.delta_secs();
        if notice.seconds_left <= 0.0 {
            notice.text = None;
        }
    }

    let view = &handle.view_to;
    let unchanged = shown
        .view
        .as_ref()
        .is_some_and(|last| Arc::ptr_eq(last, view))
        && shown.tool == tools.active_id()
        && shown.variant == tools.variant()
        && shown.hint == tools.hint()
        && shown.speed_index == handle.speed_index()
        && shown.lagging == handle.sim_lagging
        && shown.notice == notice.text;
    if unchanged {
        return;
    }

    let line = status_text(&handle, &tools, notice.text.as_deref());
    for mut text in &mut lines {
        if text.0 != line {
            text.0.clone_from(&line);
        }
    }
    *shown = Shown {
        view: Some(view.clone()),
        tool: tools.active_id(),
        variant: tools.variant(),
        hint: tools.hint().to_string(),
        speed_index: handle.speed_index(),
        lagging: handle.sim_lagging,
        notice: notice.text.clone(),
    };
}

/// The line, e.g. `Day 2, 00:00 | 1x | Flow 74% | 213 vehicles (+4 waiting) | 0 stuck |
/// tick 2.1 ms | Road (local_2lane): drag to build | > Game saved`. Pure, so tests can check
/// it. The day and the time come from the same snapshot, so the day changes at 00:00.
pub fn status_text(handle: &SimHandle, tools: &ToolManager, notice: Option<&str>) -> String {
    let view = &handle.view_to;
    let mut parts = vec![
        format!("Day {}, {}", view.day, view.time_text),
        if handle.is_paused() {
            "paused".to_string()
        } else {
            format!("{}x", handle.multiplier())
        },
        flow_text(view.flow_percent),
        format!(
            "{} vehicles (+{} waiting)",
            view.vehicle_count, view.waiting_count
        ),
        format!("{} stuck", view.stuck_count),
    ];
    let mut tick = format!("tick {:.1} ms", handle.average_tick_ms);
    if handle.sim_lagging {
        tick += &format!(" | sim at {:.1}x (can't keep up)", handle.effective_speed);
    }
    parts.push(tick);
    if let Some(id) = tools.active_id() {
        let mut tool = tool_name(id).to_string();
        if let Some(variant) = tools.variant() {
            tool += &format!(" ({variant})");
        }
        let hint = tools.hint();
        parts.push(if hint.is_empty() {
            tool
        } else {
            format!("{tool}: {}", hint.replace('·', "|").replace('→', "->"))
        });
    }
    let mut line = parts.join(" | ");
    if let Some(notice) = notice {
        line += " | > ";
        line += notice;
    }
    line
}

/// `Flow -` until a trip has ended (`flow_percent` is -1 then), like the HUD.
fn flow_text(percent: f64) -> String {
    if percent < 0.0 {
        "Flow -".to_string()
    } else {
        format!("Flow {}%", percent.round() as i64)
    }
}

/// `RoadTool` -> `Road`, `ResidentialZoneTool` -> `ResidentialZone`.
fn tool_name(id: &str) -> &str {
    id.strip_suffix("Tool").unwrap_or(id)
}
