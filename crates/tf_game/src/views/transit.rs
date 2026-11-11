//! Transit (`view/transit_renderer.gd`): each bus line as a coloured ribbon along the curb
//! side of the lanes its buses drive (lines sharing a road lie side by side), each stop as a
//! square sign at the curb (beyond the pull-in for a bus bay, which is a dark rectangle cut
//! into the sidewalk), and the stops' crosswalks as zebra stripes. Waiting riders are
//! drawn with the pedestrians (`moving`).
//!
//! The data comes from the latest snapshot, which carries every line and stop on each tick;
//! the view compares it with what it drew and rebuilds only what differs, one entity per
//! line, per stop and per crossing (the GDScript redraws everything every 6 frames). A
//! line that moved to another index in the list is rebuilt too (its offset depends on it).

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use glam::IVec2;
use tf_sim::ids::StopId;
use tf_sim::traffic::vehicle::BUS_LENGTH;
use tf_sim::traffic::vehicle_system::BAY_OFFSET;

use super::{ViewMaterial, spawn_shape, z};
use crate::camera::sim_to_bevy;
use crate::draw::ShapeBuilder;
use crate::events::SimulationReset;
use crate::runner::SimHandle;
use crate::snapshot::{BusLineView, ViewSnapshot};

/// Width of a line's ribbon (px).
pub const LINE_WIDTH: f32 = 3.0;
pub const LINE_ALPHA: f32 = 0.85;
/// Ribbons start this far right of the lane the buses drive (toward the curb).
pub const LINE_OFFSET: f32 = 4.0;
pub const SIGN_COLOR: Color = Color::srgb(0.949, 0.937, 0.902);
pub const SIGN_BORDER: Color = Color::srgb(0.05, 0.05, 0.05);
/// A square, so it reads at any zoom.
pub const SIGN_SIZE: f32 = 8.0;
pub const SIGN_BORDER_WIDTH: f32 = 1.2;
/// From the stop (on its lane) out to the sign.
pub const SIGN_OFFSET: f32 = 11.0;
/// How much further out the sign stands at a bay.
pub const BAY_SIGN_SHIFT: f32 = 8.0;
/// A bus bay: a pull-in the length of two buses beside the lane.
pub const BAY_SIZE: Vec2 = Vec2::new(2.0 * BUS_LENGTH as f32 + 6.0, 10.0);
pub const BAY_COLOR: Color = Color::srgb(0.27, 0.29, 0.32);
pub const BAY_EDGE_COLOR: Color = Color::srgb(0.62, 0.62, 0.6);
pub const ZEBRA_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.6);
pub const ZEBRA_LENGTH: f32 = 6.0;
pub const ZEBRA_WIDTH: f32 = 2.5;
pub const ZEBRA_SPACING: f32 = 5.0;

/// What one stop is drawn from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopData {
    pub position: Vec2,
    pub direction: Vec2,
    pub bay: bool,
}

/// What one crosswalk is drawn from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrossingData {
    pub center: Vec2,
    pub across: Vec2,
    pub half_width: f32,
}

/// Where the sign of a stop stands (sim coordinates): at the curb, beyond the pull-in for
/// a bay (`TransitRenderer.sign_spot`).
pub fn sign_spot(stop: &StopData) -> Vec2 {
    // Toward the curb: right of the traffic's direction (y grows downward).
    let out = Vec2::new(-stop.direction.y, stop.direction.x);
    let beyond = if stop.bay { BAY_SIGN_SHIFT } else { 0.0 };
    stop.position + out * (SIGN_OFFSET + beyond)
}

/// A line's ribbon: each piece of its path moved `LINE_OFFSET` plus its place in the
/// list to its right, with round joints (a mitred offset would spike where a path doubles
/// back, round a loop's end or into a bay).
pub fn line_shape(index: usize, line: &BusLineView) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new();
    let offset = LINE_OFFSET + (index as f32 + 0.5) * LINE_WIDTH;
    let color = Color::srgba(line.color[0], line.color[1], line.color[2], LINE_ALPHA);
    for pair in line.path.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a.distance_squared(b) < 0.01 {
            continue;
        }
        let dir = (b - a).normalize();
        let right = Vec2::new(-dir.y, dir.x) * offset;
        shape.line(
            sim_to_bevy(a + right),
            sim_to_bevy(b + right),
            LINE_WIDTH,
            color,
        );
        shape.circle(sim_to_bevy(b + right), LINE_WIDTH * 0.5, 8, color);
    }
    shape
}

/// The sign (with its bay, if it has one).
pub fn stop_shape(stop: &StopData) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new();
    let angle = -stop.direction.to_angle();
    if stop.bay {
        let out = Vec2::new(-stop.direction.y, stop.direction.x);
        let center = sim_to_bevy(stop.position + out * BAY_OFFSET as f32);
        shape.quad(center, BAY_SIZE + Vec2::splat(2.0), angle, BAY_EDGE_COLOR);
        shape.quad(center, BAY_SIZE, angle, BAY_COLOR);
    }
    let center = sim_to_bevy(sign_spot(stop));
    shape.quad(
        center,
        Vec2::splat(SIGN_SIZE + SIGN_BORDER_WIDTH * 2.0),
        angle,
        SIGN_BORDER,
    );
    shape.quad(center, Vec2::splat(SIGN_SIZE), angle, SIGN_COLOR);
    shape
}

