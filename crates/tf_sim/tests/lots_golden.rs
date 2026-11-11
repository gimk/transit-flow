//! Lots against the Godot build's reference data: every lot of every golden layout,
//! restored from its save, has Godot's plot and access points.

mod common;

use common::golden::{self, Layout};
use tf_sim::city::LotSystem;
use tf_sim::events::Outbox;
use tf_sim::ids::LotId;
use tf_sim::roads::RoadNetwork;

#[test]
fn every_lot_has_godots_plot_and_access() {
    let mut count = 0;
    for name in golden::LAYOUTS {
        let layout: Layout = golden::read(name);
        let roads = RoadNetwork::from_dict(&layout.save["roads"]);
        let mut lots = LotSystem::new(&roads);
        lots.restore(&roads, &layout.save["lots"], &mut Outbox::new());
        let ids: Vec<i32> = lots.lots.keys().map(|id| id.0).collect();
        let expected: Vec<i32> = layout.lots.iter().map(|lot| lot.id).collect();
        assert_eq!(ids, expected, "{name}: lot ids");
        for g in &layout.lots {
            let what = format!("{name}, lot {}", g.id);
            let lot = &lots.lots[&LotId(g.id)];
            assert_eq!([lot.cell.x, lot.cell.y], g.cell, "{what}: cell");
            assert_eq!(lot.lot_type as i32, g.lot_type, "{what}: type");
            let plot = [
                lot.plot.position.x,
                lot.plot.position.y,
                lot.plot.size.x,
                lot.plot.size.y,
            ];
            for (got, want) in plot.iter().zip(&g.plot) {
                assert!(
                    (got - want).abs() < 1e-3,
                    "{what}: plot {plot:?} vs Godot {:?}",
                    g.plot
                );
            }
            let access = lots.access_points(&roads, LotId(g.id));
            assert_eq!(access.len(), g.access.len(), "{what}: access point count");
            for (got, want) in access.iter().zip(&g.access) {
                assert_eq!(got.lane_id.0, want.lane_id, "{what}: access lane");
                assert!(
                    (got.distance - want.distance).abs() < 1e-3,
                    "{what}: access distance {} vs {}",
                    got.distance,
                    want.distance
                );
                assert_eq!(got.penalty, want.penalty, "{what}: access penalty");
            }
            count += 1;
        }
    }
    assert!(count > 200, "{count} lots");
}
