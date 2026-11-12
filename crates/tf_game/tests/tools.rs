//! The build tools (M10), driven through the `Pointer` resource, mouse button messages and
//! keys, as `tests/integration/test_build_tools.gd` drove the main scene. The palette, top
//! bar, hint-position and debug-menu tests of that file go away with the UI.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use tf_game::events::*;
use tf_game::input::Pointer;
use tf_game::plugin::GamePlugin;
use tf_game::runner::SimHandle;
use tf_game::tools::{Shape, ToolManager, ToolPreview};
use tf_game::views::ViewsPlugin;
use tf_game::views::grid::ShowGrid;
use tf_sim::grid;
use tf_sim::roads::RoadType;

struct Harness {
    app: App,
}

fn local() -> &'static RoadType {
    RoadType::load_by_id("local_2lane").unwrap()
}

/// A grid point a few cells away from the map centre.
fn grid_point(dx: i32, dy: i32) -> Vec2 {
    grid::snap_to_point(grid::map_rect().center()) + Vec2::new(dx as f32, dy as f32) * 64.0
}

impl Harness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::input::InputPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<ColorMaterial>()
        .add_plugins((GamePlugin { threaded: false }, ViewsPlugin));
        app.update();
        let mut harness = Self { app };
        // Start from an empty map without the highway.
        harness.sim(|sim| *sim = tf_sim::Simulation::new());
        harness.frames(2);
        harness
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update();
        }
    }

    /// Runs `f` on the simulation (waits for the worker, if any).
    fn sim<R>(&mut self, f: impl FnOnce(&mut tf_sim::Simulation) -> R) -> R {
        let mut handle = self.app.world_mut().resource_mut::<SimHandle>();
        f(handle.get_mut())
    }

    /// Builds a road directly, like a scenario does.
    fn road(&mut self, from: Vec2, to: Vec2) {
        self.sim(|sim| {
            sim.call(|sim, out| {
                sim.roads.add_road(from, to, local(), 0, out);
            })
        });
        self.frames(2);
    }

    fn key(&mut self, code: KeyCode) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            self.app.world_mut().write_message(KeyboardInput {
                key_code: code,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            self.frames(1);
        }
    }

    fn move_to(&mut self, world: Vec2) {
        self.app
            .world_mut()
            .resource_mut::<Pointer>()
            .set_world(world);
        self.frames(1);
    }

    fn button(&mut self, button: MouseButton, state: ButtonState) {
        self.app.world_mut().write_message(MouseButtonInput {
            button,
            state,
            window: Entity::PLACEHOLDER,
        });
        self.frames(1);
    }

    fn left(&mut self, state: ButtonState) {
        self.button(MouseButton::Left, state);
    }

    fn drag(&mut self, from: Vec2, to: Vec2) {
        self.move_to(from);
        self.left(ButtonState::Pressed);
        self.move_to(to);
        self.left(ButtonState::Released);
        self.frames(2);
    }

    fn click(&mut self, at: Vec2, button: MouseButton) {
        self.move_to(at);
        self.button(button, ButtonState::Pressed);
        self.button(button, ButtonState::Released);
    }

    fn active_tool(&self) -> Option<&'static str> {
        self.app.world().resource::<ToolManager>().active_id()
    }

    fn tool_variant(&self) -> Option<String> {
        self.app.world().resource::<ToolManager>().variant()
    }

    fn hint(&self) -> String {
        self.app
            .world()
            .resource::<ToolManager>()
            .hint()
            .to_string()
    }

    fn grid_shown(&self) -> bool {
        self.app.world().resource::<ShowGrid>().0
    }

    fn segments(&mut self) -> usize {
        self.sim(|sim| sim.roads.segments.len())
    }

    /// Messages of type `M` written since the last call (reads them all).
    fn messages<M: Message + Clone>(&mut self) -> Vec<M> {
        let cursor = self.app.world_mut().resource_mut::<Messages<M>>();
        cursor.iter_current_update_messages().cloned().collect()
    }
}

#[test]
fn road_tool_builds_axis_locked_road_from_drag() {
    let mut h = Harness::new();
    h.key(KeyCode::KeyR);
    assert_eq!(h.active_tool(), Some("RoadTool"));
    // Slightly off-axis and off-grid drag: snaps and locks to a horizontal road.
    h.drag(
        grid_point(-2, 0) + Vec2::new(5.0, -3.0),
        grid_point(2, 0) + Vec2::new(-4.0, 20.0),
    );
    assert_eq!(h.segments(), 1);
    let (a, b) = h.sim(|sim| {
        (
            sim.roads.find_node_at(grid_point(-2, 0), 0),
            sim.roads.find_node_at(grid_point(2, 0), 0),
        )
    });
    assert!(a.is_some() && b.is_some());
}