/// Zebra stripes across the road at a stop's crosswalk.
pub fn crossing_shape(crossing: &CrossingData) -> ShapeBuilder {
    let mut shape = ShapeBuilder::new();
    let along = Vec2::new(crossing.across.y, -crossing.across.x);
    let mut offset = -crossing.half_width + ZEBRA_SPACING * 0.5;
    while offset < crossing.half_width {
        let stripe = crossing.center + crossing.across * offset;
        shape.line(
            sim_to_bevy(stripe - along * ZEBRA_LENGTH * 0.5),
            sim_to_bevy(stripe + along * ZEBRA_LENGTH * 0.5),
            ZEBRA_WIDTH,
            ZEBRA_COLOR,
        );
        offset += ZEBRA_SPACING;
    }
    shape
}

fn stops_of(view: &ViewSnapshot) -> Vec<(StopId, StopData)> {
    (0..view.stop_positions.len())
        .map(|i| {
            (
                view.stop_ids[i],
                StopData {
                    position: view.stop_positions[i],
                    direction: view.stop_directions[i],
                    bay: view.stop_bays[i],
                },
            )
        })
        .collect()
}

/// What is on screen, so a change touches only its own entities.
#[derive(Resource, Default, Debug)]
pub struct TransitViews {
    lines: Vec<(BusLineView, Entity)>,
    stops: HashMap<StopId, (StopData, Entity)>,
    crossings: HashMap<IVec2, (CrossingData, Entity)>,
    /// How many of each were built so far (tests: an edit builds only what it changed).
    pub lines_built: usize,
    pub stops_built: usize,
    pub crossings_built: usize,
    /// The snapshot last compared (address and tick time).
    seen: Option<(usize, u64)>,
}

impl TransitViews {
    pub fn line_entities(&self) -> Vec<Entity> {
        self.lines.iter().map(|(_, e)| *e).collect()
    }

    pub fn stop_entity(&self, id: StopId) -> Option<Entity> {
        self.stops.get(&id).map(|(_, e)| *e)
    }

    pub fn stop_count(&self) -> usize {
        self.stops.len()
    }

    pub fn crossing_count(&self) -> usize {
        self.crossings.len()
    }
}

fn update_transit(
    mut resets: MessageReader<SimulationReset>,
    handle: Res<SimHandle>,
    mut views: ResMut<TransitViews>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Res<ViewMaterial>,
) {
    // A new simulation can start at the same time as the last one, in the same memory.
    if resets.read().count() > 0 {
        views.seen = None;
    }
    let view: &Arc<ViewSnapshot> = &handle.view_to;
    let key = (Arc::as_ptr(view) as usize, view.time.to_bits());
    if views.seen == Some(key) {
        return;
    }
    views.seen = Some(key);

    // Lines: the same line at the same place keeps its entity.
    let mut old = std::mem::take(&mut views.lines);
    for (k, line) in view.bus_lines.iter().enumerate() {
        match old.get(k) {
            Some((drawn, entity)) if drawn == line => {
                views.lines.push((drawn.clone(), *entity));
            }
            _ => {
                if let Some((_, entity)) = old.get(k) {
                    commands.entity(*entity).despawn();
                }
                let entity = spawn_shape(
                    &mut commands,
                    &mut meshes,
                    &material,
                    line_shape(k, line),
                    z::TRANSIT_LINES,
                );
                views.lines_built += 1;
                views.lines.push((line.clone(), entity));
            }
        }
    }
    for (_, entity) in old.drain(view.bus_lines.len().min(old.len())..) {
        commands.entity(entity).despawn();
    }

    // Stops.
    let stops = stops_of(view);
    let gone: Vec<StopId> = views
        .stops
        .keys()
        .filter(|id| !stops.iter().any(|(s, _)| s == *id))
        .copied()
        .collect();
    for id in gone {
        if let Some((_, entity)) = views.stops.remove(&id) {
            commands.entity(entity).despawn();
        }
    }
    for (id, data) in stops {
        if views
            .stops
            .get(&id)
            .is_some_and(|(drawn, _)| *drawn == data)
        {
            continue;
        }
        if let Some((_, entity)) = views.stops.remove(&id) {
            commands.entity(entity).despawn();
        }
        let entity = spawn_shape(
            &mut commands,
            &mut meshes,
            &material,
            stop_shape(&data),
            z::BUS_STOPS,
        );
        views.stops_built += 1;
        views.stops.insert(id, (data, entity));
    }

    // Crosswalks at stops.
    let crossings: HashMap<IVec2, CrossingData> = view
        .stop_crossings
        .iter()
        .map(|(&key, c)| {
            (
                key,
                CrossingData {
                    center: c.center,
                    across: c.across,
                    half_width: c.half_width as f32,
                },
            )
        })
        .collect();
    let gone: Vec<IVec2> = views
        .crossings
        .keys()
        .filter(|key| !crossings.contains_key(*key))
        .copied()
        .collect();
    for key in gone {
        if let Some((_, entity)) = views.crossings.remove(&key) {
            commands.entity(entity).despawn();
        }
    }
    for (key, data) in crossings {
        if views
            .crossings
            .get(&key)
            .is_some_and(|(drawn, _)| *drawn == data)
        {
            continue;
        }
        if let Some((_, entity)) = views.crossings.remove(&key) {
            commands.entity(entity).despawn();
        }
        let entity = spawn_shape(
            &mut commands,
            &mut meshes,
            &material,
            crossing_shape(&data),
            z::CROSSWALKS,
        );
        views.crossings_built += 1;
        views.crossings.insert(key, (data, entity));
    }
}

pub struct TransitViewPlugin;

impl Plugin for TransitViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TransitViews>()
            .add_systems(Update, update_transit);
    }
}
