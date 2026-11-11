//! `FlowMetric` (`sim/city/flow_metric.gd`): the player's score, how smoothly the city's
//! trips flow. A finished trip scores its ideal (free-flow) time divided by its actual
//! time, capped at 1; a failed or abandoned trip scores 0. Flow % is the average over
//! trips that ended within the last game day. Citizens' trips are scored door to door,
//! all legs together (walking to the car, the drive, walking from where it was parked);
//! freight is scored per vehicle. Debug stress traffic doesn't count. Also keeps the
//! share of each mode among citizens' trips.
//!
//! The GDScript class connects to the vehicle and citizen signals in its `_init`; here
//! `Simulation::dispatch` calls the `on_*` methods, after the other listeners.

use std::collections::VecDeque;

use indexmap::IndexMap;

use super::citizen::{Citizen, Mode};
use crate::clock::SimClock;
use crate::traffic::stress_traffic::OWNER_ID as STRESS_OWNER_ID;
use crate::traffic::vehicle::Vehicle;

#[derive(Clone, Debug, Default)]
pub struct FlowMetric {
    /// (end time, score) for each counted trip in the window, oldest first. Godot keeps
    /// them in `Vector2`s, so both are `f32` (and `_total` loses the rounding when a
    /// record is forgotten).
    records: VecDeque<(f32, f32)>,
    total: f64,
    /// (end time in whole sim seconds, `Citizen.Mode`) for each citizen trip in the
    /// window, oldest first.
    modes: VecDeque<(i32, Mode)>,
}

impl FlowMetric {
    /// One game day, in sim seconds.
    pub const WINDOW: f64 =
        SimClock::MINUTES_PER_DAY as f64 / SimClock::GAME_MINUTES_PER_SIM_SECOND;

    pub fn new() -> Self {
        Self::default()
    }

    /// Flow over the last game day, 0-100, or -1 when no trip has ended yet.
    pub fn flow_percent(&mut self, now: f64) -> f64 {
        self.forget_before(now - Self::WINDOW);
        if self.records.is_empty() {
            return -1.0;
        }
        100.0 * self.total / self.records.len() as f64
    }

    pub fn trip_count(&mut self, now: f64) -> usize {
        self.forget_before(now - Self::WINDOW);
        self.records.len()
    }

    /// `Citizen.Mode` → share (0-1) of citizens' trips over the last game day (empty if
    /// none), in the order the modes first appear.
    pub fn mode_shares(&mut self, now: f64) -> IndexMap<Mode, f64> {
        self.forget_before(now - Self::WINDOW);
        let mut shares = IndexMap::new();
        for &(_, mode) in &self.modes {
            *shares.entry(mode).or_insert(0.0) += 1.0 / self.modes.len() as f64;
        }
        shares
    }

    pub fn score(vehicle: &Vehicle, arrived: bool) -> f64 {
        if !arrived {
            return 0.0;
        }
        Self::trip_score(
            vehicle.trip_start_time,
            vehicle.ideal_trip_time,
            vehicle.trip_end_time,
        )
    }

    fn trip_score(start: f64, ideal: f64, end: f64) -> f64 {
        let actual = end - start;
        if actual <= 0.0 {
            1.0
        } else {
            (ideal / actual).clamp(0.0, 1.0)
        }
    }

    fn record(&mut self, vehicle: &Vehicle, arrived: bool) {
        if vehicle.owner_id == STRESS_OWNER_ID || vehicle.scored_by_owner {
            return;
        }
        self.add(vehicle.trip_end_time, Self::score(vehicle, arrived));
    }

    fn add(&mut self, end_time: f64, value: f64) {
        self.records.push_back((end_time as f32, value as f32));
        self.total += value;
    }

    fn forget_before(&mut self, time: f64) {
        while let Some(&(end, score)) = self.records.front() {
            if f64::from(end) >= time {
                break;
            }
            self.total -= f64::from(score);
            self.records.pop_front();
        }
        while self
            .modes
            .front()
            .is_some_and(|&(end, _)| f64::from(end) < time)
        {
            self.modes.pop_front();
        }
    }

    /// `VehicleSystem.vehicle_arrived`.
    pub fn on_vehicle_arrived(&mut self, vehicle: &Vehicle) {
        self.record(vehicle, true);
    }

    /// `VehicleSystem.vehicle_failed`.
    pub fn on_vehicle_failed(&mut self, vehicle: &Vehicle) {
        self.record(vehicle, false);
    }

    /// `CitizenSystem.trip_ended`.
    pub fn on_trip_ended(&mut self, citizen: &Citizen, completed: bool) {
        let value = if completed {
            Self::trip_score(
                citizen.trip_start_time,
                citizen.trip_ideal_time,
                citizen.trip_end_time,
            )
        } else {
            0.0
        };
        self.add(citizen.trip_end_time, value);
        // Whole sim seconds are plenty to forget old records by.
        self.modes
            .push_back((citizen.trip_end_time.floor() as i32, citizen.trip_mode));
    }
}
