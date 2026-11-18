//! What the run reports about danger (pivot plan §5.5): near misses (post-encroachment
//! time in the conflict zones of two movements, hard braking), contacts between bodies,
//! and the chain of causes that led to them.

use glam::Vec2;

use super::agent::Agent;
use super::detect::{Body, swept_overlap};
use super::drive::{DT, brake_thresholds};
use super::events::{
    Cause, CauseKind, ConflictKind, Grade, Outcome, PEDESTRIAN_SEVERITY, StreetEvent,
    VEHICLE_SEVERITY, draw_outcome, rounded, rounded_point,
};
use super::{
    CONFLICT_COOLDOWN_S, CRASH_BLOCK_S, EndReason, PET_LIGHT_S, PET_SERIOUS_S, StreetRun, mix,
};
use crate::rng::SimRng;
use crate::street::ids::{AgentId, StreetLaneId};
use crate::street::level::Mode;

/// Speeding is told when an agent drives this much faster than the limit.
pub(super) const SPEEDING_FACTOR: f64 = 1.15;
/// "Could not stop" is blamed on agents who braked hard this recently.
const HARD_BRAKE_MEMORY_S: f64 = 3.0;
/// Bodies further apart than the sum of their half lengths and this are not tested.
const CONTACT_REACH: f32 = 2.0;

pub(super) type ZoneExits = indexmap::IndexMap<(StreetLaneId, StreetLaneId), (AgentId, u64)>;

/// Remembers when an agent leaves the conflict zones it is in (it leaves the junction or
/// the run), so a later arrival can be measured against it.
pub(super) fn flush_zones(agent: &mut Agent, exits: &mut ZoneExits, tick: u64) {
    if let Some(approach) = &agent.approach {
        for c in agent.in_zones.drain(..) {
            exits.insert((approach.movement, c), (agent.id, tick));
        }
    } else {
        agent.in_zones.clear();
    }
}

fn body(pos: Vec2, dir: Vec2, agent: &Agent) -> Body {
    Body {
        center: pos,
        dir,
        half_length: (agent.length * 0.5) as f32,
        half_width: (agent.width * 0.5) as f32,
    }
}

impl StreetRun {
    /// What each of the two did wrong that bears on the other, or on nobody in
    /// particular. An agent may be gone already (taken away, or arrived).
    pub(super) fn causes_between(&self, a: Option<&Agent>, b: Option<&Agent>) -> Vec<Cause> {
        let mut causes: Vec<Cause> = Vec::new();
        for (x, y) in [(a, b), (b, a)] {
            let Some(x) = x else {
                continue;
            };
            for (_, what) in &x.recent {
                let relevant = match what.about() {
                    None => true,
                    Some(about) => y.is_some_and(|y| y.id == about),
                };
                if relevant {
                    push_unique(
                        &mut causes,
                        Cause {
                            agent: x.id,
                            what: *what,
                        },
                    );
                }
            }
            let route = &self.routes[x.route];
            let limit = self.street.lanes[&route.lanes[route.lane_index(x.d)]].speed_limit;
            if x.speed > SPEEDING_FACTOR * limit {
                push_unique(
                    &mut causes,
                    Cause {
                        agent: x.id,
                        what: CauseKind::Speeding {
                            kmh: rounded(x.speed * 3.6, 10.0),
                            limit_kmh: rounded(limit * 3.6, 10.0),
                        },
                    },
                );
            }
        }
        if causes.is_empty() {
            for x in [a, b].into_iter().flatten() {
                if x.last_hard_brake
                    .is_some_and(|t| self.tick.saturating_sub(t) as f64 * DT <= HARD_BRAKE_MEMORY_S)
                {
                    push_unique(
                        &mut causes,
                        Cause {
                            agent: x.id,
                            what: CauseKind::TooFastToStop,
                        },
                    );
                }
            }
        }
        causes
    }

    /// Logs a conflict unless the same pair had one of the same kind a moment ago.
    pub(super) fn log_conflict(
        &mut self,
        kind: ConflictKind,
        grade: Grade,
        (a, b): (AgentId, AgentId),
        position: Vec2,
        measure: f64,
    ) {
        let key = (kind, a.min(b), a.max(b));
        if let Some(last) = self.conflict_last.get(&key)
            && (self.tick - last) as f64 * DT < CONFLICT_COOLDOWN_S
        {
            return;
        }
        self.conflict_last.insert(key, self.tick);
        let mut causes = self.causes_between(self.agents.get(&a), self.agents.get(&b));
        for cause in self
            .walker_causes(a)
            .into_iter()
            .chain(self.walker_causes(b))
        {
            push_unique(&mut causes, cause);
        }
        self.log(StreetEvent::Conflict {
            kind,
            grade,
            a,
            b,
            position: rounded_point(position),
            measure: rounded(measure, 100.0),
            causes,
        });
    }

