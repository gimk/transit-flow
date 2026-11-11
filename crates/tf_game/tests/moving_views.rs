//! The moving views (M9.2): vehicles interpolate between two snapshots, the batched mesh
//! has four vertices per vehicle, and F2 changes the colours.

use bevy::prelude::*;
use glam::Vec2;
use tf_game::camera::sim_to_bevy;
use tf_game::events::DebugOverlayToggleRequested;
use tf_game::plugin::GamePlugin;
use tf_game::runner::SimHandle;
use tf_game::snapshot::ViewSnapshot;
use tf_game::views::ViewsPlugin;
use tf_game::views::moving::{Batch, MovingViews, ShowLateness, draw_vehicles, lateness_color};
use tf_sim::Simulation;
use tf_sim::debug::scenarios;
use tf_sim::ids::VehicleId;
use tf_sim::traffic::vehicle::Kind;

/// A snapshot with one vehicle (id 1) at `x`, turned by `rotation`.
fn snapshot(time: f64, x: f32, rotation: f32) -> ViewSnapshot {
    let mut s = ViewSnapshot {
        time,
        ..Default::default()
    };
    s.vehicle_index.insert(VehicleId(1), 0);
    s.positions.push(Vec2::new(x, 10.0));
    s.rotations.push(rotation);
    s.sizes.push(Vec2::new(11.0, 6.0));
    s.colors.push([0.4, 0.7, 0.4, 1.0]);
    s.lateness.push(0.0);
    s
}

fn centre(batch_positions: &[[f32; 3]]) -> Vec2 {
    let sum: Vec2 = batch_positions.iter().map(|p| Vec2::new(p[0], p[1])).sum();
    sum / batch_positions.len() as f32
}

#[test]
fn a_vehicle_is_drawn_between_its_two_poses() {
    let (from, to) = (snapshot(0.0, 100.0, 0.0), snapshot(0.05, 110.0, 0.4));
    let (mut batch, mut poses) = (Batch::default(), Vec::new());
    draw_vehicles(&mut batch, &mut poses, &from, &to, 0.5, false);
    assert_eq!(poses.len(), 1);
    let (position, rotation) = poses[0];
    assert!((position.x - 105.0).abs() < 1e-4, "{position:?}");
    assert!((rotation - 0.2).abs() < 1e-4, "{rotation}");
    // The mesh is where the pose is, in the Bevy world.
    let mut mesh = bevy::prelude::Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        Default::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, Vec::<[f32; 4]>::new())
    .with_inserted_indices(bevy::mesh::Indices::U32(Vec::new()));
    batch.swap_into(&mut mesh);
    let positions = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|a| a.as_float3())
        .unwrap();
    assert_eq!(positions.len(), 4);
    assert!((centre(positions) - sim_to_bevy(Vec2::new(105.0, 10.0))).length() < 1e-3);
}

#[test]
fn new_vehicles_stand_where_they_are_and_gone_ones_vanish() {
    let mut empty = snapshot(0.0, 0.0, 0.0);
    empty.vehicle_index.clear();
    empty.positions.clear();
    empty.rotations.clear();
    empty.sizes.clear();
    empty.colors.clear();
    empty.lateness.clear();
    let one = snapshot(0.05, 50.0, 0.0);
    let (mut batch, mut poses) = (Batch::default(), Vec::new());
    draw_vehicles(&mut batch, &mut poses, &empty, &one, 0.3, false);
    assert_eq!(poses, vec![(Vec2::new(50.0, 10.0), 0.0)]);
    draw_vehicles(&mut batch, &mut poses, &one, &empty, 0.3, false);
    assert!(poses.is_empty());
    assert_eq!(batch.vertex_count(), 0);
}

#[test]
fn lateness_goes_from_green_through_yellow_to_red() {
    let (on_time, late, very) = (
        lateness_color(0.0),
        lateness_color(0.5),
        lateness_color(1.0),
    );
    assert!(on_time[1] > on_time[0] && very[0] > very[1]);
    assert!(late[0] > 0.9 && late[1] > 0.8 && late[2] < 0.3);
}

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

fn vertices(app: &App, handle: &Handle<Mesh>) -> (usize, Vec<[f32; 4]>) {
    let mesh = app.world().resource::<Assets<Mesh>>().get(handle).unwrap();
    let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap() {
        bevy::mesh::VertexAttributeValues::Float32x4(c) => c.clone(),
        _ => panic!("colours are float4"),
    };
    (mesh.count_vertices(), colors)
}

#[test]
fn the_mesh_has_four_vertices_per_vehicle_and_f2_recolours_them() {
    let mut app = app();
    let mut sim = Simulation::new();
    scenarios::build_bottleneck(&mut sim, glam::IVec2::new(2, 2));
    app.world_mut().resource_mut::<SimHandle>().replace(sim);
    app.world_mut()
        .resource_mut::<SimHandle>()
        .get_mut()
        .stress
        .target_count = 30;
    // Let the stress traffic start up.
    for _ in 0..400 {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(1));
        if app.world().resource::<SimHandle>().view_to.vehicle_count >= 10 {
            break;
        }
    }
    let handle = app.world().resource::<MovingViews>().vehicles.clone();
    let count = app.world().resource::<SimHandle>().view_to.positions.len();
    assert!(count > 0, "some vehicles are running");
    app.update();
    let (n, plain) = vertices(&app, &handle);
    assert_eq!(n, 4 * count);
    assert_eq!(
        app.world().resource::<SimHandle>().view_to.kinds.len(),
        count
    );
    assert!(
        app.world()
            .resource::<SimHandle>()
            .view_to
            .kinds
            .iter()
            .all(|&k| k == Kind::Car as u8 || k == Kind::Truck as u8 || k == Kind::Bus as u8)
    );

    app.world_mut()
        .resource_mut::<Messages<DebugOverlayToggleRequested>>()
        .write(DebugOverlayToggleRequested("lateness".into()));
    app.update();
    assert!(app.world().resource::<ShowLateness>().0);
    let (n2, late) = vertices(&app, &handle);
    assert_eq!(
        n2,
        4 * app.world().resource::<SimHandle>().view_to.positions.len()
    );
    assert_ne!(plain, late, "the colours changed");
}
