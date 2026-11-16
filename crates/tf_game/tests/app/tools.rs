//! The build tools (M10), driven through the `Pointer` resource, mouse button messages and
//! keys, as `tests/integration/test_build_tools.gd` drove the main scene. The palette, top
//! bar, hint-position and debug-menu tests of that file go away with the UI.

use crate::common::{Harness, grid_point};
use bevy::input::ButtonState;
use bevy::prelude::*;
use tf_game::events::*;
use tf_game::runner::SimHandle;
use tf_game::tools::{Shape, ToolPreview};
use tf_sim::grid;
use tf_sim::roads::RoadType;

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
    h.key(KeyCode::KeyH);
    assert!(h.grid_shown(), "zoning does");
    h.key(KeyCode::KeyB);
    h.key(KeyCode::KeyB);
    assert_eq!(h.active_tool(), None, "the shortcut again toggles it off");
    assert!(!h.grid_shown());
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

// --- M10.2: zone, signal, bus line and route tools ---------------------------------------

fn cell_center(cell: IVec2) -> Vec2 {
    grid::cell_center(cell)
}

#[test]
fn zone_tools_paint_areas_that_develop_next_to_roads() {
    let mut h = Harness::new();
    h.road(grid_point(-4, 0), grid_point(4, 0));
    let row = grid::world_to_cell(grid_point(0, -1) + Vec2::splat(32.0)).y;
    let first = IVec2::new(grid::world_to_cell(grid_point(-3, 0)).x, row);
    let last = first + IVec2::new(2, 0);
    h.key(KeyCode::KeyH);
    assert_eq!(h.active_tool(), Some("ResidentialZoneTool"));
    assert!(h.grid_shown(), "zoning snaps to the grid");
    h.move_to(cell_center(first));
    h.left(ButtonState::Pressed);
    h.move_to(cell_center(last));
    assert_eq!(h.hint(), "3 cells");
    h.left(ButtonState::Released);
    for x in first.x..=last.x {
        let zone = h.sim(|sim| sim.zones.zones.get(&IVec2::new(x, row)).copied());
        assert_eq!(zone, Some(tf_sim::city::LotType::Residential));
    }
    // A cell two rows away from any road is zoned but can't develop.
    let far = first + IVec2::new(0, -3);
    h.key(KeyCode::KeyC);
    assert_eq!(h.active_tool(), Some("CommercialZoneTool"));
    h.click(cell_center(far), MouseButton::Left);

    h.sim(|sim| {
        sim.call(|sim, out| {
            sim.zones.step(1000.0, &sim.roads, &mut sim.lots, out);
        });
    });
    for x in first.x..=last.x {
        let lot = h.sim(|sim| sim.lots.lot_at_cell(IVec2::new(x, row)));
        assert!(lot.is_some(), "developed next to the road");
    }
    assert!(
        h.sim(|sim| sim.lots.lot_at_cell(far).is_none()),
        "no road, no building"
    );
    assert!(h.sim(|sim| sim.zones.is_waiting_for_road(far)));

    h.key(KeyCode::KeyX);
    assert_eq!(h.active_tool(), Some("DezoneTool"));
    h.click(cell_center(last), MouseButton::Left);
    assert!(h.sim(|sim| !sim.zones.zones.contains_key(&last)));
    assert!(
        h.sim(|sim| sim.lots.lot_at_cell(last).is_none()),
        "dezoning removes the building"
    );

    h.key(KeyCode::KeyB);
    h.move_to(cell_center(first));
    h.left(ButtonState::Pressed);
    h.left(ButtonState::Released);
    assert!(h.sim(|sim| sim.lots.lot_at_cell(first).is_none()));
    assert!(
        h.sim(|sim| !sim.zones.zones.contains_key(&first)),
        "bulldozing a building also dezones it"
    );
}

#[test]
fn zone_drag_can_be_cancelled_and_each_type_has_its_hint() {
    let mut h = Harness::new();
    h.key(KeyCode::KeyI);
    assert!(h.hint().contains("Industrial"), "{}", h.hint());
    let cell = grid::world_to_cell(grid_point(0, 0));
    h.move_to(cell_center(cell));
    h.left(ButtonState::Pressed);
    h.move_to(cell_center(cell + IVec2::new(2, 2)));
    assert_eq!(h.hint(), "9 cells");
    h.key(KeyCode::Escape);
    assert_eq!(
        h.active_tool(),
        Some("IndustrialZoneTool"),
        "Esc cancelled the drag only"
    );
    h.left(ButtonState::Released);
    assert!(h.sim(|sim| sim.zones.zones.is_empty()), "nothing was zoned");
    assert!(h.hint().contains("Industrial"), "{}", h.hint());
}

