//! Typed ids. GDScript uses plain `int` ids (and `-1` for "none"); Rust wraps each kind
//! in its own type so a lane id can't be passed where a node id is expected, and uses
//! `Option<Id>` instead of `-1`. Ids keep Godot's values and counters (they're saved),
//! and serialize as plain integers.

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub i32);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    )*};
}

id_type!(
    /// `RoadNode.id`.
    NodeId,
    /// `RoadSegment.id`.
    SegmentId,
    /// `Lane.id` (road lanes and junction connectors).
    LaneId,
    /// `Vehicle.id`.
    VehicleId,
    /// `Lot.id`.
    LotId,
    /// `Citizen.id`.
    CitizenId,
    /// `Pedestrian.id`.
    PedestrianId,
    /// `BusStop.id`.
    StopId,
    /// `BusLine.id`.
    LineId,
);
