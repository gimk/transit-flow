//! Geometry the sim needs from Godot: `Curve2D`'s baked polyline (`Polyline`) and the
//! `Geometry2D` segment functions (`segment`).

pub mod polyline;
pub mod segment;

pub use polyline::Polyline;
