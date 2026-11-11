//! Highway markers (`view/external_renderer.gd`): a sign on the map edge at each external
//! connection, blue when a road reaches it, grey when not. Rebuilt on `ExternalsChanged`,
//! `RoadNetworkChanged` (a road may now reach it) and `SimulationReset`, when the points or
//! their state differ from what is on screen.

use bevy::prelude::*;

use super::lots::bevy_rect;
use super::{ViewMaterial, spawn_shape, z};
use crate::draw::ShapeBuilder;
use crate::events::{ExternalsChanged, RoadNetworkChanged, SimulationReset};
use crate::runner::SimHandle;
use tf_sim::geom::Rect2;

pub const SIGN_SIZE: Vec2 = Vec2::new(46.0, 22.0);
pub const ACTIVE_COLOR: Color = Color::srgb(0.118, 0.533, 0.898);
pub const INACTIVE_COLOR: Color = Color::srgba(0.5, 0.5, 0.5, 0.7);

#[derive(Component)]
pub struct ExternalView;

/// Each marker as drawn: where the connection is, and whether a road reaches it.
#[derive(Resource, Default, Debug)]
pub struct ExternalViews {
    drawn: Vec<(Vec2, bool)>,
    entities: Vec<Entity>,
}

impl ExternalViews {
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }
}

pub struct ExternalViewPlugin;

impl Plugin for ExternalViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ExternalViews>()
            .add_systems(Update, update_externals);
    }
}

/// The sign for a connection point: above and just right of it, with a white border.
pub fn marker_shape(point: Vec2, active: bool) -> ShapeBuilder {
    let rect = Rect2::new(point + Vec2::new(4.0, -SIGN_SIZE.y - 18.0), SIGN_SIZE);
    let (min, max) = bevy_rect(rect);
    let mut out = ShapeBuilder::new();
    out.rect(min, max, if active { ACTIVE_COLOR } else { INACTIVE_COLOR })
        .rect_outline(min, max, 1.5, Color::WHITE);
    // A road-sign bar across it stands in for the "HWY" lettering.
    let mid = (min + max) * 0.5;
    out.rect(
        mid - Vec2::new(SIGN_SIZE.x * 0.3, 1.5),
        mid + Vec2::new(SIGN_SIZE.x * 0.3, 1.5),
        Color::WHITE,
    );
    out
}

#[allow(clippy::too_many_arguments)]
fn update_externals(
    mut resets: MessageReader<SimulationReset>,
    mut external_changes: MessageReader<ExternalsChanged>,
    mut road_changes: MessageReader<RoadNetworkChanged>,
    mut handle: ResMut<SimHandle>,
    mut views: ResMut<ExternalViews>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
    mut built: Local<bool>,
) {
    let reset = resets.read().count() > 0 || !*built;
    let changed = external_changes.read().count() + road_changes.read().count() > 0;
    *built = true;
    if !reset && !changed {
        return;
    }
    let sim = handle.get_mut();
    let active = sim.externals.active_points(&sim.roads);
    let wanted: Vec<(Vec2, bool)> = sim
        .externals
        .points
        .iter()
        .map(|&p| (p, active.contains(&p)))
        .collect();
    if wanted == views.drawn && !reset {
        return;
    }
    for entity in views.entities.drain(..) {
        commands.entity(entity).despawn();
    }
    for &(point, is_active) in &wanted {
        let entity = spawn_shape(
            &mut commands,
            &mut meshes,
            &material,
            marker_shape(point, is_active),
            z::EXTERNALS,
        );
        commands.entity(entity).insert(ExternalView);
        views.entities.push(entity);
    }
    views.drawn = wanted;
}
