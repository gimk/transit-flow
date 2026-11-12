//! `tools/route_tool.gd`, the debug route inspector: click a road to set an origin, click
//! another to set a destination, and the quickest route between them is drawn. Clicking
//! again starts over. Positions snap to the nearest lane, so clicking either side of a
//! road picks its direction.

use bevy::prelude::*;
use tf_sim::roads::RoadNetwork;
use tf_sim::roads::road_network::LanePosition;
use tf_sim::routing::Route;

use super::{BuildTool, Shape, ToolCtx};

pub const PICK_DISTANCE: f64 = 18.0;
pub const ROUTE_COLOR: Color = Color::srgba(1.0, 0.85, 0.2, 0.85);
pub const ORIGIN_COLOR: Color = Color::srgb(0.35, 0.9, 0.45);
pub const DESTINATION_COLOR: Color = Color::srgb(1.0, 0.4, 0.35);
pub const IDLE_HINT: &str = "Click a road to set the route origin";
pub const ROUTE_WIDTH: f32 = 5.0;

#[derive(Default)]
pub struct RouteTool {
    origin: Option<LanePosition>,
    destination: Option<LanePosition>,
    route: Option<Route>,
    shapes: Vec<Shape>,
}

impl RouteTool {
    /// The route shown now, if there is one.
    pub fn route(&self) -> Option<&Route> {
        self.route.as_ref()
    }

    fn clear(&mut self, ctx: &mut ToolCtx) {
        self.origin = None;
        self.destination = None;
        self.route = None;
        self.shapes.clear();
        ctx.out.hint(IDLE_HINT);
    }

    fn compute_route(&mut self, ctx: &mut ToolCtx) {
        let (Some(origin), Some(destination)) = (self.origin, self.destination) else {
            return;
        };
        let sim = ctx.sim.get_mut();
        if !(sim.roads.lanes.contains_key(&origin.lane_id)
            && sim.roads.lanes.contains_key(&destination.lane_id))
        {
            self.clear(ctx);
            return;
        }
        self.route = sim.router.find_route(
            &sim.roads,
            origin.lane_id,
            origin.distance,
            destination.lane_id,
            destination.distance,
            false,
        );
        match &self.route {
            None => ctx.out.hint("No route between these points"),
            Some(route) => ctx.out.hint(format!(
                "Route: {} lanes · {:.1} s at free flow",
                route.lane_ids.len(),
                route.travel_time
            )),
        }
    }

    /// The drawing of the route and its ends against `roads`.
    fn draw(&mut self, roads: &RoadNetwork) {
        self.shapes.clear();
        if let Some(route) = &self.route {
            self.shapes.push(Shape::Band {
                points: route.polyline(roads),
                width: ROUTE_WIDTH,
                color: ROUTE_COLOR,
            });
        }
        for (picked, color) in [
            (self.origin, ORIGIN_COLOR),
            (self.destination, DESTINATION_COLOR),
        ] {
            if let Some(picked) = picked
                && let Some(lane) = roads.lanes.get(&picked.lane_id)
            {
                self.shapes.push(Shape::Disc {
                    center: lane.curve().sample_baked(picked.distance as f32),
                    radius: 6.0,
                    color,
                });
            }
        }
    }
}

impl BuildTool for RouteTool {
    fn id(&self) -> &'static str {
        "RouteTool"
    }

    fn activate(&mut self, ctx: &mut ToolCtx) {
        self.clear(ctx);
    }

    fn cancel(&mut self, ctx: &mut ToolCtx) -> bool {
        if self.origin.is_none() {
            return false;
        }
        self.clear(ctx);
        true
    }

    fn update(&mut self, ctx: &mut ToolCtx) {
        let pointer = ctx.pointer;
        if pointer.right.just_pressed {
            self.cancel(ctx);
        }
        let mut changed = false;
        if pointer.left.just_pressed {
            let picked =
                ctx.sim
                    .get_mut()
                    .roads
                    .find_lane_position(pointer.world, PICK_DISTANCE, 0);
            if let Some(picked) = picked {
                if self.origin.is_none() || self.destination.is_some() {
                    self.clear(ctx);
                    self.origin = Some(picked);
                    ctx.out.hint("Click a road to set the destination");
                } else {
                    self.destination = Some(picked);
                    self.compute_route(ctx);
                }
                changed = true;
            }
        }
        // The network may have changed under the displayed route; work it out again.
        let view = ctx.sim.roads();
        if let Some(route) = &self.route
            && !route
                .lane_ids
                .iter()
                .all(|id| view.roads.lanes.contains_key(id))
        {
            self.compute_route(ctx);
            changed = true;
        }
        if changed || !self.shapes.is_empty() {
            // The picked lanes may be gone with no route to notice it by.
            let view = ctx.sim.roads();
            self.draw(&view.roads);
        }
    }

    fn preview(&self, shapes: &mut Vec<Shape>) {
        shapes.extend(self.shapes.iter().cloned());
    }
}
