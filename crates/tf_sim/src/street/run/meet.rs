//! Vehicles and people on foot meeting (pivot plan §5.5, Phase 3 stage C): contacts,
//! near misses and the causes behind them.
//!
//! - **Contact.** A walker's circle, swept from where it was to where it is, against a
//!   vehicle's rectangle, swept the same way, at the moments of `detect::SWEEP`. The
//!   outcome is drawn from `PEDESTRIAN_SEVERITY` at the impact speed (the closing speed of
//!   the two) with a stream seeded from the run seed, the two ids and the tick, as for
//!   two vehicles. A vehicle standing (slower than `crowd::STANDING_VEHICLE`) is solid for
//!   walkers instead: they are kept out of it, and brushing it is no contact. Both stop: the vehicle as after a vehicle contact, the walker where it
//!   fell (`Walker::hit_until`), and both are taken away after `CRASH_BLOCK_S`. A death
//!   ends the run.
//! - **Causes** come from both sides: what the driver did lately (distracted, ran the
//!   red, did not yield at the zebra, did not look, too fast for a hidden kerb, too fast),
//!   whether the walker was hidden from it in the snapshot it decided from (and by what),
//!   and what the walker did (crossed out of a crossing, on red, in a short gap).
//! - **Near misses.** Post-encroachment time in the band a vehicle route cuts through a
//!   road area: the band is the part of the road area within half a car width and a body
//!   of the route's line, cut in cells of `BAND_CELL_M` along the route (a PET conflict
//!   means the two nearly used the same spot, not the same carriageway piece). A standing
//!   vehicle is no conflict. A walker entering the band this soon after a vehicle left it,
//!   or a vehicle after a walker, is a conflict (`PET_LIGHT_S`, `PET_SERIOUS_S`), and so
//!   is the two in the band at once (time 0). Braking hard for a walker is logged like
//!   braking for a vehicle.

use glam::Vec2;
use indexmap::IndexMap;

use super::agent::Agent;
use super::conflicts::push_unique;
use super::crowd::STANDING_VEHICLE;
use super::detect::{Body, SWEEP};
use super::drive::DT;
use super::events::{
    Cause, CauseKind, ConflictKind, Grade, Outcome, PEDESTRIAN_SEVERITY, StreetEvent, draw_outcome,
    rounded, rounded_point,
};
use super::perceive;
use super::walker::Walker;
use super::yielding::project;
use super::{CRASH_BLOCK_S, EndReason, PET_LIGHT_S, PET_SERIOUS_S, StreetRun, mix};
use crate::rng::SimRng;
use crate::street::ids::AgentId;

/// A vehicle and a walker further apart than this (beyond half the vehicle's length)
/// are not tested, metres.
const MEET_REACH: f32 = 2.0;
/// A walker who could not be seen this long before a meeting (1 s) was a surprise.
const SURPRISE_BEFORE_TICKS: usize = 20;
/// The band of a vehicle route through a road area: this far each side of its line,
/// beyond the walker's body, metres (half a car width).
pub(super) const BAND_HALF_WIDTH: f32 = 0.9;

/// Does a circle touch a rectangle body?
pub fn circle_touches(body: &Body, center: Vec2, radius: f32) -> bool {
    let local = center - body.center;
    let along = (local.dot(body.dir).abs() - body.half_length).max(0.0);
    let side = (local.dot(body.dir.perp()).abs() - body.half_width).max(0.0);
    along * along + side * side < radius * radius
}

fn body_of(agent: &Agent, pos: Vec2, dir: Vec2) -> Body {
    Body {
        center: pos,
        dir,
        half_length: (agent.length * 0.5) as f32,
        half_width: (agent.width * 0.5) as f32,
    }
}

/// Who is in each band now: per (road area, route), the vehicles and the walkers.
#[derive(Clone, Debug, Default)]
pub(super) struct Band {
    vehicles: Vec<AgentId>,
    walkers: Vec<AgentId>,
    vehicle_left: Option<(AgentId, u64)>,
    walker_left: Option<(AgentId, u64)>,
}

/// A band cell: (road area, route, cell along the route).
pub(super) type BandKey = (usize, usize, i32);
pub(super) type Bands = IndexMap<BandKey, Band>;

/// Band cells are this long along the route, metres (start value: about a car length, so
/// that two road users in one cell nearly used the same spot).
const BAND_CELL_M: f64 = 4.0;

