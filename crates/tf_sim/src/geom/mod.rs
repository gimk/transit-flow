//! Geometry the sim needs from Godot: `Curve2D` with Godot's baking (`curve`), the
//! `Geometry2D` segment functions (`segment`), `Rect2` (`rect`), and `Vector2` methods
//! computed the way Godot does (`gd`).

pub mod curve;
pub mod gd;
pub mod rect;
pub mod segment;

pub use curve::{Curve2D, CurvePoint};
pub use gd::GdVec2;
pub use rect::Rect2;
