//! `SimClock` (`sim/sim_clock.gd`): in-game time of day. Advanced only by the
//! simulation tick, never by frame delta.

use crate::events::{Outbox, SimEvent};

/// The four traffic periods of a day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Period {
    MorningRush,
    MiddayLogistics,
    EveningRush,
    Night,
}

impl Period {
    /// `PERIOD_NAMES`: what the HUD shows.
    pub fn name(self) -> &'static str {
        match self {
            Period::MorningRush => "Morning Rush",
            Period::MiddayLogistics => "Midday Logistics",
            Period::EveningRush => "Evening Rush",
            Period::Night => "Night",
        }
    }

    /// `SimClock.period_for_hour(hour)`.
    pub fn for_hour(hour: i32) -> Period {
        if (6..10).contains(&hour) {
            return Period::MorningRush;
        }
        if (10..16).contains(&hour) {
            return Period::MiddayLogistics;
        }
        if (16..20).contains(&hour) {
            return Period::EveningRush;
        }
        Period::Night
    }
}

#[derive(Clone, Debug)]
pub struct SimClock {
    /// Game minutes elapsed since midnight of day 1.
    pub total_minutes: f64,
}

impl Default for SimClock {
    fn default() -> Self {
        Self {
            total_minutes: f64::from(Self::START_HOUR) * 60.0,
        }
    }
}

impl SimClock {
    /// A full day lasts MINUTES_PER_DAY / GAME_MINUTES_PER_SIM_SECOND = 288 s at 1x speed.
    pub const GAME_MINUTES_PER_SIM_SECOND: f64 = 5.0;
    pub const MINUTES_PER_DAY: i32 = 24 * 60;
    pub const START_HOUR: i32 = 6;

    pub fn new() -> Self {
        Self::default()
    }

    /// Emits `DayStarted` then `HourChanged` for every hour crossed, in order, like
    /// the GDScript signals.
    pub fn advance(&mut self, sim_seconds: f64, out: &mut Outbox) {
        let previous_hours = (self.total_minutes / 60.0) as i32;
        self.total_minutes += sim_seconds * Self::GAME_MINUTES_PER_SIM_SECOND;
        let current_hours = (self.total_minutes / 60.0) as i32;
        for absolute_hour in previous_hours + 1..current_hours + 1 {
            let hour = absolute_hour % 24;
            if hour == 0 {
                out.push(SimEvent::DayStarted(absolute_hour / 24 + 1));
            }
            out.push(SimEvent::HourChanged(hour));
        }
    }

    pub fn day(&self) -> i32 {
        (self.total_minutes / f64::from(Self::MINUTES_PER_DAY)) as i32 + 1
    }

    pub fn minute_of_day(&self) -> f64 {
        self.total_minutes % f64::from(Self::MINUTES_PER_DAY)
    }

    pub fn hour(&self) -> i32 {
        (self.minute_of_day() / 60.0) as i32
    }

    pub fn minute(&self) -> i32 {
        self.minute_of_day() as i32 % 60
    }

    /// Time of day as fractional hours (0.0 – 24.0), handy for sampling demand curves.
    pub fn time_of_day_hours(&self) -> f64 {
        self.minute_of_day() / 60.0
    }

    pub fn format_time(&self) -> String {
        format!("{:02}:{:02}", self.hour(), self.minute())
    }

    pub fn period(&self) -> Period {
        Period::for_hour(self.hour())
    }

    pub fn period_name(&self) -> &'static str {
        self.period().name()
    }
}
