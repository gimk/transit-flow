//! `Router` (`sim/routing/router.gd`): a stub until work package M2.1 ports it. It only has the methods
//! `Simulation` already calls, with empty bodies.

use crate::events::Outbox;
use crate::roads::ChangeSet;

/// Stub: see the module docs.
#[derive(Debug, Default)]
pub struct Router {}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_roads_changed(&mut self, _change: &ChangeSet, _out: &mut Outbox) {}
}