impl StreetRun {
    /// What the driver of `car` and the walker did that bears on their meeting.
    pub(super) fn meeting_causes(&self, car: &Agent, walker: &Walker) -> Vec<Cause> {
        let mut causes = Vec::new();
        for (_, what) in &car.recent {
            if what.about().is_none_or(|about| about == walker.id) {
                push_unique(
                    &mut causes,
                    Cause {
                        agent: car.id,
                        what: *what,
                    },
                );
            }
        }
        if let Some(crossing) = &car.crossing
            && !crossing.looks
            && self.walk_paths[walker.path].areas[walker.area_index] == crossing.area
        {
            push_unique(
                &mut causes,
                Cause {
                    agent: car.id,
                    what: CauseKind::DidNotLook { at: walker.id },
                },
            );
        }
        // Hidden in what the driver saw?
        let back = ((car.traits.reaction_s / DT).round() as usize)
            .max(1)
            .min(self.history.len());
        let snapshot = &self.history[self.history.len() - back];
        let seen_car = snapshot.seen.iter().find(|s| s.id == car.id);
        let seen_walker = snapshot.walkers.iter().find(|w| w.id == walker.id);
        if let (Some(c), Some(w)) = (seen_car, seen_walker) {
            let from = c.pos + c.dir * (c.length as f32 * 0.5);
            if let Some(by) = self.sight.hider(from, w.pos, &snapshot.seen, Some(car.id)) {
                push_unique(
                    &mut causes,
                    Cause {
                        agent: car.id,
                        what: CauseKind::HiddenWalker {
                            walker: walker.id,
                            by,
                        },
                    },
                );
            }
        }
        self.perception_causes(car, walker, &mut causes);
        let route = &self.routes[car.route];
        let limit = self.street.lanes[&route.lanes[route.lane_index(car.d)]].speed_limit;
        if car.speed > super::conflicts::SPEEDING_FACTOR * limit {
            push_unique(
                &mut causes,
                Cause {
                    agent: car.id,
                    what: CauseKind::Speeding {
                        kmh: rounded(car.speed * 3.6, 10.0),
                        limit_kmh: rounded(limit * 3.6, 10.0),
                    },
                },
            );
        }
        for (_, what) in &walker.recent {
            push_unique(
                &mut causes,
                Cause {
                    agent: walker.id,
                    what: *what,
                },
            );
        }
        if causes.is_empty() {
            push_unique(
                &mut causes,
                Cause {
                    agent: car.id,
                    what: CauseKind::TooFastToStop,
                },
            );
        }
        causes
    }

    /// What perception adds to the cause chain of a meeting (`perceive`): the driver's gaze,
    /// blind spot and surprise, and what the walker saw and heard of the car.
    fn perception_causes(&self, car: &Agent, walker: &Walker, causes: &mut Vec<Cause>) {
        let back = ((car.traits.reaction_s / DT).round() as usize)
            .max(1)
            .min(self.history.len());
        let snapshot = &self.history[self.history.len() - back];
        let (Some(c), Some(w)) = (
            snapshot.seen.iter().find(|s| s.id == car.id),
            snapshot.walkers.iter().find(|w| w.id == walker.id),
        ) else {
            return;
        };
        let eye = c.pos + c.dir * (c.length as f32 * 0.5);
        let to = w.pos - eye;
        let hidden_from_car = self
            .sight
            .hider(eye, w.pos, &snapshot.seen, Some(car.id))
            .is_some();
        if !hidden_from_car {
            let looker = perceive::Looker {
                mode: car.mode,
                eye,
                dir: c.dir,
                table: perceive::driver_table(car.gaze_task),
                seed: car.gaze_seed,
                fov: perceive::useful_fov(car.traits.attention, car.traits.risk),
                alternate: false,
            };
            // A glance at a kerb or an exit is a glance at the place the walker is.
            let place = |_: perceive::Focus, _: u64| to;
            if perceive::in_blind_spot(car.mode, c.dir, to) {
                push_unique(
                    causes,
                    Cause {
                        agent: car.id,
                        what: CauseKind::InBlindSpot { walker: walker.id },
                    },
                );
            } else if looker
                .notices(walker.id.0 as u64, w.pos, self.tick, place)
                .is_none()
            {
                let index = perceive::glance_index(car.gaze_seed, self.tick);
                push_unique(
                    causes,
                    Cause {
                        agent: car.id,
                        what: CauseKind::LookedElsewhere {
                            at: walker.id,
                            focus: looker.gaze(index, place).1,
                        },
                    },
                );
            }
        }
        // A surprise: a second before, the driver could not have seen the walker.
        let before = self.history.len().saturating_sub(1 + SURPRISE_BEFORE_TICKS);
        if let Some(old) = self.history.get(before)
            && let Some(ow) = old.walkers.iter().find(|x| x.id == walker.id)
            && self
                .sight
                .hider(eye, ow.pos, &old.seen, Some(car.id))
                .is_some()
        {
            push_unique(
                causes,
                Cause {
                    agent: car.id,
                    what: CauseKind::Surprise {
                        reaction_s: rounded(
                            perceive::reaction_s(car.traits.reaction_s, true),
                            10.0,
                        ),
                    },
                },
            );
        }
        // What the walker saw and heard of the car.
        let nose = eye;
        let hidden = self
            .sight
            .hider(walker.pos, nose, &snapshot.seen, Some(car.id));
        let ahead = walker.pos + walker.vel;
        if !self.walker_notices(
            walker,
            c,
            nose,
            hidden.is_some(),
            snapshot,
            walker.pos,
            ahead,
        ) {
            if let Some(by) = hidden {
                push_unique(
                    causes,
                    Cause {
                        agent: walker.id,
                        what: CauseKind::HiddenVehicle {
                            vehicle: car.id,
                            by,
                        },
                    },
                );
            }
            push_unique(
                causes,
                Cause {
                    agent: walker.id,
                    what: CauseKind::NotHeard {
                        vehicle: car.id,
                        kmh: rounded(car.speed * 3.6, 1.0),
                        electric: car.electric,
                        deaf: walker.deaf,
                    },
                },
            );
        }
    }

