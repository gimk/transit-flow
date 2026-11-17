//! Signal plans (pivot plan §4, "Signals"): a junction's lights as a fixed cycle of
//! phases, each giving green to some approaches (the signal groups), then amber, then an
//! all-red clearance. The light of an approach is a pure function of the run time, so
//! there is no signal state to save or to keep in step. A pedestrian light shows walk
//! while the traffic that drives over its crossing has red for long enough (`walk`,
//! `until_conflict`). Actuation (minimum green, extension,
//! skipping empty phases), push buttons and bicycle lamps come later.

use serde::{Deserialize, Serialize};

pub const DEFAULT_GREEN_S: f32 = 20.0;
pub const DEFAULT_AMBER_S: f32 = 3.0;
pub const DEFAULT_ALL_RED_S: f32 = 2.0;
/// A pedestrian light stops showing walk this long before the traffic that drives over
/// its crossing gets green (start value). Walkers also check their own crossing time
/// against `until_conflict` before they step off.
pub const PEDESTRIAN_CLEARANCE_S: f64 = 5.0;

/// The shortest and longest green the player may set (seconds).
pub const MIN_GREEN_S: f32 = 10.0;
pub const MAX_GREEN_S: f32 = 60.0;

/// Groups approaches into phases: each approach with the later ones that head against
/// it (`heading` dot < -0.7). Shared by the default plan and the templates, so both
/// split a junction the same way.
pub fn pair_opposite_arms(arms: &[i32], heading: impl Fn(i32) -> glam::Vec2) -> Vec<Vec<i32>> {
    let mut left: Vec<i32> = arms.to_vec();
    let mut groups = Vec::new();
    while !left.is_empty() {
        let first = left.remove(0);
        let mut green = vec![first];
        left.retain(|other| {
            let opposite = heading(first).dot(heading(*other)) < -0.7;
            if opposite {
                green.push(*other);
            }
            !opposite
        });
        groups.push(green);
    }
    groups
}

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

impl SignalPlan {
    /// Seconds from `time_s` until one of the `conflicting` approaches gets green: 0
    /// while one has green or amber, infinite when none ever does.
    pub fn until_conflict(&self, conflicting: &[i32], time_s: f64) -> f64 {
        let cycle = self.cycle_s();
        if cycle <= 0.0 {
            return f64::INFINITY;
        }
        if conflicting
            .iter()
            .any(|a| matches!(self.light(*a, time_s), Light::Green(_) | Light::Amber(_)))
        {
            return 0.0;
        }
        let t = time_s.rem_euclid(cycle);
        let mut starts = Vec::with_capacity(self.phases.len());
        let mut start = 0.0;
        for phase in &self.phases {
            starts.push(start);
            start += self.phase_s(phase);
        }
        let n = self.phases.len();
        for k in 0..2 * n {
            let at = starts[k % n] + (k / n) as f64 * cycle;
            if at >= t
                && self.phases[k % n]
                    .green
                    .iter()
                    .any(|a| conflicting.contains(a))
            {
                return at - t;
            }
        }
        f64::INFINITY
    }

    /// The pedestrian light of a crossing that the traffic of the `conflicting`
    /// approaches drives over: walk while that traffic will not get green for more than
    /// `PEDESTRIAN_CLEARANCE_S`. A short green elsewhere does not hide the walk light:
    /// it depends on the time left, not on the phase.
    pub fn walk(&self, conflicting: &[i32], time_s: f64) -> bool {
        self.until_conflict(conflicting, time_s) > PEDESTRIAN_CLEARANCE_S
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
    fn walk_shows_only_in_phases_that_leave_the_crossing_free() {
        let plan = plan();
        // A crossing that approaches 1 and 2 drive over (green 0 to 20 s, amber to 23 s):
        // walk from the end of their amber until 5 s before their next green (40 s).
        let conflicting = [1, 2];
        assert!(!plan.walk(&conflicting, 5.0));
        assert!(!plan.walk(&conflicting, 21.0), "their amber");
        assert_eq!(plan.until_conflict(&conflicting, 21.0), 0.0);
        assert!(plan.walk(&conflicting, 24.0), "the all-red after them");
        assert_eq!(plan.until_conflict(&conflicting, 24.0), 16.0);
        assert!(plan.walk(&conflicting, 34.9));
        assert!(
            !plan.walk(&conflicting, 35.1),
            "the clearance before their green"
        );
        // A crossing nobody drives over always walks.
        assert!(plan.walk(&[9], 21.0));
        // A short green elsewhere (4 s, less than the clearance) still shows walk: the
        // time left counts its amber and all-red too.
        let short = SignalPlan {
            phases: vec![
                SignalPhase {
                    green: vec![1],
                    green_s: 20.0,
                },
                SignalPhase {
                    green: vec![3],
                    green_s: 4.0,
                },
            ],
            amber_s: 3.0,
            all_red_s: 2.0,
        };
        assert!(short.walk(&[1], 25.5));
        assert_eq!(short.until_conflict(&[1], 25.0), 9.0);
        for tenth in 0..400 {
            let t = f64::from(tenth) / 10.0;
            if plan.walk(&conflicting, t) {
                for approach in conflicting {
                    assert!(matches!(plan.light(approach, t), Light::Red(_)), "{t}");
                }
            }
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
