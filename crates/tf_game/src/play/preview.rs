//! Tool previews (Design phase): under the pointer, a ghost of what a click would do, so
//! the player sees before they commit.
//!
//! - Zebra: the ghost bars at the snapped place; on an existing crossing, a red outline
//!   (a click removes it).
//! - Speed limit: the segment lit, and the next limit painted on its lanes as the marking
//!   will show it.
//! - Bollards: the start dot on the nearest kerb; while dragging, the whole row at the real
//!   spacing; on an existing row, red rings (a click removes it).
//! - Signals: the junction lit, and the next mode in the info panel.
//!
//! A click the street would refuse is drawn red and the reason goes to the info panel
//! (`PlayState::hover_note`). The check is `Street::apply` on a clone, run only when the
//! snapped place, the tool or the street changes (a level is small, so it is cheap).
//! `build_preview` is a pure function, tested without a window.

use bevy::color::Alpha;
use bevy::prelude::*;
use tf_sim::roads::lane_builder;
use tf_sim::street::design::{Side, SignalTemplate};
use tf_sim::street::level::JunctionControl;
use tf_sim::street::model::{BOLLARD_SPACING, Crossing};
use tf_sim::street::{DesignCommand, Street};

use super::{PlayPhase, PlayState, PlayTool, PointerOverUi, pick};
use crate::draw::ShapeBuilder;
use crate::input::Pointer;
use crate::level::{LoadedLevel, PX_PER_M};
use crate::views::speed_signs::{segment_signs, sign_shape};
use crate::views::street::{carriageway, crossing_bars, points, to_bevy};
use crate::views::{ViewMaterial, palette, spawn_shape, z};

/// Where the button went down for a drag tool (the bollards).
#[derive(Resource, Default, Debug)]
pub struct DragStart(pub Option<Vec2>);

/// The one preview entity.
#[derive(Component)]
pub struct PreviewShape;

