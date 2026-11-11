//! `ViewSnapshot` (`core/view_snapshot.gd`): what the screen needs from one simulation
//! tick, copied out of the simulation right after the tick. The simulation runs on a
//! worker thread (see `runner`), so per-frame views (vehicles, lights, HUD, day/night,
//! heatmap) draw from snapshots instead of reading the simulation while it is being
//! stepped. Captured on the worker; read-only afterwards (the runner shares them as
//! `Arc<ViewSnapshot>`).
//!
//! It lives in `tf_game` rather than `tf_sim`: it is view data (lamps, colours, lateness
//! for a debug colouring), and the sim itself never reads it. Colours are RGBA `f32`
//! arrays (`[f32; 4]`, sRGB), the type `BusLine::color` already uses.

use glam::{IVec2, Vec2};
use indexmap::IndexMap;
use std::collections::HashMap;
use tf_sim::Simulation;
use tf_sim::city::Mode;
use tf_sim::ids::{LotId, NodeId, PedestrianId, StopId, VehicleId};
use tf_sim::roads::Turn;
use tf_sim::traffic::traffic_lights::Light;
use tf_sim::traffic::vehicle::{Kind, Vehicle};
use tf_sim::traffic::vehicle_system::{IMPATIENT_FACTOR, IMPATIENT_GRACE};
use tf_sim::walking::crosswalks::MidBlockCrossing;

pub type Color = [f32; 4];

/// `VehicleRenderer.CAR_COLOR` and `TRUCK_COLOR`.
pub const CAR_COLOR: Color = [0.4, 0.733_333_3, 0.415_686_27, 1.0];
pub const TRUCK_COLOR: Color = [0.552_941_2, 0.431_372_55, 0.388_235_3, 1.0];
/// Lamp flags (`VehicleRenderer.LAMP_*`): what the `indicators` array holds.
pub const LAMP_BRAKE: u8 = 1;
pub const LAMP_LEFT: u8 = 2;
pub const LAMP_RIGHT: u8 = 4;
/// Drivers signal this far before a turn or their driveway (px).
pub const SIGNAL_DISTANCE: f64 = 50.0;
/// A vehicle that hasn't moved for this long (sim seconds) counts as stuck.
pub const STUCK_SECONDS: f64 = 30.0;

/// One bus line as the views draw it.
#[derive(Clone, Debug, PartialEq)]
pub struct BusLineView {
    pub color: Color,
    pub path: Vec<Vec2>,
}

#[derive(Clone, Debug, Default)]
pub struct ViewSnapshot {
    /// Sim time of the tick (sim seconds).
    pub time: f64,
    /// Vehicle id -> index into the arrays below.
    pub vehicle_index: HashMap<VehicleId, usize>,
    pub positions: Vec<Vec2>,
    pub rotations: Vec<f32>,
    /// Length x width per vehicle.
    pub sizes: Vec<Vec2>,
    pub colors: Vec<Color>,
    /// `Kind` per vehicle (as its Godot value), its speed (px/s; renderers tell braking
    /// from the change between snapshots) and its indicator (`LAMP_LEFT` / `LAMP_RIGHT`,
    /// or 0).
    pub kinds: Vec<u8>,
    pub speeds: Vec<f32>,
    pub indicators: Vec<u8>,
    /// How late each vehicle is, from 0 (within its ideal trip time) to 1 (late enough
    /// for its driver to turn impatient), for the lateness view.
    pub lateness: Vec<f32>,
    /// Lot id -> citizens' cars parked there (not driving, not waiting in the driveway).
    pub parked: HashMap<LotId, i32>,
    /// Pedestrian id -> index into `pedestrian_positions`.
    pub pedestrian_index: HashMap<PedestrianId, usize>,
    pub pedestrian_positions: Vec<Vec2>,
    /// Light-controlled junction node id -> light per phase.
    pub lights: HashMap<NodeId, Vec<Light>>,
    /// Bus stops: where, the direction of the traffic they serve, riders waiting, bay or
    /// not, id, and how many riders have boarded there so far.
    pub stop_positions: Vec<Vec2>,
    pub stop_directions: Vec<Vec2>,
    pub stop_waiting: Vec<i32>,
    pub stop_bays: Vec<bool>,
    pub stop_ids: Vec<StopId>,
    pub stop_boarded: Vec<i32>,
    pub bus_lines: Vec<BusLineView>,
    /// Crosswalks at bus stops (`Crosswalks::mid_block`).
    pub stop_crossings: IndexMap<IVec2, MidBlockCrossing>,
    /// Lane id -> congestion. Godot shares the monitor's dictionary (it replaces it, never
    /// edits it); here it is copied, which only costs on maps with many lanes and is
    /// cheap next to a tick.
    pub congestion: IndexMap<tf_sim::ids::LaneId, f64>,

