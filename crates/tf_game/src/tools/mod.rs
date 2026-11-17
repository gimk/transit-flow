//! The player's build tools (`tools/` in Godot, M10): the `ToolManager` resource routes
//! keys and the pointer to the active `BuildTool`; each tool edits the sim only through
//! its command methods (`SimHandle::get_mut().call(|sim, out| ...)`), so the changes
//! reach the views as the usual change messages one frame later.
//!
//! - **Input:** tools read the `Pointer` resource (world / snapped position, buttons) and
//!   the actions pressed this frame, never the window or raw keys.
//! - **Selecting:** a tool's shortcut (`Action::TOOL_SHORTCUTS`) toggles it; pressed
//!   again while it is active it cycles its variants (road types, stop kinds) if it has
//!   any. `ToolRequested` / `ToolVariantRequested` messages do the same for a future UI.
//!   Esc aborts the tool's drag, and puts the tool away when there is none.
//! - **What the rest of the game sees:** `ToolManager::active_id` / `variant` / `hint`,
//!   the messages `ToolChanged`, `ToolVariantChanged`, `ToolHintChanged` and
//!   `NotificationPosted`, and `ShowGrid` (on while the active tool snaps to the grid).
//! - **Previews:** the active tool lists `Shape`s (sim coordinates) in `ToolPreview`;
//!   `views::tool_preview` draws them (filled shapes as one mesh, thin lines with gizmos).

use bevy::prelude::*;

use crate::events::*;
use crate::input::{Action, ActionInput, Pointer};
use crate::runner::SimHandle;
use crate::views::grid::ShowGrid;

pub mod bulldoze;
pub mod bus_line;
pub mod road;
pub mod route;
pub mod signal;
pub mod zone;

/// Something a tool draws in the world, in sim coordinates (y down).
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A filled band of world width along a polyline.
    Band {
        points: Vec<Vec2>,
        width: f32,
        color: Color,
    },
    Disc {
        center: Vec2,
        radius: f32,
        color: Color,
    },
    Rect {
        min: Vec2,
        max: Vec2,
        color: Color,
    },
    /// An outline of `width` world px, inside the rectangle.
    RectOutline {
        min: Vec2,
        max: Vec2,
        width: f32,
        color: Color,
    },
    /// A thin line of constant screen width (gizmos).
    Line {
        points: Vec<Vec2>,
        color: Color,
    },
    Dashed {
        from: Vec2,
        to: Vec2,
        color: Color,
    },
    /// A thin circle (gizmos).
    Ring {
        center: Vec2,
        radius: f32,
        color: Color,
    },
}

/// What the tools want drawn this frame.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct ToolPreview {
    pub shapes: Vec<Shape>,
}

/// What a tool reports back from a call: contextual help, notifications for the player
/// and a variant change. The manager turns them into messages.
#[derive(Default, Debug)]
pub struct ToolOutput {
    pub hint: Option<String>,
    pub notifications: Vec<String>,
    pub variant_changed: Option<String>,
}

impl ToolOutput {
    pub fn hint(&mut self, text: impl Into<String>) {
        self.hint = Some(text.into());
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.notifications.push(text.into());
    }
}

/// What a tool works with in one call.
pub struct ToolCtx<'a> {
    pub pointer: &'a Pointer,
    pub sim: &'a mut SimHandle,
    /// Actions pressed this frame (`V` for the road tool).
    pub actions: &'a [Action],
    pub out: &'a mut ToolOutput,
}

/// A player build tool (`BuildTool`). Only the active tool is called.
pub trait BuildTool: Send + Sync {
    /// The tool's id (the node name in Godot): `Action::TOOL_SHORTCUTS`' second column.
    fn id(&self) -> &'static str;

    fn activate(&mut self, _ctx: &mut ToolCtx) {}

    /// Called every frame while active: hover, clicks and drags from `ctx.pointer`.
    fn update(&mut self, ctx: &mut ToolCtx);

    /// Aborts any in-progress action; true if there was one.
    fn cancel(&mut self, _ctx: &mut ToolCtx) -> bool {
        false
    }

    /// Whether the building grid shows while the tool is active (tools that snap to it).
    fn shows_grid(&self) -> bool {
        false
    }

    /// Variants a tool comes in (road types, stop kinds), as ids; empty if none.
    fn variants(&self) -> Vec<String> {
        Vec::new()
    }

    fn variant(&self) -> Option<String> {
        None
    }

    /// Switches to a variant of `variants()`, and says so in `ctx.out.variant_changed`.
    fn set_variant(&mut self, _variant: &str, _ctx: &mut ToolCtx) {}

    /// The tool's preview this frame.
    fn preview(&self, shapes: &mut Vec<Shape>);