/// What the preview was last made for; it is remade when this changes or the street does.
#[derive(Resource, Default, Debug)]
pub struct Preview {
    key: Option<PreviewKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreviewKey {
    tool: PlayTool,
    at: IVec2,
    drag: Option<IVec2>,
}

/// A ghost: its shape (Bevy world px) and what to tell the player.
pub struct PreviewPlan {
    pub shape: ShapeBuilder,
    pub note: Option<String>,
}

/// Outline width of a ghost, metres.
const LINE_M: f32 = 0.15;
/// Radius of a ghost bollard dot, metres (a bit more than the real 0.1 m, to be seen).
const DOT_M: f32 = 0.3;
const RING_M: f32 = 0.6;
const SIDES: u32 = 12;

fn fill(ok: bool) -> Color {
    let base = if ok {
        palette::GHOST_OK
    } else {
        palette::GHOST_BAD
    };
    base.with_alpha(palette::GHOST_FILL_ALPHA)
}

fn line(ok: bool) -> Color {
    let base = if ok {
        palette::GHOST_OK
    } else {
        palette::GHOST_BAD
    };
    base.with_alpha(palette::GHOST_LINE_ALPHA)
}

/// The words for the next signal mode a click on a junction brings.
pub fn signals_next_text(command: &DesignCommand) -> String {
    match command {
        DesignCommand::SetJunctionControl {
            control: JunctionControl::PriorityToRight,
            ..
        } => "-> Priority to the right (no signals)".into(),
        DesignCommand::SetJunctionControl {
            template: Some(SignalTemplate::TwoPhase { green_s }),
            ..
        } => format!("-> Signals, 2 phases, green {green_s:.0} s"),
        DesignCommand::SetJunctionControl {
            template: Some(SignalTemplate::TwoPhaseWithWalk { green_s, walk_s }),
            ..
        } => format!(
            "-> Signals, 2 phases and a walk phase, green {green_s:.0} s, walk {walk_s:.0} s"
        ),
        DesignCommand::SetJunctionControl { .. } => "-> Signals".into(),
        _ => String::new(),
    }
}

/// Whether the street accepts `command` (tried on a copy), else why not.
fn refusal(street: &Street, command: &DesignCommand) -> Option<String> {
    street
        .clone()
        .apply(command)
        .err()
        .map(|error| format!("Not possible here: {error}."))
}

/// A row of bollard dots of the given side, from and to, as the street would place them.
pub fn bollard_dots(street: &Street, segment: i32, side: Side, from: f32, to: f32) -> Vec<Vec2> {
    let Some(seg) = street.segments.get(&segment) else {
        return Vec::new();
    };
    let Some((left, right)) = carriageway(&seg.profile) else {
        return Vec::new();
    };
    let kerb = match side {
        Side::Left => left,
        Side::Right => right,
    };
    let line = lane_builder::offset_polyline(&seg.centerline, f64::from(kerb));
    let gaps = ((to - from) / BOLLARD_SPACING).floor().max(1.0);
    let step = (to - from) / gaps;
    (0..=gaps as usize)
        .filter_map(|k| {
            let d = f64::from(from + step * k as f32);
            lane_builder::sub_polyline(&line, d, d + 0.01)
                .first()
                .copied()
        })
        .collect()
}

fn zebra_plan(street: &Street, at: Vec2) -> Option<PreviewPlan> {
    let command = pick::zebra_command(street, at)?;
    let mut shape = ShapeBuilder::new();
    let note = match &command {
        DesignCommand::RemoveCrossing { id } => {
            let crossing = street.crossings.get(id)?;
            for bar in crossing_bars(street, crossing) {
                let outline = points(&bar);
                shape.polygon(&outline, fill(false));
                shape.closed_polyline(&outline, LINE_M * PX_PER_M, line(false));
            }
            Some("Click to remove this zebra.".to_string())
        }
        DesignCommand::AddCrossing { crossing } => {
            let ghost = Crossing {
                id: -1,
                kind: crossing.kind,
                segment: crossing.segment,
                at: crossing.at,
                width: crossing.width,
            };
            let refused = refusal(street, &command);
            let ok = refused.is_none();
            for bar in crossing_bars(street, &ghost) {
                let outline = points(&bar);
                shape.polygon(&outline, fill(ok));
                shape.closed_polyline(&outline, LINE_M * PX_PER_M, line(ok));
            }
            refused
        }
        _ => return None,
    };
    Some(PreviewPlan { shape, note })
}

fn speed_plan(street: &Street, at: Vec2) -> Option<PreviewPlan> {
    let hit = pick::segment_at(street, at)?;
    let DesignCommand::SetSpeedLimit { kmh, .. } = pick::speed_command(street, at)? else {
        return None;
    };
    let segment = &street.segments[&hit.segment];
    let mut shape = ShapeBuilder::new();
    for index in 0..segment.profile.strips.len() {
        shape.polygon(
            &points(&segment.strip_polygon(index)),
            fill(true).with_alpha(0.22),
        );
    }
    for spot in segment_signs(street, hit.segment) {
        sign_shape(&mut shape, &spot, kmh, 0.85);
    }
    Some(PreviewPlan {
        shape,
        note: Some(format!(
            "Click: limit {:.0} -> {kmh:.0} km/h.",
            segment.speed_limit_kmh
        )),
    })
}

fn dots(shape: &mut ShapeBuilder, dots: &[Vec2], ok: bool) {
    for dot in dots {
        shape.circle(to_bevy(*dot), DOT_M * PX_PER_M, SIDES, line(ok));
    }
}

fn bollards_plan(street: &Street, at: Vec2, drag: Option<Vec2>) -> Option<PreviewPlan> {
    let mut shape = ShapeBuilder::new();
    if let Some(start) = drag {
        let command = pick::bollards_command(street, start, at);
        let Some(DesignCommand::AddBollards {
            segment,
            side,
            from,
            to,
        }) = &command
        else {
            // Too short, or not along one segment: only the start shows.
            let hit = pick::segment_at(street, start)?;
            let seg = &street.segments[&hit.segment];
            dots(
                &mut shape,
                &[pick::point_at(&seg.centerline, hit.along, hit.offset)],
                true,
            );
            return Some(PreviewPlan {
                shape,
                note: Some("Drag along one sidewalk edge.".into()),
            });
        };
        let refused = command.as_ref().and_then(|c| refusal(street, c));
        let ok = refused.is_none();
        dots(
            &mut shape,
            &bollard_dots(street, *segment, *side, *from, *to),
            ok,
        );
        return Some(PreviewPlan {
            shape,
            note: refused,
        });
    }
    if let Some(id) = pick::bollard_row_at(street, at) {
        let row = street.bollard_rows.get(&id)?;
        for obstacle in &row.obstacle_ids {
            if let Some(tf_sim::street::level::Shape::Circle(circle)) =
                street.obstacles.get(obstacle).map(|o| &o.shape)
            {
                let center = to_bevy(circle.center);
                shape.closed_polyline(
                    &(0..SIDES)
                        .map(|k| {
                            center
                                + Vec2::from_angle(std::f32::consts::TAU * k as f32 / SIDES as f32)
                                    * RING_M
                                    * PX_PER_M
                        })
                        .collect::<Vec<_>>(),
                    LINE_M * PX_PER_M,
                    line(false),
                );
            }
        }
        return Some(PreviewPlan {
            shape,
            note: Some("Click to remove this row of bollards.".into()),
        });
    }
    // The start of a row: the dot on the nearest kerb, at the snapped place.
    let hit = pick::segment_at(street, at)?;
    let seg = &street.segments[&hit.segment];
    let (left, right) = carriageway(&seg.profile)?;
    let kerb = if (hit.offset - left).abs() <= (hit.offset - right).abs() {
        left
    } else {
        right
    };
    let along = (hit.along / pick::CROSSING_SNAP_M).round() * pick::CROSSING_SNAP_M;
    dots(
        &mut shape,
        &[pick::point_at(&seg.centerline, along, kerb)],
        true,
    );
    Some(PreviewPlan {
        shape,
        note: Some("Drag along a sidewalk edge to place bollards.".into()),
    })
}

fn signals_plan(street: &Street, at: Vec2) -> Option<PreviewPlan> {
    let node = pick::junction_at(street, at)?;
    let command = pick::signals_click_command(street, at)?;
    let outline = points(&street.junctions[&node].polygon.points);
    let mut shape = ShapeBuilder::new();
    shape.polygon(&outline, fill(true).with_alpha(0.25));
    shape.closed_polyline(&outline, LINE_M * 2.0 * PX_PER_M, line(true));
    Some(PreviewPlan {
        shape,
        note: Some(signals_next_text(&command)),
    })
}

/// The ghost of what `tool` would do at `at` (street metres), or `None` when a click would
/// find nothing there. `drag` is where the button went down, for a drag tool.
pub fn build_preview(
    street: &Street,
    tool: PlayTool,
    at: Vec2,
    drag: Option<Vec2>,
) -> Option<PreviewPlan> {
    match tool {
        PlayTool::Zebra => zebra_plan(street, at),
        PlayTool::SpeedLimit => speed_plan(street, at),
        PlayTool::Bollards => bollards_plan(street, at, drag),
        PlayTool::Signals => signals_plan(street, at),
    }
}

fn round_key(p: Vec2) -> IVec2 {
    (p / pick::CROSSING_SNAP_M).round().as_ivec2()
}

/// Remakes the ghost when the pointer moves to a new snapped place, the tool or the drag
/// changes, or the street does. No tool, a pointer over a panel or another phase: no ghost.
#[allow(clippy::too_many_arguments)]
pub fn update_preview(
    phase: Res<State<PlayPhase>>,
    mut state: ResMut<PlayState>,
    pointer: Res<Pointer>,
    over_ui: Res<PointerOverUi>,
    drag: Res<DragStart>,
    loaded: Res<LoadedLevel>,
    mut preview: ResMut<Preview>,
    existing: Query<Entity, With<PreviewShape>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
) {
    let at = pointer.world / PX_PER_M;
    let key = match state.tool {
        Some(tool)
            if *phase.get() == PlayPhase::Design
                && pointer.in_window
                && !over_ui.0
                && loaded.street.is_some() =>
        {
            Some(PreviewKey {
                tool,
                at: round_key(at),
                drag: drag.0.map(round_key),
            })
        }
        _ => None,
    };
    if key == preview.key && !loaded.is_changed() {
        return;
    }
    preview.key = key;
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    let plan = key.and_then(|key| {
        let street = loaded.street.as_ref()?;
        build_preview(street, key.tool, at, drag.0)
    });
    let note = plan.as_ref().and_then(|plan| plan.note.clone());
    if state.hover_note != note {
        state.hover_note = note;
    }
    if let Some(plan) = plan
        && !plan.shape.is_empty()
    {
        let entity = spawn_shape(
            &mut commands,
            &mut meshes,
            &material,
            plan.shape,
            z::PLAY_PREVIEW,
        );
        commands.entity(entity).insert(PreviewShape);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tf_sim::street::{Design, LevelFile};

    fn street() -> Street {
        let level = LevelFile::load("carrefour-ecole").expect("loads");
        Street::from_level_and_design(&level, &Design::new(&level.id)).expect("builds")
    }

    /// A point on the carriageway of some segment, with no crossing on it.
    fn road_point(street: &Street) -> Vec2 {
        for segment in street.segments.values() {
            let length = lane_builder::polyline_length(&segment.centerline) as f32;
            let mut along = segment.start_trim + 4.0;
            while along < length - segment.end_trim - 4.0 {
                let p = pick::point_at(&segment.centerline, along, 0.0);
                if matches!(
                    pick::zebra_command(street, p),
                    Some(DesignCommand::AddCrossing { .. })
                ) {
                    return p;
                }
                along += 3.0;
            }
        }
        panic!("no road point");
    }

    #[test]
    fn a_zebra_ghost_stands_on_the_road_and_nowhere_else() {
        let street = street();
        let p = road_point(&street);
        let plan = build_preview(&street, PlayTool::Zebra, p, None).expect("a ghost");
        assert!(!plan.shape.is_empty());
        // Far outside the level there is no road: no ghost.
        assert!(build_preview(&street, PlayTool::Zebra, Vec2::new(-500.0, -500.0), None).is_none());
    }

    #[test]
    fn the_speed_ghost_shows_the_next_limit() {
        let street = street();
        let p = road_point(&street);
        let hit = pick::segment_at(&street, p).expect("on a segment");
        let now = street.segments[&hit.segment].speed_limit_kmh;
        let plan = build_preview(&street, PlayTool::SpeedLimit, p, None).expect("a ghost");
        let next = pick::next_speed_limit(now);
        assert!(plan.note.expect("a note").contains(&format!("{next:.0}")));
    }

    #[test]
    fn a_bollard_drag_previews_the_row_at_the_real_spacing() {
        let street = street();
        let segment = street
            .segments
            .values()
            .max_by(|a, b| a.centerline.len().cmp(&b.centerline.len()))
            .expect("a segment");
        let (from, to) = (segment.start_trim + 2.0, segment.start_trim + 12.0);
        let dots = bollard_dots(&street, segment.id, Side::Right, from, to);
        let gaps = ((to - from) / BOLLARD_SPACING).floor() as usize;
        assert_eq!(dots.len(), gaps + 1);
        let step = dots[0].distance(dots[1]);
        assert!(step <= BOLLARD_SPACING * 1.5 && step > 0.5, "{step}");
    }

    #[test]
    fn the_signals_ghost_names_the_next_mode() {
        let street = street();
        let node = street.junctions.keys().next().copied().expect("a junction");
        let polygon = &street.junctions[&node].polygon.points;
        let inside = polygon.iter().copied().sum::<Vec2>() / polygon.len() as f32;
        let plan = build_preview(&street, PlayTool::Signals, inside, None).expect("a ghost");
        let note = plan.note.expect("a note");
        assert!(note.starts_with("->"), "{note}");
    }
}