    /// Stage 6: post-encroachment time. An agent entering the conflict zone of its
    /// movement with another one meets either the other's body (time 0) or the moment
    /// the other left it.
    pub(super) fn zone_stage(&mut self) {
        let tick = self.tick;
        let ids: Vec<AgentId> = self.agents.keys().copied().collect();
        for id in ids {
            let agent = &self.agents[&id];
            let Some(approach) = &agent.approach else {
                continue;
            };
            let movement = approach.movement;
            let front = agent.d - approach.line_d;
            let rear = front - agent.length;
            let out_lane = self.moves[&movement].out_lane;
            let mut inside = Vec::new();
            for (c, zone) in &self.street.lanes[&movement].conflict_zones {
                if self.moves[c].out_lane == out_lane {
                    continue; // a merge is the following rules' business, not a crossing
                }
                if front >= f64::from(zone.x) && rear <= f64::from(zone.y) {
                    inside.push(*c);
                }
            }
            let before = agent.in_zones.clone();
            let position = agent.pos;
            let mut found: Vec<(AgentId, f64)> = Vec::new();
            for c in inside.iter().filter(|c| !before.contains(c)) {
                let occupant = self.agents.values().find(|o| {
                    o.id != id
                        && o.approach.as_ref().is_some_and(|a| a.movement == *c)
                        && o.in_zones.contains(&movement)
                });
                if let Some(occupant) = occupant {
                    found.push((occupant.id, 0.0));
                } else if let Some(&(who, left)) = self.zone_exits.get(&(*c, movement))
                    && who != id
                {
                    let pet = (tick - left) as f64 * DT;
                    if pet < PET_LIGHT_S {
                        found.push((who, pet));
                    }
                }
            }
            for left in before.iter().filter(|c| !inside.contains(c)) {
                self.zone_exits.insert((movement, *left), (id, tick));
            }
            self.agents[&id].in_zones = inside;
            for (who, pet) in found {
                let grade = if pet < PET_SERIOUS_S {
                    Grade::Serious
                } else {
                    Grade::Light
                };
                self.log_conflict(ConflictKind::Pet, grade, (who, id), position, pet);
            }
        }
    }

    /// Stage 7: bodies that touch. Both stop where they are and block the street until
    /// they are taken away.
    pub(super) fn contact_stage(&mut self) {
        let tick = self.tick;
        let ids: Vec<AgentId> = self.agents.keys().copied().collect();
        for i in 0..ids.len() {
            for j in i + 1..ids.len() {
                let (a, b) = (&self.agents[&ids[i]], &self.agents[&ids[j]]);
                if (a.crashed() && b.crashed()) || self.touching.contains(&(a.id, b.id)) {
                    continue;
                }
                let reach = ((a.length + b.length) * 0.5) as f32 + CONTACT_REACH;
                if a.pos.distance(b.pos) > reach {
                    continue;
                }
                if !swept_overlap(
                    &body(a.prev_pos, a.prev_dir, a),
                    &body(a.pos, a.dir, a),
                    &body(b.prev_pos, b.prev_dir, b),
                    &body(b.pos, b.dir, b),
                ) {
                    continue;
                }
                let impact_kmh = f64::from((a.velocity() - b.velocity()).length()) * 3.6;
                let position = (a.pos + b.pos) * 0.5;
                let mut causes = self.causes_between(Some(a), Some(b));
                // Only for contacts with a cyclist: the vehicle-only logs stay as they were.
                if causes.is_empty() && (a.mode == Mode::Bicycle || b.mode == Mode::Bicycle) {
                    // No hard brake and no rule broken (one that follows too close and brakes
                    // late and gently): the faster one could not stop in time.
                    let striker = if b.speed > a.speed { b } else { a };
                    causes.push(Cause {
                        agent: striker.id,
                        what: CauseKind::TooFastToStop,
                    });
                }
                let seed = mix(mix(self.seed, ids[i].0 as u64), mix(ids[j].0 as u64, tick));
                // A cyclist is as exposed as a person on foot, against a car or another cyclist.
                let table = if a.mode == Mode::Bicycle || b.mode == Mode::Bicycle {
                    PEDESTRIAN_SEVERITY
                } else {
                    VEHICLE_SEVERITY
                };
                let outcome = draw_outcome(table, impact_kmh, &mut SimRng::new(seed));
                self.log(StreetEvent::Contact {
                    a: ids[i],
                    b: ids[j],
                    position: rounded_point(position),
                    impact_kmh: rounded(impact_kmh, 10.0),
                    outcome,
                    causes,
                });
                self.touching.push((ids[i], ids[j]));
                let until = tick + (CRASH_BLOCK_S / DT).round() as u64;
                for id in [ids[i], ids[j]] {
                    let agent = &mut self.agents[&id];
                    agent.speed = 0.0;
                    agent.accel = 0.0;
                    agent.crashed_until.get_or_insert(until);
                }
                if outcome == Outcome::Death {
                    self.ended = Some(EndReason::Death);
                }
            }
        }
    }

    /// The grade of a hard brake.
    pub(super) fn brake_grade(mode: Mode, rate: f64) -> Grade {
        if rate >= brake_thresholds(mode).1 {
            Grade::Serious
        } else {
            Grade::Light
        }
    }
}

pub(super) fn push_unique(causes: &mut Vec<Cause>, cause: Cause) {
    if !causes.contains(&cause) {
        causes.push(cause);
    }
}
