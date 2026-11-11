//! `Pedestrian` (`sim/walking/pedestrian.gd`): a person walking a route from door to
//! door. Only `PedestrianSystem` creates and moves them.

use glam::{IVec2, Vec2};

use crate::ids::{LotId, PedestrianId};
use crate::walking::crosswalks::NONE;

#[derive(Clone, Debug)]
pub struct Pedestrian {
    pub id: PedestrianId,
    /// Who the trip belongs to (a citizen id; -1 for none).
    pub owner_id: i32,
    pub destination_lot_id: Option<LotId>,

    /// The route: corner points, and per leg (between points i and i+1) the crosswalk it
    /// crosses (`NONE` if none).
    pub points: Vec<Vec2>,
    pub crossings: Vec<IVec2>,
    pub leg: usize,
    /// Distance walked along the current leg.
    pub leg_progress: f64,
    pub position: Vec2,

    /// On a crosswalk right now / waiting at its curb (`NONE` if not).
    pub on_crosswalk: IVec2,
    pub waiting_at: IVec2,

    /// Trip timing, like `Vehicle`'s (sim seconds), for the Flow score.
    pub trip_start_time: f64,
    pub ideal_trip_time: f64,
    pub trip_end_time: f64,
}

impl Pedestrian {
    pub fn new(id: PedestrianId) -> Self {
        Self {
            id,
            owner_id: -1,
            destination_lot_id: None,
            points: Vec::new(),
            crossings: Vec::new(),
            leg: 0,
            leg_progress: 0.0,
            position: Vec2::ZERO,
            on_crosswalk: NONE,
            waiting_at: NONE,
            trip_start_time: 0.0,
            ideal_trip_time: 0.0,
            trip_end_time: 0.0,
        }
    }
}
