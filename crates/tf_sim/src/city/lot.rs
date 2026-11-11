//! `Lot` (`sim/city/lot.gd`): one grid cell of built-up land: a building and its parking,
//! on the part of the cell the roads around it leave free (`plot`, see
//! `LotSystem::compute_plot`).

use glam::{IVec2, Vec2};

use crate::geom::Rect2;
use crate::grid;
use crate::ids::LotId;

/// Godot's values (saved as ints).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum LotType {
    Residential = 0,
    Commercial = 1,
    Industrial = 2,
}

impl LotType {
    pub fn from_index(value: i32) -> LotType {
        match value {
            1 => LotType::Commercial,
            2 => LotType::Industrial,
            _ => LotType::Residential,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lot {
    pub id: LotId,
    pub cell: IVec2,
    pub lot_type: LotType,
    /// The land the lot stands on (world px): its cell, less the roads and sidewalks.
    pub plot: Rect2,
}

impl Lot {
    pub fn new(id: LotId, cell: IVec2, lot_type: LotType) -> Self {
        Self {
            id,
            cell,
            lot_type,
            plot: Rect2::default(),
        }
    }

    pub fn center(&self) -> Vec2 {
        grid::cell_center(self.cell)
    }
}