    /// The shortcut pressed again: the next variant. False if there is nothing to cycle.
    fn cycle_variant(&mut self, ctx: &mut ToolCtx) -> bool {
        let variants = self.variants();
        if variants.len() < 2 {
            return false;
        }
        let at = self
            .variant()
            .and_then(|v| variants.iter().position(|x| *x == v))
            .unwrap_or(0);
        let next = variants[(at + 1) % variants.len()].clone();
        self.set_variant(&next, ctx);
        true
    }
}

/// Routes input to the tools (`ToolManager`).
#[derive(Resource)]
pub struct ToolManager {
    tools: Vec<Box<dyn BuildTool>>,
    active: Option<usize>,
    hint: String,
    out: ToolOutput,
    /// Messages to write at the end of the frame.
    changed: Vec<Option<String>>,
}

impl Default for ToolManager {
    fn default() -> Self {
        Self {
            tools: vec![
                Box::new(zone::ZoneTool::residential()),
                Box::new(zone::ZoneTool::commercial()),
                Box::new(zone::ZoneTool::industrial()),
                Box::new(zone::ZoneTool::dezone()),
                Box::new(signal::SignalTool::default()),
                Box::new(bus_line::BusLineTool::default()),
                Box::new(route::RouteTool::default()),
                Box::new(road::RoadTool::default()),
                Box::new(bulldoze::BulldozeTool::default()),
            ],
            active: None,
            hint: String::new(),
            out: ToolOutput::default(),
            changed: Vec::new(),
        }
    }
}

impl ToolManager {
    /// The id of the active tool, if any.
    pub fn active_id(&self) -> Option<&'static str> {
        self.active.map(|i| self.tools[i].id())
    }

    /// The active tool's variant (road type id, stop kind), if it has one.
    pub fn variant(&self) -> Option<String> {
        self.active.and_then(|i| self.tools[i].variant())
    }

    /// The variant of any tool (the road tool keeps its type while another tool is active).
    pub fn variant_of(&self, tool_id: &str) -> Option<String> {
        self.tool(tool_id).and_then(|t| t.variant())
    }

    pub fn variants_of(&self, tool_id: &str) -> Vec<String> {
        self.tool(tool_id).map_or_else(Vec::new, |t| t.variants())
    }

    /// The contextual help for the active tool ("" if none).
    pub fn hint(&self) -> &str {
        &self.hint
    }

    pub fn tool(&self, tool_id: &str) -> Option<&dyn BuildTool> {
        self.tools
            .iter()
            .find(|t| t.id() == tool_id)
            .map(|t| t.as_ref())
    }

    fn index_of(&self, tool_id: &str) -> Option<usize> {
        self.tools.iter().position(|t| t.id() == tool_id)
    }

    /// `ToolManager.select_tool`: makes `tool_id` the active tool; the active tool again
    /// (or `None`) puts it away.
    fn select(&mut self, tool_id: Option<&str>, pointer: &Pointer, sim: &mut SimHandle) {
        let mut next = None;
        if let Some(id) = tool_id {
            match self.index_of(id) {
                Some(index) => next = Some(index),
                None => return,
            }
        }
        if next == self.active {
            next = None;
        }
        if let Some(index) = self.active {
            let mut ctx = ToolCtx {
                pointer,
                sim,
                actions: &[],
                out: &mut self.out,
            };
            self.tools[index].cancel(&mut ctx);
            self.out.hint("");
        }
        self.active = next;
        if let Some(index) = next {
            let mut ctx = ToolCtx {
                pointer,
                sim,
                actions: &[],
                out: &mut self.out,
            };
            self.tools[index].activate(&mut ctx);
        }
        self.changed.push(self.active_id().map(str::to_string));
    }

    /// `ToolManager.use_tool`: the tool, without toggling it off, in `variant` if given.
    fn use_tool(
        &mut self,
        tool_id: &str,
        variant: Option<&str>,
        pointer: &Pointer,
        sim: &mut SimHandle,
    ) {
        let Some(index) = self.index_of(tool_id) else {
            return;
        };
        if self.active != Some(index) {
            self.select(Some(tool_id), pointer, sim);
        }
        if let Some(variant) = variant {
            let mut ctx = ToolCtx {
                pointer,
                sim,
                actions: &[],
                out: &mut self.out,
            };
            self.tools[index].set_variant(variant, &mut ctx);
        }
    }

    /// One frame of tool input: shortcuts, Esc, then the active tool's update.
    fn run(&mut self, actions: &[Action], pointer: &Pointer, sim: &mut SimHandle) {
        let shortcut = Action::TOOL_SHORTCUTS
            .iter()
            .find(|(action, _)| actions.contains(action));
        if let Some(&(_, tool_id)) = shortcut {
            // The active tool's shortcut cycles its variants; otherwise it toggles the tool.
            let cycled = match self.active {
                Some(index) if self.tools[index].id() == tool_id => {
                    let mut ctx = ToolCtx {
                        pointer,
                        sim,
                        actions: &[],
                        out: &mut self.out,
                    };
                    self.tools[index].cycle_variant(&mut ctx)
                }
                _ => false,
            };
            if !cycled {
                self.select(Some(tool_id), pointer, sim);
            }
        } else if actions.contains(&Action::ToolCancel) {
            // First Escape aborts the current drag, the second one puts the tool away.
            let cancelled = match self.active {
                Some(index) => {
                    let mut ctx = ToolCtx {
                        pointer,
                        sim,
                        actions: &[],
                        out: &mut self.out,
                    };
                    self.tools[index].cancel(&mut ctx)
                }
                None => false,
            };
            if !cancelled {
                self.select(None, pointer, sim);
            }
        }
        if let Some(index) = self.active {
            let mut ctx = ToolCtx {
                pointer,
                sim,
                actions,
                out: &mut self.out,
            };
            self.tools[index].update(&mut ctx);
        }
    }

    /// Back to no tool, forgetting what the tools were doing (a new sim replaced the
    /// old one: stop ids and the like no longer mean anything).
    fn reset(&mut self, pointer: &Pointer, sim: &mut SimHandle) {
        for index in 0..self.tools.len() {
            let mut ctx = ToolCtx {
                pointer,
                sim,
                actions: &[],
                out: &mut self.out,
            };
            self.tools[index].cancel(&mut ctx);
        }
        if let Some(index) = self.active {
            // Re-activate so the tool starts from a clean state.
            let mut ctx = ToolCtx {
                pointer,
                sim,
                actions: &[],
                out: &mut self.out,
            };
            self.tools[index].activate(&mut ctx);
        }
    }
}

