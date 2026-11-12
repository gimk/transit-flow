//! What the active tool shows (`BuildTool._draw` in Godot): the shapes of `ToolPreview`.
//! Filled shapes (road bands, highlights, zone rectangles) are one mesh, rebuilt when the
//! preview changes; thin lines and rings are gizmos of their own group (`ToolGizmos`,
//! constant screen width), drawn every frame.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::{ViewMaterial, z};
use crate::camera::sim_to_bevy;
use crate::draw::ShapeBuilder;
use crate::tools::{Shape, ToolPreview};

/// Gizmo line width (screen px).
pub const TOOL_LINE_PX: f32 = 2.0;
pub const DASH: f32 = 6.0;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct ToolGizmos;

/// The preview mesh, and what it was last built from.
#[derive(Resource, Debug)]
pub struct PreviewView {
    pub entity: Entity,
    mesh: Handle<Mesh>,
    /// How many times the mesh was rebuilt (tests).
    pub builds: usize,
}

impl FromWorld for PreviewView {
    fn from_world(world: &mut World) -> Self {
        let material = world.resource::<ViewMaterial>().0.clone();
        let mesh = world.resource_mut::<Assets<Mesh>>().add(
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3])
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3])
            .with_inserted_indices(Indices::U32(vec![0, 1, 2])),
        );
        let entity = world
            .spawn((
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material),
                Transform::from_xyz(0.0, 0.0, z::TOOL),
                Visibility::Hidden,
                NoFrustumCulling,
            ))
            .id();
        Self {
            entity,
            mesh,
            builds: 0,
        }
    }
}

/// The filled shapes of a preview as a mesh builder (Bevy coordinates).
pub fn filled_shapes(shapes: &[Shape]) -> ShapeBuilder {
    let mut builder = ShapeBuilder::new();
    for shape in shapes {
        match shape {
            Shape::Band {
                points,
                width,
                color,
            } => {
                let points: Vec<Vec2> = points.iter().copied().map(sim_to_bevy).collect();
                builder.polyline(&points, *width, *color);
            }
            Shape::Disc {
                center,
                radius,
                color,
            } => {
                builder.circle(sim_to_bevy(*center), *radius, 24, *color);
            }
            Shape::Rect { min, max, color } => {
                let (a, b) = (sim_to_bevy(*min), sim_to_bevy(*max));
                builder.rect(a.min(b), a.max(b), *color);
            }
            Shape::RectOutline {
                min,
                max,
                width,
                color,
            } => {
                let (a, b) = (sim_to_bevy(*min), sim_to_bevy(*max));
                builder.rect_outline(a.min(b), a.max(b), *width, *color);
            }
            Shape::Line { .. } | Shape::Dashed { .. } | Shape::Ring { .. } => {}
        }
    }
    builder
}

fn update_preview_mesh(
    preview: Res<ToolPreview>,
    mut view: ResMut<PreviewView>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut visibility: Query<&mut Visibility>,
) {
    if !preview.is_changed() {
        return;
    }
    let builder = filled_shapes(&preview.shapes);
    let empty = builder.is_empty();
    if let Some(mut slot) = meshes.get_mut(&view.mesh) {
        *slot = if empty {
            super::moving::empty_batch_mesh()
        } else {
            builder.build()
        };
    }
    if let Ok(mut visible) = visibility.get_mut(view.entity) {
        *visible = if empty {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    view.builds += 1;
}

fn draw_preview_lines(preview: Res<ToolPreview>, mut gizmos: Gizmos<ToolGizmos>) {
    for shape in &preview.shapes {
        match shape {
            Shape::Line { points, color } => {
                gizmos.linestrip_2d(points.iter().copied().map(sim_to_bevy), *color);
            }
            Shape::Dashed { from, to, color } => {
                let (a, b) = (sim_to_bevy(*from), sim_to_bevy(*to));
                let length = a.distance(b);
                if length <= 0.0 {
                    continue;
                }
                let dir = (b - a) / length;
                let mut at = 0.0;
                while at < length {
                    let end = (at + DASH).min(length);
                    gizmos.line_2d(a + dir * at, a + dir * end, *color);
                    at += DASH * 2.0;
                }
            }
            Shape::Ring {
                center,
                radius,
                color,
            } => {
                gizmos.circle_2d(sim_to_bevy(*center), *radius, *color);
            }
            _ => {}
        }
    }
}

pub struct ToolPreviewPlugin;

impl Plugin for ToolPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolPreview>()
            .init_resource::<PreviewView>()
            .add_systems(Update, update_preview_mesh);
        // Gizmos exist only in the full app (tests run without rendering).
        if app.is_plugin_added::<bevy::gizmos::GizmoPlugin>() {
            app.init_gizmo_group::<ToolGizmos>()
                .add_systems(Update, draw_preview_lines)
                .add_systems(Startup, |mut store: ResMut<GizmoConfigStore>| {
                    store.config_mut::<ToolGizmos>().0.line.width = TOOL_LINE_PX;
                });
        }
    }
}
