//! `tools/bus_line_tool.gd`: draws bus lines. Click roads to place stops (at the curb on
//! the side clicked, or an existing stop there) in the order buses call at them; click
//! the first stop again, or right-click, to close the loop and open the line.
//! Right-clicking a stop while no line is being drawn removes it (lines through it skip
//! it). Buses start running at once. Variants: curbside stops (buses stop in the lane)
//! or bus bays (they pull out of it); clicking an existing stop makes it the current kind.

use bevy::prelude::*;
use tf_sim::ids::StopId;
use tf_sim::transit::TransitSystem;
use tf_sim::transit::transit_system::CurbSpot;

use super::{BuildTool, Shape, ToolCtx};

pub const VARIANTS: [&str; 2] = ["curbside", "bay"];
pub const STOP_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.9);
pub const PREVIEW_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.5);
pub const LINE_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.6);
pub const STOP_RADIUS: f32 = 6.0;

fn variant_name(variant: &str) -> &'static str {
    if variant == "bay" {
        "Bus bays"
    } else {
        "Curbside stops"
    }
}

fn idle_hint(kind: &str) -> String {
    format!(
        "{kind}: click roads to place stops in order (each side of a street is its own stop) · right-click a stop to remove it"
    )
}

fn drawing_hint(kind: &str, stops: usize) -> String {
    format!(
        "{kind} · line: {stops} stops · click the first stop again or right-click to open the line · Esc cancels"
    )
}

#[derive(Default)]
pub struct BusLineTool {
    /// Stops of the line being drawn, and their positions (for drawing without waiting
    /// for the simulation).
    stop_ids: Vec<StopId>,
    stop_positions: Vec<Vec2>,
    /// Where a click would put a stop now (`None` if not on a road).
    hover: Option<CurbSpot>,
    bay: bool,
}

impl BusLineTool {
    /// The stops placed so far in the line being drawn.
    pub fn drawing(&self) -> &[StopId] {
        &self.stop_ids
    }

    fn kind(&self) -> &'static str {
        variant_name(if self.bay { "bay" } else { "curbside" })
    }

    fn hint(&self) -> String {
        if self.stop_ids.is_empty() {
            idle_hint(self.kind())
        } else {
            drawing_hint(self.kind(), self.stop_ids.len())
        }
    }

    fn open_line(&mut self, ctx: &mut ToolCtx) {
        let ids = std::mem::take(&mut self.stop_ids);
        self.stop_positions.clear();
        let mut line = None;
        ctx.sim.get_mut().call(|sim, _out| {
            let (transit, mut tctx) = sim.transit_ctx();
            if let Some(id) = transit.add_line(&mut tctx, &ids) {
                let l = &transit.lines[&id];
                line = Some((id, l.stop_ids.len(), l.bus_count));
            }
        });
        match line {
            None => ctx.out.notify("A bus line needs at least two stops"),
            Some((id, stops, buses)) => {
                ctx.out
                    .notify(format!("Bus line {id}: {stops} stops, {buses} buses"));
            }
        }
        ctx.out.hint(self.hint());
    }
}

impl BuildTool for BusLineTool {
    fn id(&self) -> &'static str {
        "BusLineTool"
    }

    fn activate(&mut self, ctx: &mut ToolCtx) {
        ctx.out.hint(self.hint());
    }

    fn variants(&self) -> Vec<String> {
        VARIANTS.iter().map(|v| v.to_string()).collect()
    }

    fn variant(&self) -> Option<String> {
        Some(VARIANTS[usize::from(self.bay)].to_string())
    }

    fn set_variant(&mut self, variant: &str, ctx: &mut ToolCtx) {
        if !VARIANTS.contains(&variant) {
            return;
        }
        let bay = variant == "bay";
        if bay == self.bay {
            return;
        }
        self.bay = bay;
        ctx.out.variant_changed = Some(variant.to_string());
        ctx.out.hint(self.hint());
    }

    fn cancel(&mut self, ctx: &mut ToolCtx) -> bool {
        if self.stop_ids.is_empty() {
            return false;
        }
        self.stop_ids.clear();
        self.stop_positions.clear();
        ctx.out.hint(self.hint());
        true
    }

    fn update(&mut self, ctx: &mut ToolCtx) {
        let pointer = ctx.pointer;
        let point = pointer.world;
        if pointer.right.just_pressed {
            if !self.stop_ids.is_empty() {
                self.open_line(ctx);
            } else {
                let sim = ctx.sim.get_mut();
                if let Some(id) = sim.transit.find_stop(point, TransitSystem::MERGE_DISTANCE) {
                    sim.call(|sim, out| {
                        let (transit, mut tctx) = sim.transit_ctx();
                        transit.remove_stop(&mut tctx, id, out);
                    });
                    ctx.out.notify("Bus stop removed");
                }
            }
        }
        if pointer.left.just_pressed {
            let bay = self.bay;
            let mut placed = None;
            let mut position = Vec2::ZERO;
            ctx.sim.get_mut().call(|sim, _out| {
                let (transit, mut tctx) = sim.transit_ctx();
                placed = transit.add_stop(&mut tctx, point, bay);
                if let Some(id) = placed {
                    if self.stop_ids.first() != Some(&id) {
                        transit.set_stop_bay(id, bay);
                    }
                    position = transit.stops[&id].position;
                }
            });
            match placed {
                None => ctx
                    .out
                    .notify("Bus stops go on a road, with room for a bus"),
                Some(id) if self.stop_ids.first() == Some(&id) => self.open_line(ctx),
                Some(id) => {
                    if !self.stop_ids.contains(&id) {
                        self.stop_ids.push(id);
                        self.stop_positions.push(position);
                    }
                    ctx.out.hint(self.hint());
                }
            }
        }
        let view = ctx.sim.roads();
        self.hover = TransitSystem::find_curb_spot(&view.roads, point);
    }

    fn preview(&self, shapes: &mut Vec<Shape>) {
        if self.stop_positions.len() >= 2 {
            shapes.push(Shape::Line {
                points: self.stop_positions.clone(),
                color: LINE_COLOR,
            });
        }
        for &center in &self.stop_positions {
            shapes.push(Shape::Disc {
                center,
                radius: STOP_RADIUS * 0.6,
                color: STOP_COLOR,
            });
        }
        if let Some(spot) = &self.hover {
            if let Some(&last) = self.stop_positions.last() {
                shapes.push(Shape::Dashed {
                    from: last,
                    to: spot.position,
                    color: PREVIEW_COLOR,
                });
            }
            shapes.push(Shape::Ring {
                center: spot.position,
                radius: STOP_RADIUS,
                color: PREVIEW_COLOR,
            });
        }
    }
}
