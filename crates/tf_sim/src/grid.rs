//! `Grid` (`core/grid.gd`): grid constants and snapping helpers. The grid is a building
//! aid only: the simulation itself works in world pixels. (A core helper in Godot; the
//! sim uses it, so it lives here.)

use glam::{IVec2, Vec2};

use crate::geom::Rect2;

pub const CELL_SIZE: i32 = 64;
pub const MAP_SIZE_CELLS: IVec2 = IVec2::new(128, 128);

pub fn map_rect() -> Rect2 {
    Rect2::new(Vec2::ZERO, (MAP_SIZE_CELLS * CELL_SIZE).as_vec2())
}

/// Cell containing a world position (floor division, so negatives behave).
pub fn world_to_cell(world_pos: Vec2) -> IVec2 {
    (world_pos / CELL_SIZE as f32).floor().as_ivec2()
}

pub fn cell_center(cell: IVec2) -> Vec2 {
    (cell.as_vec2() + Vec2::splat(0.5)) * CELL_SIZE as f32
}

/// Nearest grid line intersection, where road nodes snap to.
pub fn snap_to_point(world_pos: Vec2) -> Vec2 {
    (world_pos / CELL_SIZE as f32).round() * CELL_SIZE as f32
}

pub fn is_inside_map(world_pos: Vec2) -> bool {
    let rect = map_rect();
    world_pos.x >= rect.position.x
        && world_pos.y >= rect.position.y
        && world_pos.x <= rect.end().x
        && world_pos.y <= rect.end().y
}

#[cfg(test)]
mod tests {
    //! Port of `tests/unit/test_grid.gd`.
    use super::*;

    #[test]
    fn snap_to_point_rounds_to_nearest_grid_intersection() {
        assert_eq!(snap_to_point(Vec2::new(95.0, 33.0)), Vec2::new(64.0, 64.0));
        assert_eq!(snap_to_point(Vec2::new(31.0, 97.0)), Vec2::new(0.0, 128.0));
    }

    #[test]
    fn world_to_cell_floors_negative_coordinates() {
        assert_eq!(world_to_cell(Vec2::new(-1.0, 0.0)), IVec2::new(-1, 0));
        assert_eq!(world_to_cell(Vec2::new(64.0, 127.0)), IVec2::new(1, 1));
    }

    #[test]
    fn cell_center_is_the_middle_of_the_cell() {
        assert_eq!(cell_center(IVec2::new(1, 2)), Vec2::new(96.0, 160.0));
    }

    #[test]
    fn map_bounds() {
        let size = (MAP_SIZE_CELLS * CELL_SIZE).as_vec2();
        assert!(is_inside_map(Vec2::ZERO));
        assert!(is_inside_map(size));
        assert!(!is_inside_map(Vec2::new(-1.0, 10.0)));
    }
}
