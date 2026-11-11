//! `Scenarios` (`sim/debug/scenarios.gd`): ready-made layouts for playtesting (F7 and F8
//! in game) and for tests, built from code through the sim's command methods.

use glam::{IVec2, Vec2};

use crate::Simulation;
use crate::city::LotType;
use crate::grid;
use crate::ids::{LineId, StopId};
use crate::roads::{JunctionControl, RoadType};

/// Where the Town scenario sits: its center cell (the Main Street / Avenue crossing),
/// placed so its two highways reach the west and north map edges after a short drive.
pub const TOWN_CENTER: IVec2 = IVec2::new(20, 18);

/// What `build_bottleneck` returns: positions (world px) tests need.
#[derive(Clone, Debug)]
pub struct BottleneckInfo {
    /// The light-controlled junction.
    pub junction: Vec2,
    /// `[from, to]` pairs for three roads that skip the lights and reach the factories
    /// from the far end (not built).
    pub bypass: Vec<[Vec2; 2]>,
}

/// What `build_town` returns (positions in world px).
#[derive(Clone, Debug)]
pub struct TownInfo {
    /// The downtown crossing.
    pub center: Vec2,
    /// The light-controlled junctions.
    pub lights: Vec<Vec2>,
    /// The two external connection points.
    pub highways: Vec<Vec2>,
    /// The bus line ids.
    pub bus_lines: Vec<LineId>,
}

fn road_type(id: &str) -> &'static RoadType {
    RoadType::load_by_id(id).unwrap_or_else(|| panic!("road type {id} is built in"))
}

/// `Vector2(cell) * Grid.CELL_SIZE`.
fn cell_point(cell: IVec2) -> Vec2 {
    cell.as_vec2() * grid::CELL_SIZE as f32
}

fn add_road(sim: &mut Simulation, from: Vec2, to: Vec2, road_type: &'static RoadType) {
    sim.call(|sim, out| {
        sim.roads.add_road(from, to, road_type, 0, out);
    });
}

fn set_control(sim: &mut Simulation, point: Vec2, control: JunctionControl) {
    let node = sim.roads.find_node_at(point, 0).expect("a node to control");
    sim.call(|sim, out| {
        sim.roads.set_node_control(node, control, out);
    });
}

fn add_stop(sim: &mut Simulation, point: Vec2, bay: bool) -> StopId {
    let (transit, mut ctx) = sim.transit_ctx();
    transit.add_stop(&mut ctx, point, bay).unwrap_or(StopId(-1))
}

fn add_line(sim: &mut Simulation, stops: &[StopId]) -> Option<LineId> {
    let (transit, mut ctx) = sim.transit_ctx();
    transit.add_line(&mut ctx, stops)
}

/// "Bottleneck": a suburb of 28 homes (56 commuters) in the west commutes along one main
/// road to 24 factories in the east, through a T-junction with traffic lights that also
/// serve a side street of commuters. Everyone leaves within ~15 s, so a long queue builds
/// at the lights.
pub fn build_bottleneck(sim: &mut Simulation, origin: IVec2) -> BottleneckInfo {
    let local = road_type("local_2lane");
    let at = |x: i32, y: i32| cell_point(origin + IVec2::new(x, y));

    add_road(sim, at(-15, 0), at(15, 0), local);
    add_road(sim, at(0, 0), at(0, 7), local);
    set_control(sim, at(0, 0), JunctionControl::TrafficLight);

    let lot = |sim: &mut Simulation, x: i32, y: i32, lot_type: LotType| {
        sim.call(|sim, out| {
            sim.lots
                .add_lot(&sim.roads, origin + IVec2::new(x, y), lot_type, out);
        });
    };
    for x in -14..-2 {
        lot(sim, x, -1, LotType::Residential);
        lot(sim, x, 0, LotType::Residential);
    }
    for y in 2..6 {
        lot(sim, -1, y, LotType::Residential);
    }
    for x in 3..15 {
        lot(sim, x, -1, LotType::Industrial);
        lot(sim, x, 0, LotType::Industrial);
    }
    // Everyone leaves within ~15 s (1.25 game hours), so the queue builds fast but drivers
    // setting off later can see the jam and choose another way.
    sim.citizens.depart_everyone_now(15.0);

    BottleneckInfo {
        junction: at(0, 0),
        bypass: vec![
            [at(-3, 0), at(-3, -4)],
            [at(-3, -4), at(15, -4)],
            [at(15, -4), at(15, 0)],
        ],
    }
}

/// A bus line along the Bottleneck's main road: east past the suburb and the factories
/// on the south side, back west on the north side, turning round at the road's ends.
pub fn add_bottleneck_bus_line(sim: &mut Simulation, origin: IVec2) -> Option<LineId> {
    let mut stops = Vec::new();
    for x in [-12, -8, -4, 5, 9, 13] {
        let point = cell_point(origin + IVec2::new(x, 0)) + Vec2::new(0.0, 10.0);
        stops.push(add_stop(sim, point, false));
    }
    for x in [13, 9, 5, -4, -8, -12] {
        let point = cell_point(origin + IVec2::new(x, 0)) + Vec2::new(0.0, -10.0);
        stops.push(add_stop(sim, point, false));
    }
    add_line(sim, &stops)
}