#[test]
fn signal_tool_cycles_junction_control() {
    use tf_sim::roads::road_node::JunctionControl::*;
    let mut h = Harness::new();
    h.road(grid_point(-2, 0), grid_point(2, 0));
    h.road(grid_point(0, -2), grid_point(0, 2));
    let node = h.sim(|sim| sim.roads.find_node_at(grid_point(0, 0), 0).unwrap());
    h.key(KeyCode::KeyL);
    assert_eq!(h.active_tool(), Some("SignalTool"));
    let at = grid_point(0, 0) + Vec2::new(5.0, 5.0);
    h.move_to(at);
    assert!(h.hint().contains("click to change"), "{}", h.hint());
    for expected in [TrafficLight, TrafficLightProtectedLeft, None] {
        h.click(at, MouseButton::Left);
        assert_eq!(h.sim(|sim| sim.roads.nodes[&node].control), expected);
    }
    // Away from a junction: nothing to hover, nothing changes.
    h.click(grid_point(0, 2), MouseButton::Left);
    assert_eq!(h.sim(|sim| sim.roads.nodes[&node].control), None);
    assert!(h.hint().starts_with("Click a junction"), "{}", h.hint());
}

#[test]
fn bus_line_tool_places_stops_and_opens_a_line() {
    let mut h = Harness::new();
    h.road(grid_point(-6, 0), grid_point(6, 0));
    h.key(KeyCode::KeyU);
    assert_eq!(h.active_tool(), Some("BusLineTool"));
    let south = Vec2::new(0.0, 10.0);
    let north = Vec2::new(0.0, -10.0);
    for point in [
        grid_point(-4, 0) + south,
        grid_point(4, 0) + south,
        grid_point(4, 0) + north,
        grid_point(-4, 0) + north,
    ] {
        h.click(point, MouseButton::Left);
    }
    assert_eq!(
        h.sim(|sim| sim.transit.stops.len()),
        4,
        "a stop on each side at each end"
    );
    assert_eq!(h.sim(|sim| sim.transit.lines.len()), 0, "not open yet");
    assert!(h.hint().contains("line: 4 stops"), "{}", h.hint());
    h.click(grid_point(-4, 0) + south, MouseButton::Left); // The first stop again.
    assert_eq!(h.sim(|sim| sim.transit.lines.len()), 1);
    let stops = h.sim(|sim| sim.transit.lines.values().next().unwrap().stop_ids.len());
    assert_eq!(stops, 4);
    // Right-clicking a stop removes it; the line goes on through the others.
    h.click(grid_point(4, 0) + north, MouseButton::Right);
    assert_eq!(h.sim(|sim| sim.transit.stops.len()), 3);
    let stops = h.sim(|sim| sim.transit.lines.values().next().unwrap().stop_ids.len());
    assert_eq!(stops, 3);
}

#[test]
fn bus_line_tool_variants_notifications_and_cancel() {
    let mut h = Harness::new();
    h.road(grid_point(-6, 0), grid_point(6, 0));
    h.key(KeyCode::KeyU);
    assert_eq!(h.tool_variant().as_deref(), Some("curbside"));
    h.key(KeyCode::KeyU);
    assert_eq!(
        h.active_tool(),
        Some("BusLineTool"),
        "U again cycles, it doesn't toggle off"
    );
    assert_eq!(h.tool_variant().as_deref(), Some("bay"));
    assert!(h.hint().starts_with("Bus bays"), "{}", h.hint());
    // A click far from any road: a notification, no stop.
    h.move_to(grid_point(0, 6));
    h.app
        .world_mut()
        .resource_mut::<Messages<NotificationPosted>>()
        .clear();
    h.button(MouseButton::Left, ButtonState::Pressed);
    let notes = h.messages::<NotificationPosted>();
    assert_eq!(
        notes,
        vec![NotificationPosted(
            "Bus stops go on a road, with room for a bus".into()
        )]
    );
    h.button(MouseButton::Left, ButtonState::Released);
    // A stop is a bay in bay mode; a line needs two stops.
    h.click(grid_point(-3, 0) + Vec2::new(0.0, 10.0), MouseButton::Left);
    let bay = h.sim(|sim| sim.transit.stops.values().next().map(|s| s.bay));
    assert_eq!(bay, Some(true));
    h.key(KeyCode::Escape);
    assert_eq!(
        h.active_tool(),
        Some("BusLineTool"),
        "the first Esc drops the line"
    );
    h.click(grid_point(-3, 0) + Vec2::new(0.0, 10.0), MouseButton::Left);
    h.click(grid_point(-3, 0) + Vec2::new(0.0, 10.0), MouseButton::Right);
    assert_eq!(h.sim(|sim| sim.transit.lines.len()), 0);
}

