//! `tools/road_tool.gd`: builds roads between grid points. Straight (default): click-drag,
//! kept horizontal or vertical. Curved (V toggles): drag from the start to the bend point
//! (the road leaves the start heading there), release, then click where the road ends; the
//! road curves from the start to the end, pulled toward the bend point
//! (`RoadNetwork::curve_path`). Crossing or touching existing roads creates junctions
//! there (white dots in the preview). The preview turns red and the hint explains why
//! when the road can't be built. R again picks the next road type.

use bevy::prelude::*;
use tf_sim::grid;
use tf_sim::roads::lane_builder::polyline_length;
use tf_sim::roads::{RoadNetwork, RoadType};

use super::{BuildTool, Shape, ToolCtx, ToolOutput};
use crate::input::{Action, Pointer};

pub const VALID_COLOR: Color = Color::srgba(0.45, 0.85, 1.0, 0.6);
pub const INVALID_COLOR: Color = Color::srgba(1.0, 0.35, 0.3, 0.6);
pub const JUNCTION_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.9);
pub const GUIDE_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.4);
/// Distance between the arrows of a one-way road preview.
pub const ARROW_SPACING: f32 = 32.0;

fn idle_hint(name: &str) -> String {
    format!(
        "{name}: drag to build (over a road: convert it; one-way: drag the way traffic goes) · R: next type · V: curved roads · Esc: cancel"
    )
}

fn curve_idle_hint(name: &str) -> String {
    format!(
        "{name}, curved: drag from the start to the bend point, then click the end · V: straight roads · Esc: cancel"
    )
}

fn curve_end_hint(name: &str) -> String {
    format!("{name}, curved: click where the road ends · Esc: cancel")
}

/// The pointer snapped to the grid and kept inside the map
/// (`BuildTool.get_snapped_pointer_position`).
pub fn snapped_pointer(pointer: &Pointer) -> Vec2 {
    let rect = grid::map_rect();
    pointer.snapped.clamp(rect.position, rect.end())
}

/// Keeps the road horizontal or vertical, whichever is closer to the cursor.
pub fn axis_locked(from: Vec2, to: Vec2) -> Vec2 {
    let delta = to - from;
    if delta.x.abs() >= delta.y.abs() {
        Vec2::new(to.x, from.y)
    } else {
        Vec2::new(from.x, to.y)
    }
}

/// Chevrons along `path` showing which way it runs.
pub fn arrows(path: &[Vec2], color: Color) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let mut next = ARROW_SPACING * 0.5;
    let mut walked = 0.0;
    for pair in path.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = a.distance(b);
        if length <= 0.0 {
            continue;
        }
        let dir = (b - a) / length;
        let side = Vec2::new(-dir.y, dir.x);
        while next <= walked + length {
            let tip = a + dir * (next - walked);
            let back = tip - dir * 5.0;
            shapes.push(Shape::Line {
                points: vec![back + side * 4.0, tip, back - side * 4.0],
                color,
            });
            next += ARROW_SPACING;
        }
        walked += length;
    }
    shapes
}

pub struct RoadTool {
    road_type: &'static RoadType,
    /// Building curves instead of straight roads.
    pub curved: bool,
    dragging: bool,
    /// Curved mode: the start and bend point are set, the end follows the pointer.
    placing_end: bool,
    start: Vec2,
    control: Vec2,
    end: Vec2,
    hover: Option<Vec2>,
    error: String,
    /// The road being previewed (empty if none).
    path: Vec<Vec2>,
    /// Where the previewed road meets existing roads.
    junctions: Vec<Vec2>,
    shapes: Vec<Shape>,
    /// The preview has to be worked out again.
    dirty: bool,
}

impl Default for RoadTool {
    fn default() -> Self {
        Self {
            road_type: &RoadType::list_all()[0],
            curved: false,
            dragging: false,
            placing_end: false,
            start: Vec2::ZERO,
            control: Vec2::ZERO,
            end: Vec2::ZERO,
            hover: None,
            error: String::new(),
            path: Vec::new(),
            junctions: Vec::new(),
            shapes: Vec::new(),
            dirty: true,
        }
    }
}

impl RoadTool {
    pub fn road_type(&self) -> &'static RoadType {
        self.road_type
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn is_placing_end(&self) -> bool {
        self.placing_end
    }

    /// The road being previewed, and why it can't be built ("" if it can).
    pub fn previewed_path(&self) -> (&[Vec2], &str) {
        (&self.path, &self.error)
    }

    fn reset_drag(&mut self) -> bool {
        let was = self.dragging || self.placing_end;
        self.dragging = false;
        self.placing_end = false;
        self.dirty = true;
        was
    }

    fn update_preview(&mut self, roads: &RoadNetwork, out: &mut ToolOutput) {
        self.error.clear();
        self.junctions.clear();
        self.path.clear();
        let name = &self.road_type.display_name;
        let mut hint = if self.curved {
            curve_idle_hint(name)
        } else {
            idle_hint(name)
        };
        let hover = self.hover.unwrap_or(Vec2::ZERO);
        if self.dragging && self.curved {
            self.control = hover;
        } else if self.dragging {
            self.end = axis_locked(self.start, hover);
            if self.start != self.end {
                self.path = vec![self.start, self.end];
            }
        } else if self.placing_end {
            self.end = hover;
            hint = curve_end_hint(name);
            if self.end != self.start {
                self.path = if self.end != self.control {
                    RoadNetwork::curve_path(self.start, self.control, self.end)
                } else {
                    vec![self.start, self.end]
                };
            }
        }
        if !self.path.is_empty() {
            let plan = roads.plan_path(&self.path, 0, Some(self.road_type));
            self.error = plan.error;
            for &point in &plan.points {
                if roads.find_node_at(point, 0).is_some()
                    || roads
                        .find_segment_at(point, RoadNetwork::EPSILON, 0)
                        .is_some()
                {
                    self.junctions.push(point);
                }
            }
            let cells = (polyline_length(&self.path) / f64::from(grid::CELL_SIZE)).round() as i32;
            hint = if self.error.is_empty() {
                format!(
                    "{} · {} cell{}",
                    name,
                    cells,
                    if cells == 1 { "" } else { "s" }
                )
            } else {
                self.error.clone()
            };
        }
        out.hint(hint);
        self.build_shapes();
    }