#[test]
fn road_tool_crossing_drag_creates_a_junction() {
    let mut h = Harness::new();
    h.road(grid_point(-2, 0), grid_point(2, 0));
    h.key(KeyCode::KeyR);
    h.drag(grid_point(0, -2), grid_point(0, 2));
    assert_eq!(h.segments(), 4);
    let degree = h.sim(|sim| {
        let center = sim.roads.find_node_at(grid_point(0, 0), 0).unwrap();
        sim.roads.nodes[&center].degree()
    });
    assert_eq!(degree, 4);
}

#[test]
fn bulldoze_tool_removes_clicked_road() {
    let mut h = Harness::new();
    h.road(grid_point(-2, 0), grid_point(2, 0));
    h.key(KeyCode::KeyB);
    assert_eq!(h.active_tool(), Some("BulldozeTool"));
    h.drag(grid_point(0, 0), grid_point(0, 0));
    assert_eq!(h.segments(), 0);
    assert_eq!(h.sim(|sim| sim.roads.nodes.len()), 0);
}

#[test]
fn bulldozing_a_sweep_removes_everything_under_the_cursor() {
    let mut h = Harness::new();
    h.road(grid_point(-3, 0), grid_point(3, 0));
    h.road(grid_point(-3, 2), grid_point(3, 2));
    h.key(KeyCode::KeyB);
    h.move_to(grid_point(-1, 0));
    h.left(ButtonState::Pressed);
    h.move_to(grid_point(-1, 2));
    h.move_to(grid_point(-1, 1));
    h.left(ButtonState::Released);
    assert_eq!(h.segments(), 0, "both roads swept away");
}

#[test]
fn road_tool_builds_a_curve() {
    let mut h = Harness::new();
    h.key(KeyCode::KeyR);
    h.key(KeyCode::KeyV);
    // Drag from the start to the bend point, then click the end: a quarter turn.
    h.move_to(grid_point(-2, -2));
    h.left(ButtonState::Pressed);
    h.move_to(grid_point(1, -2));
    h.left(ButtonState::Released);
    h.click(grid_point(1, 1), MouseButton::Left);
    assert_eq!(h.segments(), 1);
    let (points, a, b, bend) = h.sim(|sim| {
        let segment = sim.roads.segments.values().next().unwrap();
        (
            segment.centerline().point_count(),
            sim.roads.find_node_at(grid_point(-2, -2), 0),
            sim.roads.find_node_at(grid_point(1, 1), 0),
            sim.roads.find_node_at(grid_point(1, -2), 0),
        )
    });
    assert!(points > 2, "curved");
    assert!(a.is_some() && b.is_some());
    assert!(bend.is_none(), "the bend point is no node");
    h.key(KeyCode::KeyV);
    h.move_to(grid_point(-3, 3));
    // Straight again: a plain drag builds a straight two-point segment.
    h.drag(grid_point(-3, 3), grid_point(-1, 3));
    assert_eq!(h.segments(), 2);
}

#[test]
fn road_shortcut_again_cycles_road_types_and_escape_puts_the_tool_away() {
    let mut h = Harness::new();
    let types = RoadType::list_all();
    h.key(KeyCode::KeyR);
    assert_eq!(h.tool_variant().as_deref(), Some(types[0].id.as_str()));
    h.key(KeyCode::KeyR);
    assert_eq!(h.active_tool(), Some("RoadTool"), "still building roads");
    assert_eq!(h.tool_variant().as_deref(), Some(types[1].id.as_str()));
    assert!(h.hint().contains(&types[1].display_name));
    let changes = h.messages::<ToolVariantChanged>();
    assert!(changes.is_empty() || changes[0].tool_id == "RoadTool");
    h.key(KeyCode::Escape);
    assert_eq!(h.active_tool(), None);
    assert_eq!(h.hint(), "");
}

#[test]
fn a_variant_request_builds_roads_of_that_type() {
    let mut h = Harness::new();
    let picked = RoadType::list_all().last().unwrap();
    h.app.world_mut().write_message(ToolVariantRequested {
        tool_id: "RoadTool".into(),
        variant: Some(picked.id.clone()),
    });
    h.frames(1);
    assert_eq!(h.active_tool(), Some("RoadTool"));
    // Asking again doesn't toggle the tool off.
    h.app.world_mut().write_message(ToolVariantRequested {
        tool_id: "RoadTool".into(),
        variant: None,
    });
    h.frames(1);
    assert_eq!(h.active_tool(), Some("RoadTool"));
    h.drag(grid_point(-2, 0), grid_point(2, 0));
    let built = h.sim(|sim| {
        sim.roads
            .segments
            .values()
            .next()
            .map(|s| s.road_type.id.clone())
    });
    assert_eq!(built.as_deref(), Some(picked.id.as_str()));
}