#[test]
fn route_tool_shows_route_between_two_clicked_roads() {
    let mut h = Harness::new();
    h.road(grid_point(-3, 0), grid_point(0, 0));
    h.road(grid_point(0, 0), grid_point(0, 3));
    h.key(KeyCode::KeyT);
    assert_eq!(h.active_tool(), Some("RouteTool"));
    assert_eq!(h.hint(), tf_game::tools::route::IDLE_HINT);
    // South side of the east-west road = eastbound lane; west side of the north-south
    // road = southbound.
    h.click(grid_point(-2, 0) + Vec2::new(0.0, 7.0), MouseButton::Left);
    assert!(h.hint().contains("destination"), "{}", h.hint());
    h.click(grid_point(0, 2) + Vec2::new(-7.0, 0.0), MouseButton::Left);
    assert!(h.hint().starts_with("Route: 3 lanes"), "{}", h.hint());
    let route_bands = |h: &Harness| {
        h.app
            .world()
            .resource::<ToolPreview>()
            .shapes
            .iter()
            .filter(|s| matches!(s, Shape::Band { .. }))
            .count()
    };
    assert_eq!(route_bands(&h), 1);
    // Bulldozing a road on the route clears it rather than leaving a stale drawing.
    h.sim(|sim| {
        let id = sim.roads.find_segment_at(grid_point(0, 2), 4.0, 0).unwrap();
        sim.call(|sim, out| {
            sim.roads.remove_segment(id, out);
        });
    });
    h.frames(3);
    assert_eq!(route_bands(&h), 0);
    assert_eq!(h.hint(), tf_game::tools::route::IDLE_HINT);
}

#[test]
fn route_tool_reports_no_route_and_starts_over() {
    let mut h = Harness::new();
    // Two roads that never meet.
    h.road(grid_point(-4, 0), grid_point(-1, 0));
    h.road(grid_point(1, 3), grid_point(4, 3));
    h.key(KeyCode::KeyT);
    h.click(grid_point(-3, 0) + Vec2::new(0.0, 7.0), MouseButton::Left);
    h.click(grid_point(3, 3) + Vec2::new(0.0, 7.0), MouseButton::Left);
    assert_eq!(h.hint(), "No route between these points");
    // Another click starts over; Esc drops the origin, the next Esc the tool.
    h.click(grid_point(-3, 0) + Vec2::new(0.0, 7.0), MouseButton::Left);
    assert!(h.hint().contains("destination"));
    h.key(KeyCode::Escape);
    assert_eq!(h.active_tool(), Some("RouteTool"));
    assert_eq!(h.hint(), tf_game::tools::route::IDLE_HINT);
    h.key(KeyCode::Escape);
    assert_eq!(h.active_tool(), None);
}

#[test]
fn a_new_simulation_puts_the_tools_back_to_idle() {
    let mut h = Harness::new();
    h.road(grid_point(-6, 0), grid_point(6, 0));
    h.key(KeyCode::KeyU);
    h.click(grid_point(-3, 0) + Vec2::new(0.0, 10.0), MouseButton::Left);
    assert!(h.hint().contains("line: 1 stops"), "{}", h.hint());
    h.sim(|_| {});
    let mut handle = h.app.world_mut().resource_mut::<SimHandle>();
    handle.replace(tf_sim::Simulation::new());
    h.frames(2);
    assert!(h.hint().starts_with("Curbside stops"), "{}", h.hint());
    assert_eq!(h.active_tool(), Some("BusLineTool"));
}

#[test]
fn the_bottleneck_scenario_loads_with_its_citizens_and_light() {
    let mut h = Harness::new();
    h.key(KeyCode::F7);
    let (citizens, lights) = h.sim(|sim| {
        (
            sim.citizens.citizens.len(),
            sim.roads
                .nodes
                .values()
                .filter(|n| n.control == tf_sim::roads::road_node::JunctionControl::TrafficLight)
                .count(),
        )
    });
    assert_eq!(citizens, 56);
    assert_eq!(lights, 1);
}
