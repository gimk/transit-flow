//! The inspector (play mode, Run and Report): hover a car or a pedestrian and a small card
//! next to the pointer tells who it is and what it does now, and an outline marks it.
//!
//! - `pick_agent` is the pure picking helper: from the two snapshots around the render
//!   clock and a point of the street it finds the agent drawn there. Walkers win over cars
//!   (they are small), and both get a margin so a thin body can be hit.
//! - The static traits (archetype, compliance, attention, ...) are not in the per-tick
//!   snapshot: the runner keeps them from the snapshot of each agent's spawn
//!   (`StreetRunner::info`). The card joins them with the current pose.
//! - In Report the runner still holds its last frame, so the card works there too.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use tf_sim::street::Street;
use tf_sim::street::ids::AgentId;

use super::pick;
use super::ui::{PlayFont, text_bundle};
use super::{PlayPhase, PointerOverUi};
use crate::draw::ShapeBuilder;
use crate::input::Pointer;
use crate::level::{LoadedLevel, PX_PER_M};
use crate::snapshot::lerp_angle;
use crate::street_runner::{
    AgentInfo, AgentPose, CarInfo, StreetRunner, StreetSnapshot, WalkerInfo, WalkerPose,
};
use crate::views::street::to_bevy;
use crate::views::{ViewMaterial, palette, z};

/// A car is hit this far outside its body, metres.
pub const CAR_PICK_MARGIN_M: f32 = 0.6;
/// A walker is hit at least this far from its centre, metres (a body is 0.25 m).
pub const WALKER_PICK_RADIUS_M: f32 = 0.9;
/// The outline stands this far outside the body, and is this wide, metres.
const OUTLINE_GAP_M: f32 = 0.25;
const OUTLINE_WIDTH_M: f32 = 0.18;
/// km/h in one m/s.
const KMH_PER_MS: f32 = 3.6;

/// What the pointer is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hovered {
    Car(AgentId),
    Walker(AgentId),
}

/// The agent under the pointer, if any (set every frame by `update_hover`).
#[derive(Resource, Default, Debug)]
pub struct Hover(pub Option<Hovered>);

/// Where a car is drawn: its pose `alpha` of the way from `from` to `to`.
pub fn car_pose_at(from: &StreetSnapshot, pose: &AgentPose, alpha: f32) -> (Vec2, Vec2) {
    match from.index_of(pose.id) {
        Some(j) => {
            let before = &from.agents[j];
            let angle = lerp_angle(before.dir.to_angle(), pose.dir.to_angle(), alpha);
            (before.pos.lerp(pose.pos, alpha), Vec2::from_angle(angle))
        }
        None => (pose.pos, pose.dir),
    }
}

/// Where a walker is drawn.
pub fn walker_pos_at(from: &StreetSnapshot, pose: &WalkerPose, alpha: f32) -> Vec2 {
    match from.walker_index_of(pose.id) {
        Some(j) => from.walkers[j].pos.lerp(pose.pos, alpha),
        None => pose.pos,
    }
}