#[test]
fn escape_aborts_a_drag_before_it_puts_the_tool_away() {
    let mut h = Harness::new();
    h.key(KeyCode::KeyR);
    h.move_to(grid_point(-2, 0));
    h.left(ButtonState::Pressed);
    h.move_to(grid_point(2, 0));
    h.key(KeyCode::Escape);
    assert_eq!(
        h.active_tool(),
        Some("RoadTool"),
        "the first Esc only cancels"
    );
    h.left(ButtonState::Released);
    assert_eq!(h.segments(), 0, "nothing was built");
    h.key(KeyCode::Escape);
    assert_eq!(h.active_tool(), None);
}

#[test]
fn right_click_cancels_a_drag() {
    let mut h = Harness::new();
    h.key(KeyCode::KeyR);
    h.move_to(grid_point(-2, 0));
    h.left(ButtonState::Pressed);
    h.move_to(grid_point(2, 0));
    h.button(MouseButton::Right, ButtonState::Pressed);
    h.button(MouseButton::Right, ButtonState::Released);
    h.left(ButtonState::Released);
    assert_eq!(h.segments(), 0);
}

fn bands(h: &Harness) -> Vec<Color> {
    h.app
        .world()
        .resource::<ToolPreview>()
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Band { color, .. } => Some(*color),
            _ => None,
        })
        .collect()
}

#[test]
fn the_preview_follows_the_drag_and_the_hint_counts_cells() {
    let mut h = Harness::new();
    h.key(KeyCode::KeyR);
    h.move_to(grid_point(-2, 0));
    h.left(ButtonState::Pressed);
    h.move_to(grid_point(2, 0));
    assert!(h.hint().contains("4 cells"), "{}", h.hint());
    assert_eq!(bands(&h), vec![tf_game::tools::road::VALID_COLOR]);
    // Back on the start there is no road to show, and releasing builds nothing.
    h.move_to(grid_point(-2, 0));
    assert!(bands(&h).is_empty());
    h.left(ButtonState::Released);
    assert_eq!(h.segments(), 0);
}

#[test]
fn an_invalid_road_is_previewed_red_with_the_reason_and_not_built() {
    let mut h = Harness::new();
    h.road(grid_point(-2, 0), grid_point(2, 0));
    h.key(KeyCode::KeyR);
    h.move_to(grid_point(-2, 0));
    h.left(ButtonState::Pressed);
    h.move_to(grid_point(2, 0));
    assert_eq!(bands(&h), vec![tf_game::tools::road::INVALID_COLOR]);
    assert_eq!(h.hint(), "Road already exists");
    h.left(ButtonState::Released);
    assert_eq!(h.segments(), 1, "the same road is not built twice");
}

#[test]
fn a_curve_too_tight_for_drivers_is_refused() {
    let mut h = Harness::new();
    h.key(KeyCode::KeyR);
    h.key(KeyCode::KeyV);
    // The bend point is the start's neighbour, the end hugs it: a hairpin.
    h.move_to(grid_point(0, 0));
    h.left(ButtonState::Pressed);
    h.move_to(grid_point(3, 0));
    h.left(ButtonState::Released);
    h.move_to(grid_point(3, 1));
    let tight = bands(&h) == vec![tf_game::tools::road::INVALID_COLOR];
    assert!(tight, "{}", h.hint());
    h.left(ButtonState::Pressed);
    h.left(ButtonState::Released);
    assert_eq!(h.segments(), 0);
}

#[test]
fn the_grid_shows_only_with_tools_that_snap_to_it() {
    let mut h = Harness::new();
    assert!(!h.grid_shown(), "hidden with no tool");
    h.key(KeyCode::KeyR);
    assert!(h.grid_shown(), "roads snap to it");
    h.key(KeyCode::KeyB);
    assert!(!h.grid_shown(), "bulldozing doesn't");
    h.key(KeyCode::KeyB);
    assert_eq!(h.active_tool(), None, "the shortcut again toggles it off");
}

#[test]
fn tool_messages_announce_the_active_tool() {
    let mut h = Harness::new();
    h.app
        .world_mut()
        .write_message(ToolRequested(Some("BulldozeTool".into())));
    h.frames(1);
    assert_eq!(h.active_tool(), Some("BulldozeTool"));
    assert_eq!(h.hint(), tf_game::tools::bulldoze::HINT);
    h.app.world_mut().write_message(ToolRequested(None));
    h.frames(1);
    assert_eq!(h.active_tool(), None);
}