    /// Contacts between vehicles and walkers (after the vehicle contacts of the tick).
    pub(super) fn walker_contact_stage(&mut self) {
        let tick = self.tick;
        let mut hits: Vec<(AgentId, AgentId, Vec2, f64)> = Vec::new();
        for walker in self.walkers.values() {
            if walker.hit() {
                continue;
            }
            for car in self.agents.values() {
                if self.touching.contains(&(car.id, walker.id)) {
                    continue;
                }
                // A standing vehicle is solid for walkers (they are pushed out of it, see
                // `crowd`): it makes a contact only if a walker is still inside it (it
                // stopped over one, or a wall kept one from getting out).
                if car.speed < STANDING_VEHICLE {
                    let body = body_of(car, car.pos, car.dir);
                    let local = walker.pos - body.center;
                    let inside = local.dot(body.dir).abs() < body.half_length
                        && local.dot(body.dir.perp()).abs() < body.half_width;
                    if inside {
                        hits.push((car.id, walker.id, walker.pos, 0.0));
                    }
                    continue;
                }
                let reach = (car.length * 0.5) as f32 + walker.radius + MEET_REACH;
                if car.pos.distance(walker.pos) > reach {
                    continue;
                }
                let (from, to) = (
                    body_of(car, car.prev_pos, car.prev_dir),
                    body_of(car, car.pos, car.dir),
                );
                let touched = SWEEP.into_iter().any(|t| {
                    circle_touches(
                        &Body::between(&from, &to, t),
                        walker.prev_pos.lerp(walker.pos, t),
                        walker.radius,
                    )
                });
                if touched {
                    let impact = f64::from((car.velocity() - walker.vel).length()) * 3.6;
                    hits.push((car.id, walker.id, (car.pos + walker.pos) * 0.5, impact));
                }
            }
        }
        let until = tick + (CRASH_BLOCK_S / DT).round() as u64;
        for (car_id, walker_id, position, impact) in hits {
            let causes = self.meeting_causes(&self.agents[&car_id], &self.walkers[&walker_id]);
            let seed = mix(
                mix(self.seed, car_id.0 as u64),
                mix(walker_id.0 as u64, tick),
            );
            let outcome = draw_outcome(PEDESTRIAN_SEVERITY, impact, &mut SimRng::new(seed));
            self.log(StreetEvent::Contact {
                a: car_id,
                b: walker_id,
                position: rounded_point(position),
                impact_kmh: rounded(impact, 10.0),
                outcome,
                causes,
            });
            self.touching.push((car_id, walker_id));
            let car = &mut self.agents[&car_id];
            car.speed = 0.0;
            car.accel = 0.0;
            car.crashed_until.get_or_insert(until);
            let walker = &mut self.walkers[&walker_id];
            walker.vel = Vec2::ZERO;
            walker.hit_until.get_or_insert(until);
            if outcome == Outcome::Death {
                self.ended = Some(EndReason::Death);
            }
        }
    }