    pub day: i32,
    pub time_text: String,
    pub period_name: String,
    pub hours: f64,
    pub vehicle_count: usize,
    /// Vehicles waiting in driveways to pull out.
    pub waiting_count: usize,
    pub stuck_count: usize,
    /// Where those stuck vehicles are.
    pub stuck_positions: Vec<Vec2>,
    pub population: usize,
    pub employed: usize,
    pub jobs: i32,
    /// -1 while no trip has ended yet.
    pub flow_percent: f64,
    /// Mode -> share of citizens' trips over the last day.
    pub mode_shares: IndexMap<Mode, f64>,
}

impl ViewSnapshot {
    /// Copies what the views need out of `sim` (needs `&mut`: Flow forgets old trips as
    /// it is read).
    pub fn capture(sim: &mut Simulation) -> Self {
        let mut snapshot = ViewSnapshot {
            time: sim.sim_time(),
            ..Default::default()
        };
        let waiting = sim.vehicles.waiting_departures();
        let count = sim.vehicles.vehicles.len() + waiting.len();
        snapshot.vehicle_index.reserve(count);
        snapshot.positions.reserve(count);
        snapshot.rotations.reserve(count);
        snapshot.sizes.reserve(count);
        snapshot.colors.reserve(count);
        snapshot.kinds.reserve(count);
        snapshot.speeds.reserve(count);
        snapshot.indicators.reserve(count);
        snapshot.lateness.reserve(count);
        let mut stuck = 0;
        // Vehicles waiting in driveways are drawn too, parked on their lot.
        for vehicle in sim.vehicles.vehicles.values().chain(waiting) {
            let i = snapshot.positions.len();
            snapshot.vehicle_index.insert(vehicle.id, i);
            snapshot.positions.push(vehicle.position);
            snapshot.rotations.push(vehicle.rotation as f32);
            snapshot
                .sizes
                .push(Vec2::new(vehicle.length as f32, vehicle.width as f32));
            snapshot.colors.push(if vehicle.kind == Kind::Bus {
                sim.transit.bus_color(vehicle.id)
            } else {
                color_for_vehicle(vehicle)
            });
            snapshot.kinds.push(vehicle.kind as u8);
            snapshot.speeds.push(vehicle.speed as f32);
            snapshot.indicators.push(indicator_for(sim, vehicle));
            let late_after = vehicle.ideal_trip_time * (IMPATIENT_FACTOR - 1.0) + IMPATIENT_GRACE;
            let late_by = snapshot.time - vehicle.trip_start_time - vehicle.ideal_trip_time;
            snapshot
                .lateness
                .push((late_by / late_after).clamp(0.0, 1.0) as f32);
            if vehicle.stopped_time >= STUCK_SECONDS {
                stuck += 1;
                snapshot.stuck_positions.push(vehicle.position);
            }
        }
        for node in sim.roads.nodes.values() {
            if node.has_lights() {
                let phases = (0..node.signal_phase_count)
                    .map(|phase| sim.signals.light(node, phase))
                    .collect();
                snapshot.lights.insert(node.id, phases);
            }
        }
        for (i, pedestrian) in sim.pedestrians.pedestrians.values().enumerate() {
            snapshot.pedestrian_index.insert(pedestrian.id, i);
            snapshot.pedestrian_positions.push(pedestrian.position);
        }
        for stop in sim.transit.stops.values().filter(|stop| stop.is_active()) {
            snapshot.stop_positions.push(stop.position);
            snapshot.stop_directions.push(stop.direction);
            snapshot.stop_waiting.push(stop.waiting.len() as i32);
            snapshot.stop_bays.push(stop.bay);
            snapshot.stop_ids.push(stop.id);
            snapshot.stop_boarded.push(stop.boarded);
        }
        snapshot.stop_crossings = sim.crosswalks.mid_block.clone();
        snapshot.bus_lines = sim
            .transit
            .lines
            .values()
            .map(|line| BusLineView {
                color: line.color,
                path: line.path.clone(),
            })
            .collect();
        snapshot.congestion = sim.congestion.congestion.clone();
        snapshot.day = sim.clock.day();
        snapshot.time_text = sim.clock.format_time();
        snapshot.period_name = sim.clock.period_name().to_string();
        snapshot.hours = sim.clock.time_of_day_hours();
        snapshot.vehicle_count = sim.vehicles.vehicles.len();
        snapshot.waiting_count = sim.vehicles.waiting_departures().len();
        snapshot.stuck_count = stuck;
        snapshot.population = sim.citizens.citizens.len();
        for citizen in sim.citizens.citizens.values() {
            if citizen.has_car && citizen.vehicle_id.is_none() {
                *snapshot.parked.entry(citizen.car_lot_id).or_insert(0) += 1;
            }
        }
        snapshot.employed = sim.citizens.employed_count();
        snapshot.jobs = sim.citizens.job_count();
        let time = snapshot.time;
        snapshot.flow_percent = sim.flow.flow_percent(time);
        snapshot.mode_shares = sim.flow.mode_shares(time);
        snapshot
    }

