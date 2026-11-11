//! Stops, lines and mid-block crosswalks against the Godot build's reference data: every
//! golden layout's saved stops and lines, restored by the Rust `TransitSystem`, end up on
//! the same lanes and at the same distances, with the same routes, timings and path,
//! and the same mid-block crosswalks, as Godot exported them.

mod common;

use common::golden::{self, Layout, key, v};
use tf_sim::Simulation;
use tf_sim::ids::{LineId, StopId};
use tf_sim::roads::RoadNetwork;

#[test]
fn every_layout_has_godots_stops_lines_and_crosswalks() {
    let (mut stops, mut lines, mut crossings, mut routes) = (0, 0, 0, 0);
    for name in golden::LAYOUTS {
        let layout: Layout = golden::read(name);
        let mut sim = Simulation::with_roads(RoadNetwork::from_dict(&layout.save["roads"]));
        {
            let (transit, mut ctx) = sim.transit_ctx();
            transit.restore(&mut ctx, &layout.save["transit"]);
        }
        let transit = &sim.transit;

        let ids: Vec<i32> = transit.stops.keys().map(|id| id.0).collect();
        let expected: Vec<i32> = layout.transit.stops.iter().map(|s| s.id).collect();
        assert_eq!(ids, expected, "{name}: stop ids");
        for g in &layout.transit.stops {
            let what = format!("{name}, stop {}", g.id);
            let stop = &transit.stops[&StopId(g.id)];
            assert_eq!(stop.lane_id.map_or(-1, |l| l.0), g.lane_id, "{what}: lane");
            assert!(
                (stop.distance - g.distance).abs() < 1e-3,
                "{what}: distance {} vs {}",
                stop.distance,
                g.distance
            );
            assert_eq!(stop.bay, g.bay, "{what}: bay");
            assert!(
                stop.position.distance(v(g.position)) < 1e-3,
                "{what}: position {} vs {:?}",
                stop.position,
                g.position
            );
            assert!(
                stop.direction.distance(v(g.direction)) < 1e-5,
                "{what}: direction {} vs {:?}",
                stop.direction,
                g.direction
            );
            stops += 1;
        }

        let ids: Vec<i32> = transit.lines.keys().map(|id| id.0).collect();
        let expected: Vec<i32> = layout.transit.lines.iter().map(|l| l.id).collect();
        assert_eq!(ids, expected, "{name}: line ids");
        for g in &layout.transit.lines {
            let what = format!("{name}, line {}", g.id);
            let line = &transit.lines[&LineId(g.id)];
            let stop_ids: Vec<i32> = line.stop_ids.iter().map(|s| s.0).collect();
            assert_eq!(stop_ids, g.stop_ids, "{what}: stops");
            assert_eq!(line.color, g.color, "{what}: colour");
            assert_eq!(line.free_flow_times.len(), g.free_flow_times.len());
            for (i, (got, want)) in line
                .free_flow_times
                .iter()
                .zip(&g.free_flow_times)
                .enumerate()
            {
                assert!(
                    (f64::from(*got) - want).abs() < 1e-3,
                    "{what}: free-flow time {i}: {got} vs {want}"
                );
            }
            assert_eq!(line.routes.len(), g.routes.len(), "{what}: route count");
            for (i, (got, want)) in line.routes.iter().zip(&g.routes).enumerate() {
                let (Some(got), Some(want)) = (got, want) else {
                    assert!(got.is_none() && want.is_none(), "{what}: route {i} exists");
                    continue;
                };
                let lane_ids: Vec<i32> = got.lane_ids.iter().map(|l| l.0).collect();
                assert_eq!(lane_ids, want.lane_ids, "{what}: route {i} lanes");
                assert!((got.start_distance - want.start_distance).abs() < 1e-3);
                assert!((got.end_distance - want.end_distance).abs() < 1e-3);
                assert!((got.travel_time - want.travel_time).abs() < 1e-3);
                routes += 1;
            }
            assert_eq!(line.path.len(), g.path.len(), "{what}: path length");
            for (p, q) in line.path.iter().zip(&g.path) {
                assert!(p.distance(v(*q)) < 1e-3, "{what}: path point {p} vs {q:?}");
            }
            lines += 1;
        }

        let keys: Vec<_> = sim.crosswalks.mid_block.keys().copied().collect();
        let expected: Vec<_> = layout.transit.mid_block.keys().map(|k| key(k)).collect();
        assert_eq!(
            keys, expected,
            "{name}: mid-block crosswalk keys (in order)"
        );
        for (k, g) in &layout.transit.mid_block {
            let what = format!("{name}, mid-block {k}");
            let got = &sim.crosswalks.mid_block[&key(k)];
            assert_eq!(got.segment_id.0, g.segment_id, "{what}: segment");
            assert!((got.distance - g.distance).abs() < 1e-3, "{what}: distance");
            assert!(got.center.distance(v(g.center)) < 1e-3, "{what}: center");
            assert!(got.across.distance(v(g.across)) < 1e-5, "{what}: across");
            assert!(
                (got.half_width - g.half_width).abs() < 1e-6,
                "{what}: width"
            );
            let got_lanes: Vec<i32> = got.lanes.iter().map(|l| l.0.0).collect();
            let want_lanes: Vec<i32> = g.lanes.iter().map(|l| l.0).collect();
            assert_eq!(got_lanes, want_lanes, "{what}: lanes");
            for (a, b) in got.lanes.iter().zip(&g.lanes) {
                assert!((a.1 - b.1).abs() < 1e-3, "{what}: lane offset");
            }
            crossings += 1;
        }
    }
    assert_eq!(stops, 12 + 4 + 36, "every stop");
    assert_eq!(lines, 1 + 1 + 4, "every line");
    assert_eq!(routes, 12 + 4 + 8 + 8 + 8 + 12, "every route");
    assert!(crossings > 20, "{crossings} mid-block crosswalks");
}