    /// Near misses between vehicles and walkers: post-encroachment time (PET, Allen et al.
    /// 1978: the time between the first road user leaving a place and the second reaching
    /// it) in the bands. A PET conflict means the two nearly used the same spot, so a band is
    /// cut in cells of `BAND_CELL_M` along the route and only road users in the same cell
    /// count, not those in the same carriageway piece (which can be 160 m long). A
    /// vehicle standing (slower than `STANDING_VEHICLE`) is solid for walkers and waits for
    /// them: it is not a conflict. One event per walker and vehicle per
    /// `CONFLICT_COOLDOWN_S` (`log_conflict`), so a walker at the edge of the band does not
    /// repeat it.
    pub(super) fn walker_pet_stage(&mut self) {
        let tick = self.tick;
        // Who is in which cell of which band now. A moving vehicle occupies the cells its
        // body covers inside the road area; a walker the cell of its place along the route.
        let mut now: IndexMap<BandKey, (Vec<AgentId>, Vec<AgentId>)> = IndexMap::new();
        for car in self.agents.values() {
            if car.speed <= STANDING_VEHICLE {
                continue;
            }
            for area in &self.route_areas[car.route] {
                if car.d > area.enter && car.d - car.length < area.exit {
                    let (from, to) = ((car.d - car.length).max(area.enter), car.d.min(area.exit));
                    let first = (from / BAND_CELL_M).floor() as i32;
                    let last = (to / BAND_CELL_M).floor() as i32;
                    for cell in first..=last {
                        now.entry((area.area, car.route, cell))
                            .or_default()
                            .0
                            .push(car.id);
                    }
                }
            }
        }
        for walker in self.walkers.values() {
            let area = self.walk_paths[walker.path].areas[walker.area_index];
            if !self.walk_space.areas[area].kind.is_road() {
                continue;
            }
            for (route, areas) in self.route_areas.iter().enumerate() {
                let Some(route_area) = areas.iter().find(|a| a.area == area) else {
                    continue;
                };
                if let Some((at, beside)) = project(&route_area.line, walker.pos)
                    && beside < BAND_HALF_WIDTH + walker.radius
                {
                    let cell = (at / BAND_CELL_M).floor() as i32;
                    now.entry((area, route, cell))
                        .or_default()
                        .1
                        .push(walker.id);
                }
            }
        }
        let mut found: Vec<(AgentId, AgentId, f64, Vec2)> = Vec::new();
        let keys: Vec<BandKey> = self.bands.keys().chain(now.keys()).copied().collect();
        for key in keys {
            let (vehicles, walkers) = now.get(&key).cloned().unwrap_or_default();
            let band = self.bands.entry(key).or_default();
            for v in vehicles.iter().filter(|v| !band.vehicles.contains(v)) {
                if let Some(&w) = walkers.first() {
                    found.push((w, *v, 0.0, Vec2::ZERO));
                } else if let Some((w, left)) = band.walker_left {
                    found.push((w, *v, (tick - left) as f64 * DT, Vec2::ZERO));
                }
            }
            for w in walkers.iter().filter(|w| !band.walkers.contains(w)) {
                if let Some(&v) = vehicles.first() {
                    found.push((v, *w, 0.0, Vec2::ZERO));
                } else if let Some((v, left)) = band.vehicle_left {
                    found.push((v, *w, (tick - left) as f64 * DT, Vec2::ZERO));
                }
            }
            for v in band.vehicles.iter().filter(|v| !vehicles.contains(v)) {
                band.vehicle_left = Some((*v, tick));
            }
            for w in band.walkers.iter().filter(|w| !walkers.contains(w)) {
                band.walker_left = Some((*w, tick));
            }
            band.vehicles = vehicles;
            band.walkers = walkers;
        }
        self.bands.retain(|_, band| {
            !band.vehicles.is_empty()
                || !band.walkers.is_empty()
                || band
                    .vehicle_left
                    .into_iter()
                    .chain(band.walker_left)
                    .any(|(_, left)| ((tick - left) as f64 * DT) < PET_LIGHT_S)
        });
        for (first, second, pet, _) in found {
            if pet >= PET_LIGHT_S {
                continue;
            }
            let at = self
                .walkers
                .get(&first)
                .or_else(|| self.walkers.get(&second))
                .map_or(Vec2::ZERO, |w| w.pos);
            let grade = if pet < PET_SERIOUS_S {
                Grade::Serious
            } else {
                Grade::Light
            };
            self.log_conflict(ConflictKind::Pet, grade, (first, second), at, pet);
        }
    }

    /// The causes a walker brings to a conflict, if `id` is a walker.
    pub(super) fn walker_causes(&self, id: AgentId) -> Vec<Cause> {
        self.walkers.get(&id).map_or_else(Vec::new, |walker| {
            walker
                .recent
                .iter()
                .map(|(_, what)| Cause {
                    agent: walker.id,
                    what: *what,
                })
                .collect()
        })
    }
}
