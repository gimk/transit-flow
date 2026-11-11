//! `BusLine` (`sim/transit/bus_line.gd`): a bus line: its buses go round its stops in
//! order, from the last back to the first. `TransitSystem` keeps the timings riders plan
//! with: the free-flow drive between stops, and the time buses actually take, measured as
//! they run.

use glam::Vec2;

use crate::ids::{LineId, StopId};
use crate::routing::Route;

/// `Color.WHITE`, and the line colours: RGBA as Godot's `Color` holds them (`f32`).
pub type Color = [f32; 4];

#[derive(Clone, Debug, PartialEq)]
pub struct BusLine {
    pub id: LineId,
    pub stop_ids: Vec<StopId>,
    pub color: Color,
    /// How many buses run the line.
    pub bus_count: i32,
    /// Per stop i, from stop i to the next: the drive on empty roads, and the time from
    /// arriving at stop i to arriving at the next one as buses really do it (a moving
    /// average, dwell included). Infinite while the next stop can't be reached. `f32`
    /// like Godot's `PackedFloat32Array`.
    pub free_flow_times: Vec<f32>,
    pub segment_times: Vec<f32>,
    /// Per stop i: the way buses take from it to the next stop (`None` while there is
    /// none): fixed when the line is laid out or the roads change, so buses keep to their
    /// line.
    pub routes: Vec<Option<Route>>,
    /// The buses' way round, for drawing.
    pub path: Vec<Vec2>,
}

impl BusLine {
    pub const WHITE: Color = [1.0, 1.0, 1.0, 1.0];

    pub fn new(id: LineId, stop_ids: Vec<StopId>) -> Self {
        Self {
            id,
            stop_ids,
            color: Self::WHITE,
            bus_count: 1,
            free_flow_times: Vec::new(),
            segment_times: Vec::new(),
            routes: Vec::new(),
            path: Vec::new(),
        }
    }

    /// Time for a bus to go once round the line (infinite if it can't).
    pub fn loop_time(&self) -> f64 {
        let mut total = 0.0;
        for &time in &self.segment_times {
            total += f64::from(time);
        }
        total
    }

    /// Expected time between two buses at a stop.
    pub fn headway(&self) -> f64 {
        self.loop_time() / f64::from(self.bus_count.max(1))
    }

    /// Index of the stop after index `i`.
    pub fn next_index(&self, i: usize) -> usize {
        (i + 1) % self.stop_ids.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line() -> BusLine {
        BusLine::new(LineId(1), vec![StopId(1), StopId(2), StopId(3)])
    }

    #[test]
    fn the_loop_time_adds_the_segments_and_the_headway_shares_it_between_buses() {
        let mut line = line();
        line.segment_times = vec![10.0, 20.0, 30.0];
        assert_eq!(line.loop_time(), 60.0);
        assert_eq!(line.headway(), 60.0);
        line.bus_count = 4;
        assert_eq!(line.headway(), 15.0);
        line.bus_count = 0;
        assert_eq!(line.headway(), 60.0, "at least one bus counts");
    }

    #[test]
    fn a_segment_that_cant_be_reached_makes_the_loop_infinite() {
        let mut line = line();
        line.segment_times = vec![10.0, f32::INFINITY, 30.0];
        assert_eq!(line.loop_time(), f64::INFINITY);
        assert_eq!(line.headway(), f64::INFINITY);
    }

    #[test]
    fn the_next_stop_wraps_round_to_the_first() {
        let line = line();
        assert_eq!(line.next_index(0), 1);
        assert_eq!(line.next_index(2), 0);
    }
}
