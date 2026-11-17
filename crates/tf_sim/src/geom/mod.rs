//! Geometry: `Curve2D` with Godot's baking (`curve`), the `Geometry2D` segment functions
//! (`segment`), `Rect2` (`rect`), `Vector2` methods computed the way Godot does (`gd`),
//! polylines for lanes (`lanes`), and polygons and circles (`polygon`).

pub mod curve;
pub mod gd;
pub mod lanes;
pub mod polygon;
pub mod rect;
pub mod segment;

pub use curve::{Curve2D, CurvePoint};
pub use gd::GdVec2;
pub use polygon::{Circle, Polygon};
pub use rect::Rect2;
