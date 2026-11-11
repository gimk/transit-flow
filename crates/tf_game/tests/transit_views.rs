//! The transit, light and overlay views (M9.3): transit entities rebuild only what changed,
//! a light bar takes its colour from the snapshot, and the overlays toggle on request.

use bevy::prelude::*;
use glam::{IVec2, Vec2};
use tf_game::events::{DebugOverlayChanged, DebugOverlayToggleRequested};
use tf_game::plugin::GamePlugin;
use tf_game::runner::SimHandle;
use tf_game::snapshot::ViewSnapshot;
use tf_game::views::ViewsPlugin;
use tf_game::views::lights::{
    AMBER, GREEN, LightBar, LightViews, RED, bar_light, draw_bars, light_bars, light_color,
};
use tf_game::views::moving::Batch;
use tf_game::views::overlays::{Heatmap, LaneOverlay, heat_color, heatmap_shape, lane_lines};
use tf_game::views::transit::{TransitViews, crossing_shape, line_shape, stop_shape};
use tf_sim::Simulation;
use tf_sim::debug::scenarios;
use tf_sim::ids::NodeId;
use tf_sim::traffic::traffic_lights::Light;

const ORIGIN: IVec2 = IVec2::new(2, 2);

fn app() -> App {
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
    app
}

fn bottleneck(line: bool, extra_stop: bool) -> Simulation {
    let mut sim = Simulation::new();
    scenarios::build_bottleneck(&mut sim, ORIGIN);
    if line {
        scenarios::add_bottleneck_bus_line(&mut sim, ORIGIN);
    }
    if extra_stop {
        let point = (ORIGIN + IVec2::new(-14, 0)).as_vec2() * tf_sim::grid::CELL_SIZE as f32
            + Vec2::new(0.0, 10.0);
        let (transit, mut ctx) = sim.transit_ctx();
        transit.add_stop(&mut ctx, point, true).expect("a stop");
    }
    sim
}

fn show(app: &mut App, sim: Simulation) {
    app.world_mut().resource_mut::<SimHandle>().replace(sim);
    app.update();
    app.update();
}

fn exists(app: &App, entity: Entity) -> bool {
    app.world().get_entity(entity).is_ok()
}

#[test]
fn transit_views_rebuild_only_what_changed() {
    let mut app = app();
    show(&mut app, bottleneck(true, false));
    let (stops, lines, crossings, stop_ids): (usize, usize, usize, Vec<_>) = {
        let handle = app.world().resource::<SimHandle>();
        let views = app.world().resource::<TransitViews>();
        let ids = handle.view_to.stop_ids.clone();
        assert_eq!(views.stop_count(), ids.len());
        assert_eq!(views.stop_count(), 12, "the line's twelve stops");
        assert_eq!(views.line_entities().len(), 1);
        (
            views.stops_built,
            views.lines_built,
            views.crossings_built,
            ids,
        )
    };
    assert_eq!((stops, lines), (12, 1));
    assert_eq!(
        crossings,
        app.world().resource::<TransitViews>().crossing_count()
    );
    let before: Vec<Entity> = stop_ids
        .iter()
        .map(|&id| {
            app.world()
                .resource::<TransitViews>()
                .stop_entity(id)
                .unwrap()
        })
        .collect();
    let line_before = app.world().resource::<TransitViews>().line_entities();

    // Another stop (a bay): only it is built.
    show(&mut app, bottleneck(true, true));
    let views = app.world().resource::<TransitViews>();
    assert_eq!(views.stop_count(), 13);
    assert_eq!(views.stops_built, stops + 1);
    assert_eq!(views.lines_built, lines, "the line was not touched");
    assert_eq!(views.line_entities(), line_before);
    for (&id, &entity) in stop_ids.iter().zip(&before) {
        assert_eq!(views.stop_entity(id), Some(entity));
    }

    // No line, no stops: everything goes.
    show(&mut app, bottleneck(false, false));
    let views = app.world().resource::<TransitViews>();
    assert_eq!(views.stop_count(), 0);
    assert!(views.line_entities().is_empty());
    assert!(!exists(&app, line_before[0]));
    assert!(!exists(&app, before[0]));
}

#[test]
fn transit_shapes_have_content() {
    let sim = bottleneck(true, true);
    let view = ViewSnapshot::capture(&mut { sim });
    assert_eq!(view.bus_lines.len(), 1);
    assert!(!line_shape(0, &view.bus_lines[0]).is_empty());
    let stop = tf_game::views::transit::StopData {
        position: view.stop_positions[0],
        direction: view.stop_directions[0],
        bay: false,
    };
    let curb = stop_shape(&stop).vertex_count();
    let bay = stop_shape(&tf_game::views::transit::StopData { bay: true, ..stop }).vertex_count();
    assert!(bay > curb, "a bay adds its pull-in");
    for crossing in view.stop_crossings.values() {
        let data = tf_game::views::transit::CrossingData {
            center: crossing.center,
            across: crossing.across,
            half_width: crossing.half_width as f32,
        };
        assert!(!crossing_shape(&data).is_empty());
    }
}

fn first_color(batch: &mut Batch, n: usize) -> Vec<[f32; 4]> {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        Default::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new())
    .with_inserted_indices(bevy::mesh::Indices::U32(Vec::new()));
    batch.swap_into(&mut mesh);
    match mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap() {
        bevy::mesh::VertexAttributeValues::Float32x4(c) => c.iter().take(n).copied().collect(),
        _ => panic!("colours are float4"),
    }
}

