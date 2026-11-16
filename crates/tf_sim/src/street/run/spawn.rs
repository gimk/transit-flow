//! Demand: how agents come into a run (pivot plan §5.4). Each flow of a slice has its own
//! random stream and a backlog of agents that want to enter; an agent enters when the
//! entry lane has room, so a jammed gate delays people instead of stacking bodies.
//! Pedestrian flows work the same way: one wish brings one walker or a small group
//! (`StreetRun::add_walkers`), which enters when the sidewalk at the gate is clear.
//! Bicycle flows are not simulated yet.

use glam::Vec2;

use super::agent::{Agent, Traits, body_size, chance, vehicle_class};
use super::drive::{self, DT, MIN_GAP, World};
use super::events::StreetEvent;
use super::kerb;
use super::route::{self, RoutePath};
use super::walker::{MEAN_GROUP_SIZE, WalkSpawn};
use super::{StreetRun, mix};
use crate::rng::SimRng;
use crate::street::ids::AgentId;
use crate::street::level::{Flow, Gate, Mode};
use crate::street::model::Street;
use crate::street::space::Access;

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
) -> (Vec2, Vec2) {
    let front = route.point(street, d);
    let rear = route.point(street, d - length);
    let dir = (front - rear).normalize_or_zero();
    (
        (front + rear) / 2.0,
        if dir == Vec2::ZERO { old_dir } else { dir },
    )
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
                    self.flow_state[s][f].backlog += 1;
                }
            }
        }
        // Wishes left over from earlier slices still enter when there is room.
        let upto = self.current_slice.map_or(self.slices.len(), |s| s + 1);
        for s in 0..upto {
            for f in 0..self.slices[s].flows.len() {
                if self.flow_state[s][f].backlog == 0 {
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
                    Attempt::Spawned => self.flow_state[s][f].backlog -= 1,
                    Attempt::NoRoute => self.flow_state[s][f].backlog = 0,
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
        let (length, width) = body_size(flow.mode);
        let first = self.routes[route_index].lanes[0];
        // The entry is clear when the last one to come in has moved on by a body and a gap.
        // The leader is the one whose rear is nearest the entry: the newcomer must be able
        // to stay behind it at its speed, with its own headway (a car that came in at full
        // speed behind a slower one would brake hard on its first tick).
        let mut leader: Option<(f64, f64)> = None; // (rear of the leader, its speed)
        for other in self.agents.values() {
            if self.routes[other.route].lanes[0] != first {
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
        };
        let route = &self.routes[route_index];
        let world = World {
            street: &self.street,
            routes: &self.routes,
            moves: &self.moves,
            plans: &self.plans,
            time_s: self.tick as f64 * DT,
        };
        let wanted = drive::desired_speed(&agent, first, &world);
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
        let (pos, dir) = pose(&self.street, route, agent.d, length, Vec2::X);
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
