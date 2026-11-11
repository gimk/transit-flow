//! The walk network against the Godot build's reference data: every golden layout, built
//! from its roads (and its mid-block crosswalks), has Godot's graph (point ids,
//! positions, connections), crosswalk edges and sidewalks, and the sampled walking
//! paths come out the same, crossings included.

mod common;

use common::golden::{self, Layout, key, v};
use glam::IVec2;
use tf_sim::roads::RoadNetwork;
use tf_sim::walking::walk_network::WalkNetwork;

#[test]
fn every_layout_has_godots_walk_network() {
    let mut points = 0;
    let mut paths = 0;
    let mut mid_block_edges = 0;
    for name in golden::LAYOUTS {
        let layout: Layout = golden::read(name);
        let roads = RoadNetwork::from_dict(&layout.save["roads"]);
        let crosswalks = layout.transit.crosswalks();
        let mut network = WalkNetwork::new();
        network.ensure_built(&roads, &crosswalks);

        let ids: Vec<i32> = network.point_ids().collect();
        let expected: Vec<i32> = layout.walk.points.iter().map(|p| p.id).collect();
        assert_eq!(ids, expected, "{name}: point ids");
        for g in &layout.walk.points {
            let what = format!("{name}, walk point {}", g.id);
            let position = network.position(g.id);
            assert!(
                position.distance(v(g.position)) < 1e-3,
                "{what}: {position} vs Godot {:?}",
                g.position
            );
            assert_eq!(network.connections(g.id), g.connections, "{what}: links");
            points += 1;
        }

        let edges: Vec<(IVec2, [i32; 2])> = network
            .crosswalk_edges
            .iter()
            .map(|(k, c)| (*k, [c.x, c.y]))
            .collect();
        let expected: Vec<(IVec2, [i32; 2])> = layout
            .walk
            .crosswalk_edges
            .iter()
            .map(|(k, c)| (key(k), *c))
            .collect();
        assert_eq!(edges, expected, "{name}: crosswalk edges (in order)");
        mid_block_edges += edges.iter().filter(|(_, c)| c[0] <= -1000).count();

        let sidewalks: Vec<(IVec2, [i32; 2])> = network
            .sidewalks
            .iter()
            .map(|(k, c)| (*k, [c.x, c.y]))
            .collect();
        let expected: Vec<(IVec2, [i32; 2])> = layout
            .walk
            .sidewalks
            .iter()
            .map(|(k, c)| (key(k), *c))
            .collect();
        assert_eq!(sidewalks, expected, "{name}: sidewalks (in order)");

        for (i, sample) in layout.walk.paths.iter().enumerate() {
            let what = format!("{name}, walk path {i}");
            let got = network.find_path(&roads, &crosswalks, v(sample.from), v(sample.to));
            let want = &sample.path;
            let Some(got) = got else {
                assert!(want.points.is_empty(), "{what}: Godot found a path");
                continue;
            };
            assert!(!want.points.is_empty(), "{what}: Godot found none");
            assert_eq!(got.points.len(), want.points.len(), "{what}: point count");
            for (p, q) in got.points.iter().zip(&want.points) {
                assert!(p.distance(v(*q)) < 1e-3, "{what}: point {p} vs {q:?}");
            }
            let crossings: Vec<[i32; 2]> = got.crossings.iter().map(|c| [c.x, c.y]).collect();
            assert_eq!(crossings, want.crossings, "{what}: crossings");
            assert!(
                (got.length - want.length).abs() < 1e-3,
                "{what}: length {} vs {}",
                got.length,
                want.length
            );
            paths += 1;
        }
    }
    assert!(points > 300, "{points} points");
    assert_eq!(paths, 6 * 12, "every sampled path found one");
    assert!(
        mid_block_edges > 20,
        "{mid_block_edges} mid-block crosswalks"
    );
}