/// The agent drawn at `at` (street metres): a walker within its pick radius, else the car
/// whose body (plus a margin) holds the point. Of several, the nearest centre.
pub fn pick_agent(
    from: &StreetSnapshot,
    to: &StreetSnapshot,
    alpha: f32,
    at: Vec2,
) -> Option<Hovered> {
    let walker = to
        .walkers
        .iter()
        .filter_map(|pose| {
            let distance = walker_pos_at(from, pose, alpha).distance(at);
            (distance <= pose.radius.max(WALKER_PICK_RADIUS_M)).then_some((distance, pose.id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, id)) = walker {
        return Some(Hovered::Walker(id));
    }
    to.agents
        .iter()
        .filter_map(|pose| {
            let (pos, dir) = car_pose_at(from, pose, alpha);
            let d = at - pos;
            let along = d.dot(dir);
            let across = d.dot(Vec2::new(-dir.y, dir.x));
            let hit = along.abs() <= pose.length * 0.5 + CAR_PICK_MARGIN_M
                && across.abs() <= pose.width * 0.5 + CAR_PICK_MARGIN_M;
            hit.then_some((d.length(), pose.id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| Hovered::Car(id))
}

fn kmh(ms: f32) -> f32 {
    ms * KMH_PER_MS
}

/// The speed limit where a car is, or `None` inside a junction.
pub fn limit_at(street: &Street, pos: Vec2) -> Option<f32> {
    pick::segment_at(street, pos).map(|hit| street.segments[&hit.segment].speed_limit_kmh)
}

/// The card of a car: who, how fast against the limit, its traits, its state.
pub fn car_card(pose: &AgentPose, info: Option<&CarInfo>, limit_kmh: Option<f32>) -> String {
    let mut text = format!(
        "Vehicle #{} - {:?}, {}\n",
        pose.id.0, pose.mode, pose.archetype
    );
    text += &match limit_kmh {
        Some(limit) => format!("Speed {:.0} km/h (limit {limit:.0})\n", kmh(pose.speed)),
        None => format!("Speed {:.0} km/h (in the junction)\n", kmh(pose.speed)),
    };
    if let (Some(info), Some(limit)) = (info, limit_kmh) {
        text += &format!(
            "Wants {:.0} km/h (x{:.2} of the limit)\n",
            limit * info.speed_factor as f32,
            info.speed_factor
        );
    }
    if let Some(info) = info {
        text += &format!(
            "Compliance {:.2}  Attention {:.2}\nReaction {:.2} s  Risk {:.2}  Courtesy {:.2}\n",
            info.compliance, info.attention, info.reaction_s, info.risk, info.courtesy
        );
        text += &format!(
            "Frustration {:.2}  Patience {:.0} s\n",
            pose.frustration, info.patience_s
        );
    }
    text += &format!("State: {}", car_state(pose));
    text
}

/// A car's state in words, the most telling first.
pub fn car_state(pose: &AgentPose) -> &'static str {
    if pose.crashed {
        "crashed"
    } else if pose.held_by_walkers {
        "held by people on foot"
    } else if pose.distracted {
        "distracted"
    } else if pose.waiting {
        "waiting"
    } else {
        "driving"
    }
}

/// The card of a pedestrian. `speed` is its speed now, m/s.
pub fn walker_card(pose: &WalkerPose, info: Option<&WalkerInfo>, speed: f32) -> String {
    let group = info.map_or(1, |i| i.group_size);
    let mut text = format!("Pedestrian #{} - {}", pose.id.0, pose.archetype);
    if group > 1 {
        text += &format!(", group of {group}");
    }
    text += "\n";
    text += &match info {
        Some(info) => format!(
            "Speed {:.1} km/h (wants {:.1})\n",
            kmh(speed),
            kmh(info.desired_speed as f32)
        ),
        None => format!("Speed {:.1} km/h\n", kmh(speed)),
    };
    if let Some(info) = info {
        text += &format!(
            "Compliance {:.2}  Attention {:.2}\nReaction {:.2} s  Risk {:.2}  Courtesy {:.2}\nPatience {:.0} s\n",
            info.compliance,
            info.attention,
            info.reaction_s,
            info.risk,
            info.courtesy,
            info.patience_s
        );
    }
    text += &format!("State: {}\n", walker_state(pose));
    text += if pose.off_crossing {
        "Way: free (crosses out of a crossing)"
    } else {
        "Way: by the rules (crossings)"
    };
    text
}

/// A pedestrian's state in words, the most telling first.
pub fn walker_state(pose: &WalkerPose) -> &'static str {
    if pose.hit {
        "hit"
    } else if pose.walked_on_red {
        "walked on red"
    } else if pose.on_carriageway && pose.off_crossing {
        "on the carriageway, off a crossing"
    } else if pose.on_carriageway {
        "on the carriageway"
    } else if pose.waiting {
        "waiting at the kerb"
    } else {
        "walking"
    }
}

/// The text of the card of what is hovered, from the newest snapshot.
pub fn card_text(
    hovered: Hovered,
    runner: &StreetRunner,
    street: Option<&Street>,
) -> Option<String> {
    let (from, to, alpha) = (runner.view_from(), runner.view_to(), runner.view_alpha());
    match hovered {
        Hovered::Car(id) => {
            let pose = &to.agents[to.index_of(id)?];
            let (pos, _) = car_pose_at(from, pose, alpha);
            let info = match runner.info(id) {
                Some(AgentInfo::Car(car)) => Some(car),
                _ => None,
            };
            let limit = street.and_then(|street| limit_at(street, pos));
            Some(car_card(pose, info, limit))
        }
        Hovered::Walker(id) => {
            let pose = &to.walkers[to.walker_index_of(id)?];
            let info = match runner.info(id) {
                Some(AgentInfo::Walker(walker)) => Some(walker),
                _ => None,
            };
            let step = pose.pos - pose.prev_pos;
            let speed = step.length() * tf_sim::street::run::TICK_RATE as f32;
            Some(walker_card(pose, info, speed))
        }
    }
}

/// The outline of what is hovered, in Bevy world px.
pub fn outline_shape(hovered: Hovered, runner: &StreetRunner) -> Option<ShapeBuilder> {
    let (from, to, alpha) = (runner.view_from(), runner.view_to(), runner.view_alpha());
    let width = OUTLINE_WIDTH_M * PX_PER_M;
    let mut shape = ShapeBuilder::new();
    match hovered {
        Hovered::Car(id) => {
            let pose = &to.agents[to.index_of(id)?];
            let (pos, dir) = car_pose_at(from, pose, alpha);
            let side = Vec2::new(-dir.y, dir.x);
            let (hl, hw) = (
                pose.length * 0.5 + OUTLINE_GAP_M,
                pose.width * 0.5 + OUTLINE_GAP_M,
            );
            let corners: Vec<Vec2> = [(hl, hw), (hl, -hw), (-hl, -hw), (-hl, hw)]
                .iter()
                .map(|(l, w)| to_bevy(pos + dir * *l + side * *w))
                .collect();
            shape.closed_polyline(&corners, width, palette::HOVER_OUTLINE);
        }
        Hovered::Walker(id) => {
            let pose = &to.walkers[to.walker_index_of(id)?];
            let pos = walker_pos_at(from, pose, alpha);
            let radius = pose.radius + OUTLINE_GAP_M;
            let ring: Vec<Vec2> = (0..16)
                .map(|k| {
                    let angle = std::f32::consts::TAU * k as f32 / 16.0;
                    to_bevy(pos + Vec2::from_angle(angle) * radius)
                })
                .collect();
            shape.closed_polyline(&ring, width, palette::HOVER_OUTLINE);
        }
    }
    Some(shape)
}

/// Sets `Hover` from the pointer: only while a run is on screen (Run or Report).
pub fn update_hover(
    phase: Res<State<PlayPhase>>,
    pointer: Res<Pointer>,
    over_ui: Res<PointerOverUi>,
    runner: Res<StreetRunner>,
    mut hover: ResMut<Hover>,
) {
    let active = matches!(*phase.get(), PlayPhase::Run | PlayPhase::Report)
        && runner.is_running()
        && pointer.in_window
        && !over_ui.0;
    let found = if active {
        pick_agent(
            runner.view_from(),
            runner.view_to(),
            runner.view_alpha(),
            pointer.world / PX_PER_M,
        )
    } else {
        None
    };
    if hover.0 != found {
        hover.0 = found;
    }
}

/// The card.
#[derive(Component)]
pub struct InspectorCard;
#[derive(Component)]
pub struct InspectorText;
/// The entity with the outline mesh.
#[derive(Component)]
pub struct HoverOutline(pub Handle<Mesh>);

/// The card's panel is not a `PlayPanel`: it follows the pointer, so it must not count as
/// "the pointer is over the UI".
pub fn spawn_card(
    mut commands: Commands,
    font: Res<PlayFont>,
    material: Option<Res<ViewMaterial>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
) {
    commands.spawn((
        InspectorCard,
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            padding: UiRect::all(Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(palette::PANEL),
        GlobalZIndex(10),
        children![text_bundle(InspectorText, &font.0)],
    ));
    if let (Some(material), Some(mut meshes)) = (material, meshes) {
        let handle = meshes.add(Mesh::from(Rectangle::new(1.0, 1.0)));
        commands.spawn((
            HoverOutline(handle.clone()),
            Mesh2d(handle),
            MeshMaterial2d(material.0.clone()),
            Transform::from_xyz(0.0, 0.0, z::HOVER_OUTLINE),
            Visibility::Hidden,
            NoFrustumCulling,
        ));
    }
}

/// Writes the card and the outline for what is hovered, and puts the card by the cursor.
#[allow(clippy::too_many_arguments)]
fn update_card(
    hover: Res<Hover>,
    runner: Res<StreetRunner>,
    loaded: Res<LoadedLevel>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    mut card: Query<&mut Node, With<InspectorCard>>,
    mut text: Query<&mut Text, With<InspectorText>>,
    mut outline: Query<(&HoverOutline, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let shown = hover.0.and_then(|hovered| {
        let text = card_text(hovered, &runner, loaded.street.as_ref())?;
        Some((text, outline_shape(hovered, &runner)?))
    });
    for mut node in &mut card {
        let display = if shown.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Some(cursor) = window.as_ref().and_then(|w| w.cursor_position()) {
            let size = window.as_ref().map_or(Vec2::splat(f32::MAX), |w| w.size());
            // Right and below the pointer; flipped to the other side near the window edge.
            let left = if cursor.x + 340.0 > size.x {
                cursor.x - 330.0
            } else {
                cursor.x + 16.0
            };
            let top = if cursor.y + 200.0 > size.y {
                cursor.y - 190.0
            } else {
                cursor.y + 16.0
            };
            node.left = Val::Px(left.max(0.0));
            node.top = Val::Px(top.max(0.0));
        }
    }
    if let Some((new_text, _)) = &shown {
        for mut text in &mut text {
            if text.0 != *new_text {
                text.0 = new_text.clone();
            }
        }
    }
    for (outline, mut visibility) in &mut outline {
        let visible = match &shown {
            Some((_, shape)) => {
                if let Some(mut mesh) = meshes.get_mut(&outline.0) {
                    *mesh = shape.clone().build();
                }
                Visibility::Inherited
            }
            None => Visibility::Hidden,
        };
        if *visibility != visible {
            *visibility = visible;
        }
    }
}

pub struct InspectPlugin;

impl Plugin for InspectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hover>().add_systems(
            Update,
            (update_hover, update_card)
                .chain()
                .run_if(resource_exists::<super::PlayState>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(id: i32, pos: Vec2) -> AgentPose {
        AgentPose {
            id: AgentId(id),
            mode: tf_sim::street::level::Mode::Car,
            archetype: "careful",
            pos,
            dir: Vec2::X,
            prev_pos: pos,
            prev_dir: Vec2::X,
            length: 4.3,
            width: 1.8,
            speed: 8.0,
            frustration: 0.0,
            waiting: false,
            crashed: false,
            distracted: false,
            held_by_walkers: false,
        }
    }

    fn walker(id: i32, pos: Vec2) -> WalkerPose {
        WalkerPose {
            id: AgentId(id),
            archetype: "child",
            group: AgentId(id),
            pos,
            prev_pos: pos,
            dir: Vec2::ZERO,
            radius: 0.25,
            waiting: false,
            on_carriageway: false,
            off_crossing: false,
            walked_on_red: false,
            hit: false,
        }
    }

    fn snapshot(agents: Vec<AgentPose>, walkers: Vec<WalkerPose>) -> StreetSnapshot {
        StreetSnapshot {
            agents,
            walkers,
            ..Default::default()
        }
    }

    #[test]
    fn a_point_on_a_car_picks_it_and_a_point_far_away_picks_nothing() {
        let s = snapshot(vec![car(1, Vec2::new(10.0, 10.0))], vec![]);
        assert_eq!(
            pick_agent(&s, &s, 0.0, Vec2::new(11.5, 10.5)),
            Some(Hovered::Car(AgentId(1)))
        );
        // Across the body and the margin.
        assert_eq!(pick_agent(&s, &s, 0.0, Vec2::new(10.0, 12.0)), None);
        // Past the nose and the margin.
        assert_eq!(pick_agent(&s, &s, 0.0, Vec2::new(13.2, 10.0)), None);
        // The margin picks a point just off the body.
        assert!(pick_agent(&s, &s, 0.0, Vec2::new(10.0, 11.3)).is_some());
    }

    #[test]
    fn a_walker_beside_a_car_wins_the_pick_and_the_nearest_car_wins_among_cars() {
        let s = snapshot(
            vec![car(1, Vec2::new(0.0, 0.0)), car(2, Vec2::new(3.0, 0.0))],
            vec![walker(1003, Vec2::new(1.0, 0.8))],
        );
        assert_eq!(
            pick_agent(&s, &s, 0.0, Vec2::new(1.0, 0.8)),
            Some(Hovered::Walker(AgentId(1003)))
        );
        // Between the cars, nearer the second.
        assert_eq!(
            pick_agent(&s, &s, 0.0, Vec2::new(2.2, 0.0)),
            Some(Hovered::Car(AgentId(2)))
        );
    }

    #[test]
    fn the_pick_follows_the_drawn_position_between_snapshots() {
        let from = snapshot(vec![car(1, Vec2::new(0.0, 0.0))], vec![]);
        let to = snapshot(vec![car(1, Vec2::new(10.0, 0.0))], vec![]);
        assert_eq!(
            pick_agent(&from, &to, 0.5, Vec2::new(5.0, 0.0)),
            Some(Hovered::Car(AgentId(1)))
        );
        assert_eq!(pick_agent(&from, &to, 0.5, Vec2::new(0.0, 0.0)), None);
    }

    #[test]
    fn the_cards_name_the_agent_and_its_state() {
        let mut pose = car(7, Vec2::ZERO);
        pose.waiting = true;
        let text = car_card(&pose, None, Some(30.0));
        assert!(text.contains("#7") && text.contains("careful"), "{text}");
        assert!(text.contains("29 km/h (limit 30)"), "{text}");
        assert!(text.contains("State: waiting"), "{text}");
        pose.crashed = true;
        assert_eq!(car_state(&pose), "crashed");

        let mut w = walker(1003, Vec2::ZERO);
        w.off_crossing = true;
        w.on_carriageway = true;
        let text = walker_card(&w, None, 1.2);
        assert!(text.contains("#1003") && text.contains("child"), "{text}");
        assert!(
            text.contains("off a crossing") && text.contains("free"),
            "{text}"
        );
    }
}
