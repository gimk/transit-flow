//! `tools/signal_tool.gd`: click a junction where 3+ roads meet to cycle its control: none,
//! traffic lights, lights with protected left turns. Without lights, drivers give way by
//! priority.

use bevy::prelude::*;
use tf_sim::ids::NodeId;
use tf_sim::roads::RoadNetwork;
use tf_sim::roads::road_node::JunctionControl;

use super::{BuildTool, Shape, ToolCtx};

pub const PICK_DISTANCE: f32 = 24.0;
pub const HIGHLIGHT_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.9);
pub const IDLE_HINT: &str =
    "Click a junction (3+ roads): no lights → lights → lights with protected left turns";

pub fn control_name(control: JunctionControl) -> &'static str {
    match control {
        JunctionControl::None => {
            "No lights (the bigger road has priority, then the driver on the right)"
        }
        JunctionControl::TrafficLight => {
            "Traffic lights (directions alternate; left turns find gaps)"
        }
        JunctionControl::TrafficLightProtectedLeft => {
            "Traffic lights with protected left turns (their own phase)"
        }
    }
}

/// The control after `control` in the cycle.
pub fn next_control(control: JunctionControl) -> JunctionControl {
    match control {
        JunctionControl::None => JunctionControl::TrafficLight,
        JunctionControl::TrafficLight => JunctionControl::TrafficLightProtectedLeft,
        JunctionControl::TrafficLightProtectedLeft => JunctionControl::None,
    }
}

/// The junction (3+ roads) within `PICK_DISTANCE` of `position`, the nearest.
fn find_junction(roads: &RoadNetwork, position: Vec2) -> Option<NodeId> {
    let mut best = None;
    let mut best_distance = PICK_DISTANCE;
    for node in roads.nodes.values() {
        let distance = node.position.distance(position);
        if node.degree() >= 3 && distance <= best_distance {
            best = Some(node.id);
            best_distance = distance;
        }
    }
    best
}

#[derive(Default)]
pub struct SignalTool {
    hovered: Option<NodeId>,
    shapes: Vec<Shape>,
}

impl SignalTool {
    pub fn hovered(&self) -> Option<NodeId> {
        self.hovered
    }
}

impl BuildTool for SignalTool {
    fn id(&self) -> &'static str {
        "SignalTool"
    }

    fn activate(&mut self, ctx: &mut ToolCtx) {
        self.hovered = None;
        ctx.out.hint(IDLE_HINT);
    }

    fn update(&mut self, ctx: &mut ToolCtx) {
        let pointer = ctx.pointer;
        if pointer.left.just_pressed {
            let sim = ctx.sim.get_mut();
            if let Some(id) = find_junction(&sim.roads, pointer.world) {
                let next = next_control(sim.roads.nodes[&id].control);
                sim.call(|sim, out| {
                    sim.roads.set_node_control(id, next, out);
                });
            }
        }
        // Hover and hint from the latest published network (a click shows next frame).
        let view = ctx.sim.roads();
        let roads = &view.roads;
        self.hovered = find_junction(roads, pointer.world);
        self.shapes.clear();
        match self.hovered.and_then(|id| roads.nodes.get(&id)) {
            Some(node) => {
                ctx.out
                    .hint(format!("{} · click to change", control_name(node.control)));
                self.shapes.push(Shape::Ring {
                    center: node.position,
                    radius: PICK_DISTANCE,
                    color: HIGHLIGHT_COLOR,
                });
            }
            None => ctx.out.hint(IDLE_HINT),
        }
    }

    fn preview(&self, shapes: &mut Vec<Shape>) {
        shapes.extend(self.shapes.iter().cloned());
    }
}
