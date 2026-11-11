//! `BusStop` (`sim/transit/bus_stop.gd`): a bus stop: a point on the outermost lane of a
//! road, for the traffic going one way. At a curbside stop buses dwell in the lane,
//! holding up the traffic behind; at a bus bay they pull out of it (`bay`). Kept as a
//! position and direction, since lane ids change as roads are edited: `TransitSystem`
//! finds its lane again after every road change.

use glam::Vec2;

use crate::ids::{CitizenId, LaneId, StopId};

#[derive(Clone, Debug, PartialEq)]
pub struct BusStop {
    pub id: StopId,
    pub position: Vec2,
    /// Direction of the traffic it serves.
    pub direction: Vec2,
    /// The lane it is on and where along it (`None`: no road there any more; Godot -1).
    pub lane_id: Option<LaneId>,
    pub distance: f64,
    /// A bus bay: buses pull out of the lane to dwell here, instead of stopping in it.
    pub bay: bool,
    /// Riders waiting here (the citizen ids of `TransitSystem`'s riders), first come
    /// first. Godot holds the `Rider` objects.
    pub waiting: Vec<CitizenId>,
    /// Riders who got on a bus here so far (for drawing them boarding).
    pub boarded: i32,
}

impl BusStop {
    pub fn new(id: StopId, position: Vec2, direction: Vec2) -> Self {
        Self {
            id,
            position,
            direction,
            lane_id: None,
            distance: 0.0,
            bay: false,
            waiting: Vec::new(),
            boarded: 0,
        }
    }

    pub fn is_active(&self) -> bool {
        self.lane_id.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_is_active_while_it_has_a_lane() {
        let mut stop = BusStop::new(StopId(1), Vec2::new(5.0, 6.0), Vec2::X);
        assert!(!stop.is_active());
        assert!(!stop.bay);
        stop.lane_id = Some(LaneId(3));
        assert!(stop.is_active());
        stop.lane_id = None;
        assert!(!stop.is_active());
    }
}
