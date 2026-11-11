//! The sim's signals, as values. In Godot, systems emit signals and the listeners run
//! at once, inside the emitter's call. Here a system pushes a `SimEvent` into the
//! `Outbox` it was given, and `Simulation` delivers it to the listeners, in the order
//! Godot connected them, as soon as the emitting call returns
//! (`Simulation::deliver`). The exact timing per signal is in
//! `docs/migration/NOTES.md` ("Event dispatch").

use glam::Vec2;

use crate::ids::{CitizenId, PedestrianId, VehicleId};
use crate::roads::ChangeSet;

/// One emitted signal. Names follow the GDScript signals.
#[derive(Clone, Debug, PartialEq)]
pub enum SimEvent {
    /// `RoadNetwork.changed(node_ids, segment_ids)`. Ids may refer to removed things.
    RoadsChanged(ChangeSet),
    /// `LotSystem.changed`.
    LotsChanged,
    /// `ZoneSystem.changed` (views only).
    ZonesChanged,
    /// `ExternalConnections.changed` (views only).
    ExternalsChanged,
    /// `SimClock.hour_changed(hour)` (views only).
    HourChanged(i32),
    /// `SimClock.day_started(day)` (views only).
    DayStarted(i32),
    /// `VehicleSystem.vehicle_arrived(vehicle)`. A vehicle that left the road is kept in
    /// `VehicleSystem`'s finished list until delivery ends, so listeners can read it.
    VehicleArrived(VehicleId),
    /// `VehicleSystem.vehicle_failed(vehicle)` (same lookup as `VehicleArrived`).
    VehicleFailed(VehicleId),
    /// `PedestrianSystem.pedestrian_arrived(pedestrian)` (kept like vehicles).
    PedestrianArrived(PedestrianId),
    /// `TransitSystem.rider_arrived(citizen_id, door)`.
    RiderArrived { citizen: CitizenId, door: Vec2 },
    /// `TransitSystem.rider_stranded(citizen_id)`.
    RiderStranded(CitizenId),
    /// `CitizenSystem.trip_ended(citizen, completed)`.
    TripEnded { citizen: CitizenId, completed: bool },
}

/// Where a system puts the events it emits during one call.
pub type Outbox = Vec<SimEvent>;
