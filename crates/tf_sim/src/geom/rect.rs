//! `Rect2`: the parts of Godot's `Rect2` the sim uses. Add methods as ports need them,
//! with Godot's semantics.

use glam::Vec2;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect2 {
    pub position: Vec2,
    pub size: Vec2,
}

impl Rect2 {
    pub fn new(position: Vec2, size: Vec2) -> Self {
        Self { position, size }
    }

    pub fn end(&self) -> Vec2 {
        self.position + self.size
    }

    /// `Rect2.expand(to)`: the smallest rect holding this one and `to`.
    pub fn expand(&self, to: Vec2) -> Rect2 {
        let begin = self.position.min(to);
        let end = self.end().max(to);
        Rect2::new(begin, end - begin)
    }

    /// `Rect2.grow(by)`: grown by `by` on every side.
    pub fn grow(&self, by: f32) -> Rect2 {
        Rect2::new(
            self.position - Vec2::splat(by),
            self.size + Vec2::splat(by * 2.0),
        )
    }

    /// `Rect2.has_point(point)`: the left and top edges are inside, the right and
    /// bottom ones are not.
    pub fn has_point(&self, point: Vec2) -> bool {
        point.x >= self.position.x
            && point.y >= self.position.y
            && point.x < self.position.x + self.size.x
            && point.y < self.position.y + self.size.y
    }

    /// `Rect2.has_area()`.
    pub fn has_area(&self) -> bool {
        self.size.x > 0.0 && self.size.y > 0.0
    }
}