/// The Bottleneck with transit priority: its main road made an avenue with bus lanes
/// (still one lane each way for cars) and a bus line along it.
pub fn add_bottleneck_bus_lanes(sim: &mut Simulation, origin: IVec2) -> Option<LineId> {
    let bus_avenue = road_type("avenue_4lane_bus");
    let y = (origin.y * grid::CELL_SIZE) as f32;
    let ids: Vec<_> = sim.roads.segments.keys().copied().collect();
    for id in ids {
        let segment = &sim.roads.segments[&id];
        let a = sim.roads.nodes[&segment.start_node_id].position;
        let b = sim.roads.nodes[&segment.end_node_id].position;
        if (a.y - y).abs() < 1.0 && (b.y - y).abs() < 1.0 {
            sim.call(|sim, out| {
                sim.roads.set_segment_type(id, bus_avenue, false, out);
            });
        }
    }
    add_bottleneck_bus_line(sim, origin)
}

/// One road of the Town: a straight one or one with a bend (a curve); one-way roads run
/// from the first point to the last.
enum TownRoad {
    Straight([i32; 4], &'static str),
    Curved([i32; 6], &'static str),
}

/// "Town": a complete small city that lives through a whole day on its own, to test the
/// simulation end to end, using every feature built so far (keep it that way: update it
/// at the end of each phase). Cells below are relative to `TOWN_CENTER`; the town spans
/// x -12..12, y -8..8.
/// - Roads: a ring road around the town (two of its corners curved); Main Street
///   (east-west, y 0), an avenue, runs in from the west highway; the Avenue (north-south,
///   x 0) has bus lanes; the two cross streets (y -4, y 4) are a one-way pair (eastbound
///   north, westbound south); two side streets (x -6, x 6), the latter fed by the north
///   highway; a cul-de-sac in the south-west.
/// - Traffic lights on the Avenue's and Main Street's busiest crossings, the Avenue's with
///   the south cross street with protected left turns; every other junction is
///   uncontrolled (the avenues and the two-lane one-ways have priority there).
/// - Zones, developed at once: two residential districts in the west (Oak Hill north of
///   Main Street, Riverside south of it) and Southgate in the south-east; shops downtown
///   along Main Street and the Avenue; an industrial park in the north-east. Buildings
///   stand back from the avenues, with their parking.
/// - Four bus lines: Main Street there and back with bus bays, the Avenue there and back
///   in its bus lanes, the Crosstown loop round the one-way pair with curbside stops, and
///   the Ring round the ring road.
pub fn build_town(sim: &mut Simulation) -> TownInfo {
    use TownRoad::{Curved, Straight};
    let at = |x: i32, y: i32| cell_point(TOWN_CENTER + IVec2::new(x, y));
    let roads = [
        Straight([-20, 0, -12, 0], "local_2lane"), // West highway,
        Straight([-12, 0, 12, 0], "avenue_4lane"), // into Main Street, an avenue.
        Straight([6, -18, 6, 4], "local_2lane"), // North highway, down through the industrial park.
        Straight([0, -8, 0, 8], "avenue_4lane_bus"), // The Avenue, with bus lanes.
        Straight([-12, -8, 10, -8], "local_2lane"), // Ring road, its south-west and north-east
        Curved([10, -8, 12, -8, 12, -6], "local_2lane"), // corners curved.
        Straight([-10, 8, 12, 8], "local_2lane"),
        Curved([-12, 6, -12, 8, -10, 8], "local_2lane"),
        Straight([-12, -8, -12, 6], "local_2lane"),
        Straight([12, -6, 12, 8], "local_2lane"),
        Straight([-12, -4, 12, -4], "one_way_2lane"), // Cross streets: a one-way pair, eastbound
        Straight([12, 4, -12, 4], "one_way_2lane"),   // in the north, westbound in the south.
        Straight([-6, -8, -6, 8], "local_2lane"),     // West side street.
        Straight([-3, 4, -3, 7], "local_2lane"),      // Cul-de-sac.
    ];
    for road in roads {
        match road {
            Curved(c, id) => {
                let (from, control, to) = (at(c[0], c[1]), at(c[2], c[3]), at(c[4], c[5]));
                sim.call(|sim, out| {
                    sim.roads
                        .add_curved_road(from, control, to, road_type(id), 0, out);
                });
            }
            Straight(c, id) => add_road(sim, at(c[0], c[1]), at(c[2], c[3]), road_type(id)),
        }
    }
    let lights = vec![at(0, -4), at(0, 0), at(0, 4), at(-6, 0), at(6, 0)];
    for &point in &lights {
        set_control(sim, point, JunctionControl::TrafficLight);
    }
    // The Avenue's crossing with the south cross street gives left turns a phase of their
    // own (tried elsewhere: at busier crossings the four-phase cycle jams the morning).
    set_control(sim, at(0, 4), JunctionControl::TrafficLightProtectedLeft);
    let highways = vec![at(-20, 0), at(6, -18)];
    for &point in &highways {
        sim.call(|sim, out| sim.externals.add(point, out));
    }

    let zone = |sim: &mut Simulation, x0: i32, y0: i32, x1: i32, y1: i32, lot_type: LotType| {
        let mut cells = Vec::new();
        for x in x0..=x1 {
            for y in y0..=y1 {
                cells.push(TOWN_CENTER + IVec2::new(x, y));
            }
        }
        sim.call(|sim, out| sim.zones.paint(&mut sim.lots, &cells, lot_type, out));
    };
    zone(sim, -11, -8, -1, -8, LotType::Residential); // Oak Hill,
    zone(sim, -11, -5, -1, -4, LotType::Residential);
    zone(sim, -11, 3, -1, 4, LotType::Residential); // Riverside,
    zone(sim, -11, 7, -4, 7, LotType::Residential);
    zone(sim, -4, 5, -3, 6, LotType::Residential); // and its cul-de-sac.
    zone(sim, 7, 3, 11, 4, LotType::Residential); // Southgate.
    zone(sim, 1, 7, 11, 7, LotType::Residential);
    zone(sim, -5, -1, 11, 0, LotType::Commercial); // Downtown, along Main Street,
    zone(sim, -1, 1, 0, 2, LotType::Commercial); // and down the Avenue.
    zone(sim, 1, -5, 11, -4, LotType::Industrial); // Industrial park.
    zone(sim, 1, -8, 10, -8, LotType::Industrial); // (Not under the curved corner.)
    zone(sim, 5, -3, 6, -2, LotType::Industrial);
    sim.call(|sim, out| sim.zones.develop_all_now(&sim.roads, &mut sim.lots, out));

    // Three bus lines: along Main Street and back, stopping in bus bays (so the avenue's
    // traffic drives past); up and down the Avenue in its bus lanes; and the Crosstown
    // loop round the one-way pair (east along the north cross street, west along the
    // south one), with curbside stops. Riders change lines at stops a short walk apart.
    let mut bus_lines = Vec::new();
    let stops = |sim: &mut Simulation, list: &[(i32, i32, Vec2, bool)]| {
        let ids: Vec<StopId> = list
            .iter()
            .map(|&(x, y, offset, bay)| add_stop(sim, at(x, y) + offset, bay))
            .collect();
        add_line(sim, &ids)
    };
    let (south, north) = (Vec2::new(0.0, 10.0), Vec2::new(0.0, -10.0));
    let (west, east) = (Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0));
    let mut main_street = Vec::new();
    for x in [-9, -2, 2, 9] {
        main_street.push((x, 0, south, true));
    }
    for x in [9, 2, -2, -9] {
        main_street.push((x, 0, north, true));
    }
    bus_lines.extend(stops(sim, &main_street));
    let mut avenue = Vec::new();
    for y in [-6, -2, 2, 6] {
        avenue.push((0, y, west, false));
    }
    for y in [6, 2, -2, -6] {
        avenue.push((0, y, east, false));
    }
    bus_lines.extend(stops(sim, &avenue));
    let mut crosstown = Vec::new();
    for x in [-9, -3, 3, 9] {
        crosstown.push((x, -4, south, false));
    }
    for x in [9, 3, -3, -9] {
        crosstown.push((x, 4, north, false));
    }
    bus_lines.extend(stops(sim, &crosstown));
    // And the Ring: clockwise round the ring road, past the homes on the town's north and
    // south edges (the right-hand curb going clockwise is the town side).
    let mut ring = Vec::new();
    for x in [-9, -3, 3, 9] {
        ring.push((x, -8, south, false));
    }
    for y in [-2, 2] {
        ring.push((12, y, west, false));
    }
    for x in [9, 3, -3, -9] {
        ring.push((x, 8, north, false));
    }
    for y in [2, -2] {
        ring.push((-12, y, east, false));
    }
    bus_lines.extend(stops(sim, &ring));

    TownInfo {
        center: at(0, 0),
        lights,
        highways,
        bus_lines,
    }
}

/// Gives a new map its highway: a short local road from the middle of the left edge, with
/// an external connection where it leaves the map.
pub fn add_highway(sim: &mut Simulation) {
    let map = grid::map_rect();
    let entry = grid::snap_to_point(Vec2::new(map.position.x, map.center().y));
    let cell = grid::CELL_SIZE as f32;
    add_road(
        sim,
        entry,
        entry + Vec2::new(6.0, 0.0) * cell,
        road_type("local_2lane"),
    );
    sim.call(|sim, out| sim.externals.add(entry, out));
}

/// Builds the bypass roads returned by `build_bottleneck`.
pub fn build_bypass(sim: &mut Simulation, info: &BottleneckInfo) {
    let local = road_type("local_2lane");
    for road in &info.bypass {
        add_road(sim, road[0], road[1], local);
    }
}
