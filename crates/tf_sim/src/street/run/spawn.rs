//! Demand: how agents come into a run (pivot plan §5.4). Each flow of a slice has its own
//! random stream and a backlog of agents that want to enter; an agent enters when the
//! entry lane has room, so a jammed gate delays people instead of stacking bodies.
//! Pedestrian flows work the same way: one wish brings one walker or a small group
//! (`StreetRun::add_walkers`), which enters when the sidewalk at the gate is clear.
//! Bicycle flows spawn cyclists, who are vehicles with a side position (`bike`).

use glam::Vec2;

use super::agent::{
    Agent, BikeHabits, CARGO_BIKE_SIZE, DeliveryStop, Traits, body_size, chance, vehicle_class,
};
use super::cross;
use super::drive::{self, DT, MIN_GAP, World};
use super::events::StreetEvent;
use super::kerb;
use super::perceive;
use super::route::{self, RoutePath};
use super::walker::{MEAN_GROUP_SIZE, WalkSpawn};
use super::{StreetRun, mix};
use crate::rng::SimRng;
use crate::street::ids::AgentId;
use crate::street::level::{Flow, Gate, Mode};
use crate::street::model::Street;
use crate::street::space::Access;

/// The headway a newcomer keeps to a slower body from its own gate in another lane, seconds
/// (start value: the car-following headway of about 1.2 s, applied to the speed it would
/// gain on that body).
const ENTRY_GAP_HEADWAY_S: f64 = 1.2;

enum Attempt {
    Spawned,
    Blocked,
    NoRoute,
}

/// The middle and heading of a body of `length` whose front is at distance `d`.
pub(super) fn pose(
    street: &Street,
    route: &RoutePath,
    d: f64,
    length: f64,
    old_dir: Vec2,
    lateral: f64,
) -> (Vec2, Vec2) {
    let front = route.point(street, d);
    let rear = route.point(street, d - length);
    let dir = (front - rear).normalize_or_zero();
    let dir = if dir == Vec2::ZERO { old_dir } else { dir };
    // The body is shifted to its side position, to the right of its heading.
    let shift = if lateral == 0.0 {
        Vec2::ZERO
    } else {
        dir.perp() * lateral as f32
    };
    ((front + rear) / 2.0 + shift, dir)
}

impl StreetRun {
    /// Stage 2: new wishes to enter from the current slice, then the entries themselves.
    pub(super) fn spawn_stage(&mut self) {
        if let Some(s) = self.current_slice {
            for f in 0..self.slices[s].flows.len() {
                let flow = &self.slices[s].flows[f];
                if vehicle_class(flow.mode).is_none() && flow.mode != Mode::Pedestrian {
                    continue;
                }
                // `per_hour` counts people: a pedestrian wish brings a group of
                // `MEAN_GROUP_SIZE` people on average.
                let per_wish = if flow.mode == Mode::Pedestrian {
                    f64::from(MEAN_GROUP_SIZE)
                } else {
                    1.0
                };
                let p = f64::from(flow.per_hour) * self.demand_scale / 3600.0 * DT / per_wish;
                if chance(&mut self.flow_state[s][f].rng, p) {
                    let tick = self.tick;
                    self.flow_state[s][f].backlog.push(tick);
                }
            }
        }
        // Wishes left over from earlier slices still enter when there is room.
        let upto = self.current_slice.map_or(self.slices.len(), |s| s + 1);
        for s in 0..upto {
            for f in 0..self.slices[s].flows.len() {
                if self.flow_state[s][f].backlog.is_empty() {
                    continue;
                }
                let flow = self.slices[s].flows[f].clone();
                let attempt = if flow.mode == Mode::Pedestrian {
                    match self.add_walkers(&flow.from, &flow.to, None) {
                        WalkSpawn::Spawned(_) => Attempt::Spawned,
                        WalkSpawn::Blocked => Attempt::Blocked,
                        WalkSpawn::NoRoute => Attempt::NoRoute,
                    }
                } else {
                    self.try_spawn(&flow)
                };
                match attempt {
                    Attempt::Spawned => {
                        self.flow_state[s][f].backlog.remove(0);
                    }
                    Attempt::NoRoute => self.flow_state[s][f].backlog.clear(),
                    Attempt::Blocked => {}
                }
            }
        }
    }

    fn gate(&self, id: &str) -> Option<&Gate> {
        self.gates.iter().find(|gate| gate.id == id)
    }

