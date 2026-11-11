//! The scenarios build the same maps as Godot's: each one's save equals the save Godot
//! exported (`migration/golden/`), roads, lots, zones, connections, stops and lines.

mod common;

use common::golden::{self, Layout, assert_json_matches};
use glam::IVec2;
use tf_sim::Simulation;
use tf_sim::debug::scenarios;

const ORIGIN: IVec2 = IVec2::new(30, 30);

fn compare(name: &str, sim: &Simulation) {
    let layout: Layout = golden::read(name);
    assert_json_matches(&sim.to_dict(), &layout.save, name);
}

#[test]
fn the_bottleneck_is_godots() {
    let mut sim = Simulation::new();
    scenarios::build_bottleneck(&mut sim, ORIGIN);
    compare("bottleneck", &sim);
}

#[test]
fn the_bypass_is_godots() {
    let mut sim = Simulation::new();
    let info = scenarios::build_bottleneck(&mut sim, ORIGIN);
    scenarios::build_bypass(&mut sim, &info);
    compare("bottleneck_bypass", &sim);
}

#[test]
fn the_bus_lanes_are_godots() {
    let mut sim = Simulation::new();
    scenarios::build_bottleneck(&mut sim, ORIGIN);
    scenarios::add_bottleneck_bus_lanes(&mut sim, ORIGIN);
    compare("bottleneck_bus_lanes", &sim);
}

#[test]
fn the_town_is_godots() {
    let mut sim = Simulation::new();
    scenarios::build_town(&mut sim);
    compare("town", &sim);
}
