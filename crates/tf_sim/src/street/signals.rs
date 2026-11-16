//! Signal plans (pivot plan §4, "Signals"): a junction's lights as a fixed cycle of
//! phases, each giving green to some approaches (the signal groups), then amber, then an
//! all-red clearance. The light of an approach is a pure function of the run time, so
//! there is no signal state to save or to keep in step. Actuation (minimum green,
//! extension, skipping empty phases) and pedestrian and bicycle lamps come later.

use serde::{Deserialize, Serialize};

pub const DEFAULT_GREEN_S: f32 = 20.0;
pub const DEFAULT_AMBER_S: f32 = 3.0;
pub const DEFAULT_ALL_RED_S: f32 = 2.0;

fn default_amber() -> f32 {
    DEFAULT_AMBER_S
}

fn default_all_red() -> f32 {
    DEFAULT_ALL_RED_S
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalPhase {
    /// The approach segments that have green in this phase.
    pub green: Vec<i32>,
    pub green_s: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalPlan {
    pub phases: Vec<SignalPhase>,
    #[serde(default = "default_amber")]
    pub amber_s: f32,
    #[serde(default = "default_all_red")]
    pub all_red_s: f32,
}

/// What an approach shows, with the seconds it has shown it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Light {
    Green(f64),
    Amber(f64),
    Red(f64),
}

impl SignalPlan {
    fn phase_s(&self, phase: &SignalPhase) -> f64 {
        f64::from(phase.green_s) + f64::from(self.amber_s) + f64::from(self.all_red_s)
    }

    pub fn cycle_s(&self) -> f64 {
        self.phases.iter().map(|phase| self.phase_s(phase)).sum()
    }

    /// The light of `approach` (a segment id) at `time_s` since the start of the run.
    /// An approach in no phase is always red.
    pub fn light(&self, approach: i32, time_s: f64) -> Light {
        let cycle = self.cycle_s();
        if cycle <= 0.0 {
            return Light::Red(time_s);
        }
        let t = time_s.rem_euclid(cycle);
        // The shortest time since one of its ambers ended, going back around the cycle.
        let mut red_for = f64::INFINITY;
        let mut start = 0.0;
        for phase in &self.phases {
            let green_end = start + f64::from(phase.green_s);
            let amber_end = green_end + f64::from(self.amber_s);
            if phase.green.contains(&approach) {
                if t >= start && t < green_end {
                    return Light::Green(t - start);
                }
                if t >= green_end && t < amber_end {
                    return Light::Amber(t - green_end);
                }
                red_for = red_for.min((t - amber_end).rem_euclid(cycle));
            }
            start += self.phase_s(phase);
        }
        if red_for.is_finite() {
            Light::Red(red_for)
        } else {
            Light::Red(time_s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> SignalPlan {
        SignalPlan {
            phases: vec![
                SignalPhase {
                    green: vec![1, 2],
                    green_s: 20.0,
                },
                SignalPhase {
                    green: vec![3],
                    green_s: 10.0,
                },
            ],
            amber_s: 3.0,
            all_red_s: 2.0,
        }
    }

    #[test]
    fn an_approach_goes_green_amber_red_over_the_cycle() {
        let plan = plan();
        assert_eq!(plan.cycle_s(), 40.0);
        assert_eq!(plan.light(1, 5.0), Light::Green(5.0));
        assert_eq!(plan.light(1, 21.0), Light::Amber(1.0));
        assert_eq!(plan.light(1, 24.0), Light::Red(1.0));
        assert_eq!(
            plan.light(3, 5.0),
            Light::Red(7.0),
            "red since 38 s of the last cycle"
        );
        assert_eq!(plan.light(3, 26.0), Light::Green(1.0));
        assert_eq!(plan.light(1, 45.0), Light::Green(5.0), "the cycle repeats");
    }

    #[test]
    fn never_two_conflicting_greens() {
        let plan = plan();
        for tick in 0..800 {
            let t = f64::from(tick) * 0.05;
            let main = !matches!(plan.light(1, t), Light::Red(_));
            let side = !matches!(plan.light(3, t), Light::Red(_));
            assert!(!(main && side), "both open at {t}");
        }
        assert!(matches!(plan.light(9, 3.0), Light::Red(_)));
    }
}