    /// The light a phase of a junction showed (green if unknown, e.g. lights just added).
    pub fn light(&self, node_id: NodeId, phase: usize) -> Light {
        self.lights
            .get(&node_id)
            .and_then(|phases| phases.get(phase))
            .copied()
            .unwrap_or(Light::Green)
    }

    /// Index of a vehicle in both of two snapshots (to interpolate it), if it is in
    /// `self` and `other`.
    pub fn index_of(&self, id: VehicleId) -> Option<usize> {
        self.vehicle_index.get(&id).copied()
    }

    /// Where vehicle `id` is drawn at `alpha` (0-1) of the way from `from` to `to`:
    /// interpolated if it is in both, as it is in `to` otherwise (new in `to`), as in
    /// `from` if it left. `None` if it is in neither. Returns position and heading
    /// (radians, turned the short way round).
    pub fn vehicle_pose(
        from: &ViewSnapshot,
        to: &ViewSnapshot,
        id: VehicleId,
        alpha: f32,
    ) -> Option<(Vec2, f32)> {
        match (from.index_of(id), to.index_of(id)) {
            (Some(a), Some(b)) => Some((
                from.positions[a].lerp(to.positions[b], alpha),
                lerp_angle(from.rotations[a], to.rotations[b], alpha),
            )),
            (None, Some(b)) => Some((to.positions[b], to.rotations[b])),
            (Some(a), None) => Some((from.positions[a], from.rotations[a])),
            (None, None) => None,
        }
    }
}

/// `lerp_angle`: interpolates along the shorter arc.
pub fn lerp_angle(from: f32, to: f32, weight: f32) -> f32 {
    use std::f32::consts::TAU;
    let difference = (to - from) % TAU;
    let shortest = (2.0 * difference) % TAU - difference;
    from + shortest * weight
}

/// Normal colour of a vehicle that isn't a bus (buses take their line's colour): cars in
/// shades of green, so they still read as people's cars but don't all look alike.
pub fn color_for_vehicle(vehicle: &Vehicle) -> Color {
    if vehicle.kind == Kind::Truck {
        return TRUCK_COLOR;
    }
    car_color(vehicle.id.0)
}