    fn build_shapes(&mut self) {
        let mut shapes = Vec::new();
        let width = self.road_type.total_width() as f32;
        let (guide, valid) = (GUIDE_COLOR, VALID_COLOR);
        if self.dragging && self.curved {
            // Setting the bend point: the way the road will leave the start.
            shapes.push(Shape::Line {
                points: vec![self.start, self.control],
                color: guide,
            });
            shapes.push(Shape::Disc {
                center: self.start,
                radius: width * 0.5,
                color: valid,
            });
            shapes.push(Shape::Disc {
                center: self.control,
                radius: 4.0,
                color: guide,
            });
        } else if self.path.is_empty() {
            if let Some(hover) = self.hover {
                shapes.push(Shape::Disc {
                    center: hover,
                    radius: width * 0.5,
                    color: valid,
                });
            }
            if self.placing_end {
                shapes.push(Shape::Line {
                    points: vec![self.start, self.control],
                    color: guide,
                });
            }
        } else {
            if self.placing_end {
                shapes.push(Shape::Line {
                    points: vec![self.start, self.control, self.end],
                    color: guide,
                });
            }
            let color = if self.error.is_empty() {
                valid
            } else {
                INVALID_COLOR
            };
            shapes.push(Shape::Band {
                points: self.path.clone(),
                width,
                color,
            });
            for end in [self.path[0], self.path[self.path.len() - 1]] {
                shapes.push(Shape::Disc {
                    center: end,
                    radius: width * 0.5,
                    color,
                });
            }
            if self.error.is_empty() {
                for &center in &self.junctions {
                    shapes.push(Shape::Disc {
                        center,
                        radius: 4.0,
                        color: JUNCTION_COLOR,
                    });
                }
                if self.road_type.one_way {
                    shapes.extend(arrows(&self.path, JUNCTION_COLOR));
                }
            }
        }
        self.shapes = shapes;
    }
}

impl BuildTool for RoadTool {
    fn id(&self) -> &'static str {
        "RoadTool"
    }

    fn shows_grid(&self) -> bool {
        true
    }

    fn activate(&mut self, _ctx: &mut ToolCtx) {
        self.hover = None;
        self.dirty = true;
    }

    fn variants(&self) -> Vec<String> {
        RoadType::list_all().iter().map(|t| t.id.clone()).collect()
    }

    fn variant(&self) -> Option<String> {
        Some(self.road_type.id.clone())
    }

    fn set_variant(&mut self, variant: &str, ctx: &mut ToolCtx) {
        let Some(road_type) = RoadType::load_by_id(variant) else {
            return;
        };
        if std::ptr::eq(road_type, self.road_type) {
            return;
        }
        self.road_type = road_type;
        ctx.out.variant_changed = Some(variant.to_string());
        self.dirty = true;
    }

    fn cancel(&mut self, _ctx: &mut ToolCtx) -> bool {
        self.reset_drag()
    }

    fn update(&mut self, ctx: &mut ToolCtx) {
        let pointer = ctx.pointer;
        let mut build: Option<Vec<Vec2>> = None;
        if ctx.actions.contains(&Action::RoadCurveMode) {
            self.reset_drag();
            self.curved = !self.curved;
        }
        if pointer.right.just_pressed {
            self.reset_drag();
        }
        let hover = snapped_pointer(pointer);
        if self.hover != Some(hover) {
            self.hover = Some(hover);
            self.dirty = true;
        }
        {
            // Polled every frame so the preview follows keyboard panning too; at a click
            // the live network is used, as the last view may be a frame old.
            let clicked = pointer.left.just_pressed || pointer.left.just_released;
            let view;
            let roads = if clicked {
                &ctx.sim.get_mut().roads
            } else {
                view = ctx.sim.roads();
                &view.roads
            };
            if pointer.left.just_pressed {
                if self.placing_end {
                    self.update_preview(roads, ctx.out);
                    if self.error.is_empty() && !self.path.is_empty() {
                        build = Some(self.path.clone());
                    }
                    self.placing_end = false;
                } else {
                    self.dragging = true;
                    self.start = hover;
                }
                self.dirty = true;
            }
            if pointer.left.just_released && self.dragging {
                self.update_preview(roads, ctx.out);
                self.dragging = false;
                if self.curved {
                    // The bend point is set: now the end.
                    self.placing_end = self.start != self.control;
                } else if self.error.is_empty() && !self.path.is_empty() {
                    build = Some(self.path.clone());
                }
                self.dirty = true;
            }
            if self.dirty && build.is_none() {
                self.dirty = false;
                self.update_preview(roads, ctx.out);
            }
        }
        if let Some(path) = build {
            let road_type = self.road_type;
            ctx.sim.get_mut().call(|sim, out| {
                sim.roads.add_path_pieces(&path, road_type, 0, out);
            });
            self.path.clear();
            self.junctions.clear();
            self.error.clear();
            // Worked out again next frame, against the network the build changed.
            self.dirty = true;
            self.build_shapes();
        }
    }

    fn preview(&self, shapes: &mut Vec<Shape>) {
        shapes.extend(self.shapes.iter().cloned());
    }
}