fn tool_input(
    actions: ActionInput,
    pointer: Res<Pointer>,
    mut handle: ResMut<SimHandle>,
    mut manager: ResMut<ToolManager>,
    mut preview: ResMut<ToolPreview>,
    mut show_grid: ResMut<ShowGrid>,
    mut messages: ToolMessages,
) {
    let manager = &mut *manager;
    let pointer = &*pointer;
    if messages.resets.read().count() > 0 {
        manager.reset(pointer, &mut handle);
    }
    for ToolRequested(id) in messages.requests.read() {
        manager.select(id.as_deref(), pointer, &mut handle);
    }
    for request in messages.variant_requests.read() {
        manager.use_tool(
            &request.tool_id,
            request.variant.as_deref(),
            pointer,
            &mut handle,
        );
    }
    let pressed: Vec<Action> = Action::ALL
        .into_iter()
        .filter(|a| actions.just_pressed(*a))
        .collect();
    manager.run(&pressed, pointer, &mut handle);

    let mut shapes = Vec::new();
    let mut grid = false;
    if let Some(index) = manager.active {
        manager.tools[index].preview(&mut shapes);
        grid = manager.tools[index].shows_grid();
    }
    if preview.shapes != shapes {
        preview.shapes = shapes;
    }
    if show_grid.0 != grid {
        show_grid.0 = grid;
    }
    manager.flush(&mut messages);
}

impl ToolManager {
    /// Turns what the tools reported into messages.
    fn flush(&mut self, messages: &mut ToolMessages) {
        for id in self.changed.drain(..) {
            messages.changed.write(ToolChanged(id));
        }
        if let Some(variant) = self.out.variant_changed.take() {
            let tool_id = self.active_id().unwrap_or_default().to_string();
            messages.variant.write(ToolVariantChanged {
                tool_id,
                variant: Some(variant),
            });
        }
        if let Some(hint) = self.out.hint.take()
            && hint != self.hint
        {
            self.hint = hint.clone();
            messages.hint.write(ToolHintChanged(hint));
        }
        for text in self.out.notifications.drain(..) {
            messages.notifications.write(NotificationPosted(text));
        }
    }
}

#[derive(bevy::ecs::system::SystemParam)]
struct ToolMessages<'w, 's> {
    changed: MessageWriter<'w, ToolChanged>,
    variant: MessageWriter<'w, ToolVariantChanged>,
    hint: MessageWriter<'w, ToolHintChanged>,
    notifications: MessageWriter<'w, NotificationPosted>,
    requests: MessageReader<'w, 's, ToolRequested>,
    variant_requests: MessageReader<'w, 's, ToolVariantRequested>,
    resets: MessageReader<'w, 's, SimulationReset>,
}

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolManager>()
            .init_resource::<ToolPreview>()
            .init_resource::<ShowGrid>()
            .add_systems(Update, tool_input.run_if(crate::play::not_playing));
    }
}