/// A car's colour: a shade of green picked by `seed` (a different hash from Godot's, so
/// the exact shades differ; the range is the same).
pub fn car_color(seed: i32) -> Color {
    let shade = hash(seed);
    let (hue, saturation, value) = rgb_to_hsv(CAR_COLOR);
    let hue = hue + ((shade % 64) as f32 / 63.0 - 0.5) * 0.08;
    let value = value * (0.75 + ((shade >> 6) % 64) as f32 / 63.0 * 0.35);
    hsv_to_rgb(hue, saturation * 0.9, value.clamp(0.0, 1.0))
}

/// A well-mixed 32-bit integer hash (murmur3's finalizer), as a non-negative `i64`.
fn hash(seed: i32) -> i64 {
    let mut x = seed as u32;
    x ^= x >> 16;
    x = x.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 13;
    x = x.wrapping_mul(0xc2b2_ae35);
    x ^= x >> 16;
    i64::from(x)
}

fn rgb_to_hsv(color: Color) -> (f32, f32, f32) {
    let [r, g, b, _] = color;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    (hue, if max == 0.0 { 0.0 } else { delta / max }, max)
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> Color {
    let h = hue.rem_euclid(1.0) * 6.0;
    let sector = h.floor();
    let f = h - sector;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * f);
    let t = value * (1.0 - saturation * (1.0 - f));
    let (r, g, b) = match sector as i32 {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    [r, g, b, 1.0]
}

/// Which indicator a driver has on (`LAMP_LEFT`, `LAMP_RIGHT` or 0): before a turn at a
/// junction or into its driveway, while waiting for or making a lane change, and pulling
/// out of a bus bay. Reads the simulation (called when capturing a snapshot).
pub fn indicator_for(sim: &Simulation, vehicle: &Vehicle) -> u8 {
    if vehicle.in_bay {
        return if vehicle.leaving_bay_since != f64::INFINITY {
            LAMP_LEFT
        } else {
            0
        };
    }
    // Still sliding across into a new lane (offset: how far right of it the vehicle is).
    if vehicle.lateral_offset > 0.5 {
        return LAMP_LEFT;
    }
    if vehicle.lateral_offset < -0.5 {
        return LAMP_RIGHT;
    }
    let Some(lane) = vehicle.lane.and_then(|id| sim.roads.lane_any(id)) else {
        return 0;
    };
    if lane.is_connector() {
        return turn_lamp(lane.turn);
    }
    let Some(next) = vehicle
        .next_lanes
        .first()
        .and_then(|&id| sim.roads.lane_any(id))
    else {
        if vehicle.destination_lot_id.is_none()
            || vehicle.end_distance - vehicle.distance > SIGNAL_DISTANCE
        {
            return 0;
        }
        // Into the driveway: across the road if it is on the far side.
        return if vehicle.arrival_checked && !vehicle.crossing_lanes.is_empty() {
            LAMP_LEFT
        } else {
            LAMP_RIGHT
        };
    };
    if lane.is_beside(next) {
        // Looking for a gap to change lanes, wherever along the lane.
        let (origin, heading) = lane.sample_pose(vehicle.distance);
        let (target, _) = next.sample_pose(vehicle.distance.min(next.length));
        let toward = target - origin;
        let along = Vec2::from_angle(heading);
        return if along.perp_dot(toward) > 0.0 {
            LAMP_RIGHT
        } else {
            LAMP_LEFT
        };
    }
    if next.is_connector() && lane.length - vehicle.distance <= SIGNAL_DISTANCE {
        return turn_lamp(next.turn);
    }
    0
}

fn turn_lamp(turn: Turn) -> u8 {
    match turn {
        Turn::Right => LAMP_RIGHT,
        Turn::Left | Turn::UTurn => LAMP_LEFT,
        _ => 0,
    }
}
