//! A uniform grid over the level, for "who is near this point" (pivot plan §5.2). It is
//! rebuilt for every snapshot. Items come back sorted, so a query has no hidden order.

use glam::Vec2;

use crate::street::level::Bounds;

/// Metres. About two car lengths: a query of a braking distance touches few cells.
pub const CELL: f32 = 8.0;

#[derive(Clone, Debug, Default)]
pub struct Grid {
    min: Vec2,
    cell: f32,
    cols: usize,
    rows: usize,
    cells: Vec<Vec<u32>>,
}

impl Grid {
    pub fn new(bounds: Bounds) -> Self {
        Self::with_cell(bounds, CELL)
    }

    /// A grid with cells of `cell` metres (walkers query a few metres around them).
    pub fn with_cell(bounds: Bounds, cell: f32) -> Self {
        let size = bounds.size();
        let cols = (size.x / cell).ceil().max(1.0) as usize;
        let rows = (size.y / cell).ceil().max(1.0) as usize;
        Self {
            min: bounds.min,
            cell,
            cols,
            rows,
            cells: vec![Vec::new(); cols * rows],
        }
    }

    /// Points outside the bounds go to the nearest edge cell (agents enter and leave
    /// through the edges).
    fn cell_of(&self, point: Vec2) -> (usize, usize) {
        let local = (point - self.min) / self.cell;
        let col = (local.x.floor().max(0.0) as usize).min(self.cols - 1);
        let row = (local.y.floor().max(0.0) as usize).min(self.rows - 1);
        (col, row)
    }

    pub fn insert(&mut self, item: u32, point: Vec2) {
        let (col, row) = self.cell_of(point);
        self.cells[row * self.cols + col].push(item);
    }

    /// Every item in a cell the circle touches (so some are a little further than
    /// `radius`), sorted.
    pub fn query(&self, point: Vec2, radius: f32, out: &mut Vec<u32>) {
        out.clear();
        let (col0, row0) = self.cell_of(point - Vec2::splat(radius));
        let (col1, row1) = self.cell_of(point + Vec2::splat(radius));
        for row in row0..=row1 {
            for col in col0..=col1 {
                out.extend(&self.cells[row * self.cols + col]);
            }
        }
        out.sort_unstable();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_finds_what_is_near_and_not_what_is_far() {
        let mut grid = Grid::new(Bounds {
            min: Vec2::ZERO,
            max: Vec2::splat(150.0),
        });
        grid.insert(2, Vec2::new(140.0, 140.0));
        grid.insert(0, Vec2::new(10.0, 10.0));
        grid.insert(1, Vec2::new(14.0, 12.0));
        grid.insert(3, Vec2::new(-5.0, 200.0));
        let mut found = Vec::new();
        grid.query(Vec2::new(12.0, 12.0), 5.0, &mut found);
        assert_eq!(found, vec![0, 1]);
        grid.query(Vec2::new(0.0, 149.0), 3.0, &mut found);
        assert_eq!(found, vec![3], "outside points sit in the edge cell");
    }
}
