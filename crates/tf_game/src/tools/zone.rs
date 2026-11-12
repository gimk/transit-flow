//! `tools/zone_tool.gd`: drag a rectangle of cells to zone them as a type, or to clear
//! their zoning (the dezone tool). Buildings appear on zoned cells next to a road after a
//! short while. Godot had one scene node per zone type plus one for erasing; here one
//! struct, four instances (`ZoneTool::residential()` ...).

use bevy::prelude::*;
use tf_sim::city::LotType;
use tf_sim::grid;

use super::{BuildTool, Shape, ToolCtx};
use crate::views::lots::zone_color;

pub const ERASE_COLOR: Color = Color::srgba(1.0, 0.35, 0.3, 0.35);

pub struct ZoneTool {
    id: &'static str,
    /// What the tool paints; `None` erases.
    zone_type: Option<LotType>,
    hint: &'static str,
    dragging: bool,
    start_cell: IVec2,
    hover_cell: Option<IVec2>,
}

impl ZoneTool {
    pub fn new(id: &'static str, zone_type: Option<LotType>, hint: &'static str) -> Self {
        Self {
            id,
            zone_type,
            hint,
            dragging: false,
            start_cell: IVec2::ZERO,
            hover_cell: None,
        }
    }

    pub fn residential() -> Self {
        Self::new(
            "ResidentialZoneTool",
            Some(LotType::Residential),
            "Drag to zone Residential (green): homes, whose people commute",
        )
    }

    pub fn commercial() -> Self {
        Self::new(
            "CommercialZoneTool",
            Some(LotType::Commercial),
            "Drag to zone Commercial (blue): shops, which employ people and need goods",
        )
    }

    pub fn industrial() -> Self {
        Self::new(
            "IndustrialZoneTool",
            Some(LotType::Industrial),
            "Drag to zone Industrial (yellow): factories, which employ people and ship goods",
        )
    }

    pub fn dezone() -> Self {
        Self::new(
            "DezoneTool",
            None,
            "Drag to remove zoning (and the buildings on it)",
        )
    }

    /// The cells covered by the current drag (just the hovered cell when not dragging).
    pub fn selected_cells(&self) -> Vec<IVec2> {
        let Some(to) = self.hover_cell else {
            return Vec::new();
        };
        let from = if self.dragging { self.start_cell } else { to };
        let (min, max) = (from.min(to), from.max(to));
        let mut cells = Vec::new();
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                cells.push(IVec2::new(x, y));
            }
        }
        cells
    }
}

impl BuildTool for ZoneTool {
    fn id(&self) -> &'static str {
        self.id
    }

    fn shows_grid(&self) -> bool {
        true
    }

    fn activate(&mut self, ctx: &mut ToolCtx) {
        self.hover_cell = None;
        ctx.out.hint(self.hint);
    }

    fn cancel(&mut self, ctx: &mut ToolCtx) -> bool {
        if !self.dragging {
            return false;
        }
        self.dragging = false;
        ctx.out.hint(self.hint);
        true
    }

    fn update(&mut self, ctx: &mut ToolCtx) {
        let pointer = ctx.pointer;
        let cell = grid::world_to_cell(pointer.world);
        if self.hover_cell != Some(cell) {
            self.hover_cell = Some(cell);
            if self.dragging {
                ctx.out
                    .hint(format!("{} cells", self.selected_cells().len()));
            }
        }
        if pointer.right.just_pressed {
            self.cancel(ctx);
        }
        if pointer.left.just_pressed {
            self.dragging = true;
            self.start_cell = cell;
        }
        if pointer.left.just_released && self.dragging {
            let cells = self.selected_cells();
            self.dragging = false;
            let zone_type = self.zone_type;
            ctx.sim.get_mut().call(|sim, out| match zone_type {
                Some(zone_type) => sim.zones.paint(&mut sim.lots, &cells, zone_type, out),
                None => sim.zones.erase(&mut sim.lots, &cells, out),
            });
            ctx.out.hint(self.hint);
        }
    }

    fn preview(&self, shapes: &mut Vec<Shape>) {
        let Some(to) = self.hover_cell else {
            return;
        };
        let mut color = ERASE_COLOR;
        if let Some(zone_type) = self.zone_type {
            color = zone_color(zone_type).with_alpha(0.35);
        }
        let from = if self.dragging { self.start_cell } else { to };
        let cell = grid::CELL_SIZE as f32;
        let min = from.min(to).as_vec2() * cell;
        let max = (from.max(to) + IVec2::ONE).as_vec2() * cell;
        shapes.push(Shape::Rect { min, max, color });
        shapes.push(Shape::RectOutline {
            min,
            max,
            width: 2.0,
            color: color.with_alpha(0.9),
        });
    }
}