    /// The index of the route between two gates for a class, found once per run.
    fn route_for(&mut self, from: &str, to: &str, class: Access) -> Option<usize> {
        let key = (class.0, from.to_string(), to.to_string());
        if let Some(found) = self.route_cache.get(&key) {
            return *found;
        }
        let found = match (self.gate(from), self.gate(to)) {
            (Some(a), Some(b)) => route::find_route(&self.street, a, b, class),
            _ => None,
        }
        .map(|lanes| {
            let route = RoutePath::new(&self.street, lanes);
            self.route_areas
                .push(kerb::road_areas_of(&self.street, &self.walk_space, &route));
            self.routes.push(route);
            self.routes.len() - 1
        });
        self.route_cache.insert(key, found);
        found
    }

    fn try_spawn(&mut self, flow: &Flow) -> Attempt {
        let class = vehicle_class(flow.mode).expect("only vehicle flows are spawned");
        let Some(route_index) = self.route_for(&flow.from, &flow.to, class) else {
            return Attempt::NoRoute;
        };
        let (mut length, mut width) = body_size(flow.mode);
        let first = self.routes[route_index].lanes[0];
        // The entry is clear when the last one to come in has moved on by a body and a gap.
        // The leader is the one whose rear is nearest the entry: the newcomer must be able
        // to stay behind it at its speed, with its own headway (a car that came in at full
        // speed behind a slower one would brake hard on its first tick).
        let mut leader: Option<(f64, f64)> = None; // (rear of the leader, its speed)
        let first_segment = self.street.lanes[&first].segment_id;
        let first_node = self.street.lanes[&first].from_node;
        // The speed the newcomer would come in at, at most: the limit of its first lane.
        let v_spawn = self.street.lanes[&first].speed_limit;
        for other in self.agents.values() {
            if self.routes[other.route].lanes[0] != first {
                // A slower body from the same gate in another lane of the segment (a rider
                // that swerves into the car lane in the first metres): the newcomer could not
                // stop for it, and it does not see it, so it waits at the gate.
                let other_first = self.routes[other.route].lanes[0];
                // Same segment and same way (same entry node): not the traffic of the other gate.
                if first_segment.is_some()
                    && self.street.lanes[&other_first].segment_id == first_segment
                    && self.street.lanes[&other_first].from_node == first_node
                    && other.speed < v_spawn
                    && other.d - other.length
                        < length + MIN_GAP + 1.0 + (v_spawn - other.speed) * ENTRY_GAP_HEADWAY_S
                {
                    return Attempt::Blocked;
                }
                continue;
            }
            let rear = other.d - other.length;
            if rear < length + MIN_GAP + 1.0 {
                return Attempt::Blocked;
            }
            if leader.is_none_or(|(r, _)| rear < r) {
                leader = Some((rear, other.speed));
            }
        }
        let id = AgentId(self.next_agent);
        self.next_agent += 1;
        let mut rng = SimRng::new(mix(self.seed, id.0 as u64));
        let archetype = self.population.archetype(flow.mode, &mut rng);
        let traits = Traits::draw(&archetype, &mut rng);
        // A cyclist's own habits come after the seven traits, so no car draw moves.
        let bike = (flow.mode == Mode::Bicycle).then(|| BikeHabits::draw(&mut rng));
        if flow.mode == Mode::Bicycle && archetype.name == "cargo bike" {
            (length, width) = CARGO_BIKE_SIZE;
        }
        // A delivery stop: a van, or a careless car, pulls over into the painted bicycle
        // lane beside its lane and stands there (Phase 6 W4b). One draw for every motor
        // vehicle, so a rider's stream does not move.
        let stop = if flow.mode == Mode::Bicycle {
            None
        } else {
            let draw = f64::from(rng.randf());
            let chance = match flow.mode {
                Mode::Van => cross::VAN_STOP_CHANCE,
                Mode::Car => cross::CAR_STOP_CHANCE * (1.0 - traits.compliance),
                _ => 0.0,
            };
            if draw < chance {
                cross::delivery_stop_place(&self.street, &self.routes[route_index], class).map(
                    |(at_d, lateral)| DeliveryStop {
                        at_d,
                        until: self.tick + (cross::STOP_DURATION_S / DT).round() as u64,
                        lateral,
                    },
                )
            } else {
                None
            }
        };
        let mut agent = Agent {
            id,
            mode: flow.mode,
            archetype: archetype.name,
            traits,
            rng,
            route: route_index,
            d: length,
            speed: 0.0,
            accel: 0.0,
            max_brake: drive::draw_max_brake(flow.mode, self.seed, id),
            length,
            width,
            pos: Vec2::ZERO,
            dir: Vec2::X,
            prev_pos: Vec2::ZERO,
            prev_dir: Vec2::X,
            spawn_tick: self.tick,
            free_time_s: 0.0,
            frustration: 0.0,
            wait_ticks: 0,
            approach: None,
            distracted_until: 0,
            crashed_until: None,
            in_zones: Vec::new(),
            recent: Vec::new(),
            last_hard_brake: None,
            crossing: None,
            held_by_walkers: false,
            last_held: None,
            electric: perceive::is_electric(self.seed, id.0, flow.mode),
            gaze_seed: perceive::gaze_seed(self.seed, id.0),
            gaze_task: perceive::DriverTask::Cruise,
            lateral: 0.0,
            lateral_target: 0.0,
            passing: false,
            bike,
            stop,
        };
        let route = &self.routes[route_index];
        let world = World {
            street: &self.street,
            routes: &self.routes,
            moves: &self.moves,
            plans: &self.plans,
            street_speeds: &self.street_speeds,
            time_s: self.tick as f64 * DT,
            peds: None,
            doors: &self.doors,
        };
        let wanted = drive::desired_speed_at(&agent, first, 0.0, &world);
        // The newcomer decides from the world one reaction time ago, where the leader was
        // `lead_speed * reaction` metres further back: the cap uses the gap as the
        // newcomer will see it on its first tick, or it would brake hard at once.
        agent.speed = match leader {
            Some((rear, lead_speed)) => {
                let lag_ticks = ((agent.traits.reaction_s / DT).round() as usize).max(1);
                let seen_gap = rear - length - lead_speed * lag_ticks as f64 * DT;
                wanted.min(drive::safe_speed(
                    seen_gap - MIN_GAP,
                    lead_speed,
                    drive::headway(&agent),
                ))
            }
            None => wanted,
        };
        agent.free_time_s = route
            .lanes
            .iter()
            .map(|lane| {
                self.street.lanes[lane].length
                    / drive::desired_speed(&agent, *lane, &world).max(0.1)
            })
            .sum();
        // A cyclist starts at its resting place in the lane.
        agent.lateral = super::bike::rest_lateral(&agent, first, &world);
        agent.lateral_target = agent.lateral;
        let (pos, dir) = pose(&self.street, route, agent.d, length, Vec2::X, agent.lateral);
        (agent.pos, agent.dir, agent.prev_pos, agent.prev_dir) = (pos, dir, pos, dir);
        self.log(StreetEvent::Spawned {
            agent: id,
            mode: flow.mode,
            archetype: archetype.name.to_string(),
            from: flow.from.clone(),
            to: flow.to.clone(),
        });
        self.agents.insert(id, agent);
        Attempt::Spawned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::street::level::{LevelFile, Slice};
    use crate::street::run::{Population, RunOptions};
    use crate::street::space::{Direction, StripUse};
    use crate::street::{Design, DesignCommand};

    fn flow(mode: Mode, per_hour: u32) -> Flow {
        Flow {
            mode,
            from: "west".to_string(),
            to: "east".to_string(),
            per_hour,
        }
    }

    /// A rider that swerves into the car lane in the first metres holds the next car at the
    /// gate; a car that is clear of it comes in.
    #[test]
    fn a_car_does_not_enter_behind_a_slow_body_of_another_lane_of_its_gate() {
        let mut level = LevelFile::load("carrefour-ecole").expect("the level loads");
        level.demand = vec![Slice {
            name: "test".to_string(),
            duration_s: 120.0,
            flows: vec![flow(Mode::Bicycle, 600)],
        }];
        let mut design = Design::new(&level.id);
        design.commands = vec![DesignCommand::SetStripUse {
            segment: 1,
            strip: 3,
            use_: StripUse::BikeLane,
            direction: Direction::Forward,
        }];
        let options = RunOptions {
            seed: 3,
            population: Population::Mixed,
            demand_scale: 1.0,
        };
        let mut run = StreetRun::new(&level, &design, options).expect("builds");
        while run.agents.is_empty() {
            run.step();
        }
        let rider = *run.agents.keys().next().expect("a rider came in");
        let car = flow(Mode::Car, 600);
        let car_lane = {
            let index = run
                .route_for("west", "east", vehicle_class(Mode::Car).unwrap())
                .expect("a car route");
            run.routes[index].lanes[0]
        };
        assert_ne!(
            run.routes[run.agents[&rider].route].lanes[0], car_lane,
            "the rider and the car must start in different lanes"
        );
        // The rider crawls near the gate: the car is held back.
        {
            let rider = run.agents.get_mut(&rider).expect("the rider");
            rider.speed = 1.0;
            rider.d = rider.length + 3.0;
        }
        assert!(matches!(run.try_spawn(&car), Attempt::Blocked));
        // The rider is far down the street: the entry is free.
        run.agents.get_mut(&rider).expect("the rider").d = 60.0;
        assert!(matches!(run.try_spawn(&car), Attempt::Spawned));
    }
}
