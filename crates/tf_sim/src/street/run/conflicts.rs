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
use super::perceive;
use super::{
    CONFLICT_COOLDOWN_S, CRASH_BLOCK_S, EndReason, PET_LIGHT_S, PET_SERIOUS_S, StreetRun, mix,
};
use crate::rng::SimRng;
use crate::street::ids::{AgentId, StreetLaneId};
use crate::street::level::Mode;

/// Speeding is told when an agent drives this much faster than the limit. A rider may ride
/// up to this factor over a low limit (`drive::BIKE_LIMIT_TOLERANCE` is this constant), so
/// a rider at its allowed top speed is not speeding.
pub(super) const SPEEDING_FACTOR: f64 = 1.15;
/// "Could not stop" is blamed on agents who braked hard this recently.
const HARD_BRAKE_MEMORY_S: f64 = 3.0;
/// A pass of a rider is a conflict when the side room between the bodies is under this (metres,
/// the French rule in town, R414-4) and the vehicle is faster than `CLOSE_PASS_MIN_SPEED` (m/s,
/// start value: below a walking-to-jogging pace nobody is at risk); serious under
/// `CLOSE_PASS_SERIOUS_M` and over `CLOSE_PASS_SERIOUS_SPEED` (start values).
const CLOSE_PASS_LIGHT_M: f64 = 1.0;
const CLOSE_PASS_MIN_SPEED: f64 = 5.0;
const CLOSE_PASS_SERIOUS_M: f64 = 0.5;
const CLOSE_PASS_SERIOUS_SPEED: f64 = 10.0;
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

    /// What perception adds to the causes of a contact of `driver` with `rider`.
    fn perception_cause_of_rider(&self, driver: &Agent, rider: &Agent, causes: &mut Vec<Cause>) {
        let eye = super::yielding::eye(driver);
        let to = rider.pos - eye;
        if perceive::in_blind_spot(driver.mode, driver.dir, to) {
            push_unique(
                causes,
                Cause {
                    agent: driver.id,
                    what: CauseKind::InBlindSpot { walker: rider.id },
                },
            );
            return;
        }
        let looker = perceive::Looker::for_agent(driver, eye, driver.dir);
        let place = |_: perceive::Focus, _: u64| Vec2::ZERO;
        if looker
            .notices(rider.id.0 as u64, rider.pos, self.tick, place)
            .is_none()
        {
            let index = perceive::glance_index(driver.gaze_seed, self.tick);
            push_unique(
                causes,
                Cause {
                    agent: driver.id,
                    what: CauseKind::LookedElsewhere {
                        at: rider.id,
                        focus: looker.gaze(index, place).1,
                    },
                },
            );
        }
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

    /// Stage 6b: a motor vehicle that passes a rider with little room at speed. The rule is the
    /// French one of 1 m in town (R414-4); a pass is a vehicle going the same way, faster
    /// than the rider, level with it. Light under `CLOSE_PASS_LIGHT_M` at over
    /// `CLOSE_PASS_MIN_SPEED`, serious under `CLOSE_PASS_SERIOUS_M` at over
    /// `CLOSE_PASS_SERIOUS_SPEED` (user, 2026-11-18).
    pub(super) fn close_pass_stage(&mut self) {
        let mut found: Vec<(AgentId, AgentId, Vec2, f64, Grade)> = Vec::new();
        let mut found_oncoming: Vec<(AgentId, AgentId, Vec2, f64, Grade)> = Vec::new();
        let lane_of = |a: &Agent| {
            let route = &self.routes[a.route];
            route.lanes[route.lane_index(a.d)]
        };
        for rider in self.agents.values().filter(|a| a.mode == Mode::Bicycle) {
            let rider_lane = lane_of(rider);
            for car in self.agents.values() {
                if !matches!(car.mode, Mode::Car | Mode::Van | Mode::Bus)
                    || car.speed <= CLOSE_PASS_MIN_SPEED
                    || car.speed <= rider.speed + 1.0
                    || car.dir.dot(rider.dir) < 0.9
                {
                    continue;
                }
                // An overtake in the lane the rider is in. A car that keeps to its own lane
                // beside a bicycle lane or a track does not overtake the rider: the room
                // between them is the width the design gave the two strips, not the room the
                // driver left (Phase 6 W4b).
                if lane_of(car) != rider_lane {
                    continue;
                }
                let to = rider.pos - car.pos;
                let along = f64::from(to.dot(car.dir)).abs();
                if along > (car.length + rider.length) * 0.5 + 0.5 {
                    continue;
                }
                let room =
                    f64::from(to.dot(car.dir.perp()).abs()) - (car.width + rider.width) * 0.5;
                if room >= CLOSE_PASS_LIGHT_M {
                    continue;
                }
                let grade = if room < CLOSE_PASS_SERIOUS_M && car.speed > CLOSE_PASS_SERIOUS_SPEED {
                    Grade::Serious
                } else {
                    Grade::Light
                };
                found.push((car.id, rider.id, rider.pos, room.max(0.0), grade));
            }
        }
        // A tight pass against oncoming traffic: a driver that squeezes past a rider pulls into
        // the lane of the other way and meets a vehicle there with little room (Phase 6 W4b).
        // The one that is off its own side position is `a`.
        let ids: Vec<AgentId> = self.agents.keys().copied().collect();
        for (i, first) in ids.iter().enumerate() {
            let one = &self.agents[first];
            if !matches!(one.mode, Mode::Car | Mode::Van | Mode::Bus)
                || one.speed <= CLOSE_PASS_MIN_SPEED
            {
                continue;
            }
            for second in &ids[i + 1..] {
                let other = &self.agents[second];
                if !matches!(other.mode, Mode::Car | Mode::Van | Mode::Bus)
                    || other.speed <= CLOSE_PASS_MIN_SPEED
                    || one.dir.dot(other.dir) > -0.9
                {
                    continue;
                }
                let to = other.pos - one.pos;
                if f64::from(to.dot(one.dir)).abs() > (one.length + other.length) * 0.5 + 0.5 {
                    continue;
                }
                let room =
                    f64::from(to.dot(one.dir.perp()).abs()) - (one.width + other.width) * 0.5;
                if room >= CLOSE_PASS_LIGHT_M {
                    continue;
                }
                let grade = if room < CLOSE_PASS_SERIOUS_M
                    && one.speed + other.speed > 2.0 * CLOSE_PASS_SERIOUS_SPEED
                {
                    Grade::Serious
                } else {
                    Grade::Light
                };
                let (a, b) = if one.lateral.abs() >= other.lateral.abs() {
                    (one.id, other.id)
                } else {
                    (other.id, one.id)
                };
                found_oncoming.push((a, b, one.pos.midpoint(other.pos), room.max(0.0), grade));
            }
        }
        for (a, b, at, room, grade) in found_oncoming {
            self.log_conflict(ConflictKind::ClosePass, grade, (a, b), at, room);
        }
        for (car, rider, at, room, grade) in found {
            self.log_conflict(ConflictKind::ClosePass, grade, (car, rider), at, room);
        }
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
                // A contact between a motor vehicle and a rider: did the driver look at it, or
                // was it in a blind spot (the right turn against a rider going straight)?
                if let Some((driver, rider)) = match (a.mode, b.mode) {
                    (Mode::Bicycle, Mode::Bicycle) => None,
                    (Mode::Bicycle, _) => Some((b, a)),
                    (_, Mode::Bicycle) => Some((a, b)),
                    _ => None,
                } {
                    self.perception_cause_of_rider(driver, rider, &mut causes);
                }
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
