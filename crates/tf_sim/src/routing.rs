//! Lane graph, routes and the router (`sim/routing/`).

/// Moved to `street::astar`; kept under this name until the old routing is deleted.
pub use crate::street::astar;
pub mod lane_graph;
pub mod route;
pub mod router;

pub use lane_graph::{Congestion, LaneGraph};
pub use route::Route;
pub use router::{LanePoint, Router};
