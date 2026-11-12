//! `tools/bulldoze_tool.gd`: click, or hold and sweep, to remove road segments or lots
//! under the cursor. Roads win when the cursor is close to one; otherwise the lot in the
//! cell is picked. A lot that zoning built also dezones its cell (or the zone would
//! just grow the building back).

use bevy::prelude::*;
use tf_sim::grid;
use tf_sim::ids::{LotId, SegmentId};
use tf_sim::roads::RoadNetwork;

use super::{BuildTool, Shape, ToolCtx};

pub const HIGHLIGHT_COLOR: Color = Color::srgba(1.0, 0.3, 0.25, 0.55);
pub const PICK_DISTANCE: f64 = 18.0;
pub const HINT: &str = "Click or drag over roads and lots to remove them";
/// While not holding, the lot under the cursor is looked up again this often (frames):
/// it needs the sim itself, so it is not done every frame.
const LOT_REFRESH_FRAMES: u32 = 30;

#[derive(Default)]
pub struct BulldozeTool {
    hovered_segment: Option<SegmentId>,
    hovered_lot: Option<LotId>,
    /// Cell the cached lot was looked up for.
    lot_cell: Option<IVec2>,
    frames: u32,
    /// Esc pressed during a hold: ignored until the button is released.
    suppressed: bool,
    shapes: Vec<Shape>,
}

impl BulldozeTool {
    pub fn hovered(&self) -> (Option<SegmentId>, Option<LotId>) {
        (self.hovered_segment, self.hovered_lot)
    }

    fn highlight(&mut self, sim: &tf_sim::Simulation) {
        let mut shapes = Vec::new();
        if let Some(lot) = self.hovered_lot.and_then(|id| sim.lots.lots.get(&id)) {
            let plot = lot.plot.grow(3.0);
            shapes.push(Shape::Rect {
                min: plot.position,
                max: plot.end(),
                color: HIGHLIGHT_COLOR,
            });
        }
        if let Some(segment) = self
            .hovered_segment
            .and_then(|id| sim.roads.segments.get(&id))
        {
            let width = segment.road_type.total_width() as f32 + 6.0;
            let points = segment.polyline().to_vec();
            if let (Some(&first), Some(&last)) = (points.first(), points.last()) {
                shapes.push(Shape::Band {
                    points,
                    width,
                    color: HIGHLIGHT_COLOR,
                });
                for center in [first, last] {
                    shapes.push(Shape::Disc {
                        center,
                        radius: width * 0.5,
                        color: HIGHLIGHT_COLOR,
                    });
                }
            }
        }
        self.shapes = shapes;
    }
}

fn segment_under(roads: &RoadNetwork, at: Vec2) -> Option<SegmentId> {
    roads.find_segment_at(at, PICK_DISTANCE, 0)
}

impl BuildTool for BulldozeTool {
    fn id(&self) -> &'static str {
        "BulldozeTool"
    }

    fn activate(&mut self, ctx: &mut ToolCtx) {
        ctx.out.hint(HINT);
        self.lot_cell = None;
    }

    fn cancel(&mut self, ctx: &mut ToolCtx) -> bool {
        let was_held = ctx.pointer.left.pressed && !self.suppressed;
        self.suppressed = ctx.pointer.left.pressed;
        was_held
    }

    fn update(&mut self, ctx: &mut ToolCtx) {
        let pointer = ctx.pointer;
        if !pointer.left.pressed && !pointer.left.just_pressed {
            self.suppressed = false;
        }
        let held = (pointer.left.pressed || pointer.left.just_pressed) && !self.suppressed;
        let at = pointer.world;
        let cell = grid::world_to_cell(at);
        self.frames += 1;
        let (segment, lot);
        if held {
            // Live, and acting on it.
            let sim = ctx.sim.get_mut();
            let segment_id = segment_under(&sim.roads, at);
            let lot_id = if segment_id.is_some() {
                None
            } else {
                sim.lots.lot_at_cell(cell)
            };
            if let Some(id) = segment_id {
                sim.call(|sim, out| {
                    sim.roads.remove_segment(id, out);
                });
            } else if let Some(id) = lot_id {
                let lot_cell = sim.lots.lots[&id].cell;
                if sim.zones.zones.contains_key(&lot_cell) {
                    // Otherwise the zone would just grow the building back.
                    sim.call(|sim, out| sim.zones.erase(&mut sim.lots, &[lot_cell], out));
                } else {
                    sim.call(|sim, out| {
                        sim.lots.remove_lot(id, out);
                    });
                }
            }
            (segment, lot) = (None, None);
            self.lot_cell = None;
        } else {
            let view = ctx.sim.roads();
            segment = segment_under(&view.roads, at);
            lot = if segment.is_some() {
                self.lot_cell = None;
                None
            } else if self.lot_cell == Some(cell) && !self.frames.is_multiple_of(LOT_REFRESH_FRAMES)
            {
                self.hovered_lot
            } else {
                self.lot_cell = Some(cell);
                ctx.sim.get_mut().lots.lot_at_cell(cell)
            };
        }
        if segment != self.hovered_segment || lot != self.hovered_lot || held {
            self.hovered_segment = segment;
            self.hovered_lot = lot;
            self.highlight(ctx.sim.get_mut());
        }
    }

    fn preview(&self, shapes: &mut Vec<Shape>) {
        shapes.extend(self.shapes.iter().cloned());
    }
}
