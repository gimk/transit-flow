//! `Scenarios` (`sim/debug/scenarios.gd`): maps built from code. Only `add_highway` so
//! far; the Bottleneck and Town scenarios come with work package M7.3.

use glam::Vec2;

use crate::Simulation;
use crate::grid;
use crate::roads::RoadType;

/// Gives a new map its highway: a short local road from the middle of the left edge, with
/// an external connection where it leaves the map.
pub fn add_highway(sim: &mut Simulation) {
    let map = grid::map_rect();
    let entry = grid::snap_to_point(Vec2::new(map.position.x, map.center().y));
    let road_type = RoadType::load_by_id("local_2lane").expect("local_2lane is built in");
    let cell = grid::CELL_SIZE as f32;
    sim.call(|sim, out| {
        sim.roads
            .add_road(entry, entry + Vec2::new(6.0, 0.0) * cell, road_type, 0, out);
    });
    sim.call(|sim, out| sim.externals.add(entry, out));
}