fn linear(c: [f32; 4]) -> [f32; 4] {
    LinearRgba::from(Color::srgba(c[0], c[1], c[2], c[3])).to_f32_array()
}

#[test]
fn a_light_bar_takes_the_best_light_of_its_turns_from_the_snapshot() {
    let bar = LightBar {
        node: NodeId(5),
        center: Vec2::ZERO,
        length: 10.0,
        angle: 0.0,
        phases: vec![0, 1],
    };
    let mut view = ViewSnapshot::default();
    view.lights
        .insert(NodeId(5), vec![Light::Red, Light::Amber]);
    assert_eq!(
        bar_light(&view, &bar),
        Light::Amber,
        "the best of red and amber"
    );
    let mut batch = Batch::default();
    draw_bars(&mut batch, std::slice::from_ref(&bar), &view);
    assert_eq!(first_color(&mut batch, 1)[0], linear(AMBER));

    view.lights
        .insert(NodeId(5), vec![Light::Green, Light::Red]);
    draw_bars(&mut batch, std::slice::from_ref(&bar), &view);
    assert_eq!(first_color(&mut batch, 1)[0], linear(GREEN));

    view.lights.insert(NodeId(5), vec![Light::Red, Light::Red]);
    assert_eq!(light_color(bar_light(&view, &bar)), RED);
    // A junction the snapshot doesn't know shows green (lights just added).
    assert_eq!(bar_light(&ViewSnapshot::default(), &bar), Light::Green);
}

#[test]
fn the_light_mesh_has_a_bar_per_stop_line() {
    let mut app = app();
    show(&mut app, bottleneck(false, false));
    let sim_bars = {
        let mut handle = app.world_mut().resource_mut::<SimHandle>();
        light_bars(&handle.get_mut().roads).len()
    };
    assert!(sim_bars >= 3, "the T junction has lights");
    let views = app.world().resource::<LightViews>();
    assert_eq!(views.bar_count(), sim_bars);
    let mesh = app
        .world()
        .resource::<Assets<Mesh>>()
        .get(&views.mesh)
        .unwrap();
    assert_eq!(mesh.count_vertices(), 4 * sim_bars);
}

#[test]
fn overlays_toggle_on_request_and_answer() {
    let mut app = app();
    show(&mut app, bottleneck(false, false));
    let heatmap = app.world().resource::<Heatmap>().entity;
    assert!(!app.world().resource::<LaneOverlay>().shown);
    assert!(!app.world().resource::<Heatmap>().shown);
    assert_eq!(
        *app.world().get::<Visibility>(heatmap).unwrap(),
        Visibility::Hidden
    );

    for id in ["lanes", "heatmap"] {
        app.world_mut()
            .resource_mut::<Messages<DebugOverlayToggleRequested>>()
            .write(DebugOverlayToggleRequested(id.into()));
    }
    app.update();
    assert!(app.world().resource::<LaneOverlay>().shown);
    assert!(app.world().resource::<Heatmap>().shown);
    assert_ne!(
        *app.world().get::<Visibility>(heatmap).unwrap(),
        Visibility::Hidden
    );
    assert!(
        !app.world()
            .resource::<LaneOverlay>()
            .lines
            .strips
            .is_empty(),
        "the lane lines are listed once shown"
    );
    assert_eq!(app.world().resource::<Heatmap>().builds, 1);
    {
        let messages = app.world().resource::<Messages<DebugOverlayChanged>>();
        let mut cursor = messages.get_cursor();
        let seen: Vec<_> = cursor
            .read(messages)
            .map(|m| (m.overlay_id.clone(), m.shown))
            .collect();
        assert!(seen.contains(&("lanes".to_string(), true)));
        assert!(seen.contains(&("heatmap".to_string(), true)));
    }

    for id in ["lanes", "heatmap"] {
        app.world_mut()
            .resource_mut::<Messages<DebugOverlayToggleRequested>>()
            .write(DebugOverlayToggleRequested(id.into()));
    }
    app.update();
    assert!(!app.world().resource::<LaneOverlay>().shown);
    assert!(!app.world().resource::<Heatmap>().shown);
    assert_eq!(
        *app.world().get::<Visibility>(heatmap).unwrap(),
        Visibility::Hidden
    );
}

#[test]
fn the_heatmap_draws_congested_lanes_in_heat_colours() {
    let mut sim = bottleneck(false, false);
    let lane = *sim.roads.lanes.keys().min().unwrap();
    let mut congestion = indexmap::IndexMap::new();
    assert!(heatmap_shape(&sim.roads, &congestion).is_empty());
    congestion.insert(lane, 2.0);
    congestion.insert(tf_sim::ids::LaneId(-99), 1.0);
    assert!(!heatmap_shape(&sim.roads, &congestion).is_empty());
    let (mild, bad) = (heat_color(0.0).to_srgba(), heat_color(100.0).to_srgba());
    assert!(mild.green > bad.green, "yellow to red");
    let lines = lane_lines(&sim.roads);
    assert_eq!(lines.strips.len(), sim.roads.lanes.len());
    assert_eq!(lines.nodes.len(), sim.roads.nodes.len());
    assert!(!lines.arrows.is_empty());
    let _ = &mut sim;
}
