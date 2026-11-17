//! The static map views (M9.1): they build from the sim, and a road edit rebuilds only
//! the segments (and node discs) its change message names.

use bevy::prelude::*;
use glam::Vec2;
use tf_game::plugin::GamePlugin;
use tf_game::runner::SimHandle;
use tf_game::views::ViewsPlugin;
use tf_game::views::externals::ExternalViews;
use tf_game::views::lots::LotViews;
use tf_game::views::roads::RoadViews;
use tf_game::views::zones::ZoneViews;
use tf_sim::debug::scenarios;
use tf_sim::ids::SegmentId;
use tf_sim::roads::RoadType;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::input::InputPlugin,
    ))
    .init_asset::<Mesh>()
    .init_asset::<ColorMaterial>()
    .add_plugins((GamePlugin { threaded: false }, ViewsPlugin));
    app.update();
    app
}

fn segment_ids(app: &App) -> Vec<SegmentId> {
    let mut ids: Vec<SegmentId> = app.world().resource::<RoadViews>().segment_ids().collect();
    ids.sort();
    ids
}

fn road(from: Vec2, to: Vec2) -> impl FnOnce(&mut tf_sim::Simulation, &mut tf_sim::events::Outbox) {
    move |sim, out| {
        sim.roads.add_road(
            from,
            to,
            RoadType::load_by_id("local_2lane").unwrap(),
            0,
            out,
        );
    }
}

#[test]
fn the_empty_map_shows_its_highway_and_the_first_road_shows_up() {
    let mut app = app();
    let sim_segments = app.world_mut().resource_mut::<SimHandle>().roads();
    assert!(!sim_segments.roads.segments.is_empty());
    assert_eq!(
        segment_ids(&app).len(),
        sim_segments.roads.segments.len(),
        "a view per segment"
    );
    let views = app.world().resource::<RoadViews>();
    for id in sim_segments.roads.segments.keys() {
        assert!(!views.segment_entities(*id).is_empty());
    }
    assert!(
        !app.world()
            .resource::<ExternalViews>()
            .entities()
            .is_empty()
    );
}

#[test]
fn an_edit_rebuilds_only_the_segments_it_changed() {
    let mut app = app();
    let before_ids = segment_ids(&app);
    let before_entities: Vec<(SegmentId, Vec<Entity>)> = before_ids
        .iter()
        .map(|&id| {
            (
                id,
                app.world()
                    .resource::<RoadViews>()
                    .segment_entities(id)
                    .to_vec(),
            )
        })
        .collect();
    let built_before = app.world().resource::<RoadViews>().segments_built;

    // A road away from the highway: one new segment, nothing else touched.
    app.world_mut()
        .resource_mut::<SimHandle>()
        .get_mut()
        .call(road(Vec2::new(1024.0, 1024.0), Vec2::new(1408.0, 1024.0)));
    app.update();
    let after_ids = segment_ids(&app);
    let new: Vec<SegmentId> = after_ids
        .iter()
        .filter(|id| !before_ids.contains(id))
        .copied()
        .collect();
    assert_eq!(new.len(), 1, "one new segment: {new:?}");
    let views = app.world().resource::<RoadViews>();
    assert_eq!(
        views.segments_built - built_before,
        1,
        "only that one is built"
    );
    assert!(!views.segment_entities(new[0]).is_empty());
    for (id, entities) in &before_entities {
        assert_eq!(
            views.segment_entities(*id),
            entities.as_slice(),
            "{id} kept"
        );
        for e in entities {
            assert!(app.world().get_entity(*e).is_ok());
        }
    }

    // A road across it splits it: the old segment's entities go, new ones come, the rest
    // (the highway) stay.
    let nodes_before = app.world().resource::<RoadViews>().nodes_built;
    let old_entities = app
        .world()
        .resource::<RoadViews>()
        .segment_entities(new[0])
        .to_vec();
    app.world_mut()
        .resource_mut::<SimHandle>()
        .get_mut()
        .call(road(Vec2::new(1216.0, 832.0), Vec2::new(1216.0, 1216.0)));
    app.update();
    let views = app.world().resource::<RoadViews>();
    for e in &old_entities {
        assert!(
            app.world().get_entity(*e).is_err(),
            "the split segment's entities are despawned"
        );
    }
    for (id, entities) in &before_entities {
        assert_eq!(
            views.segment_entities(*id),
            entities.as_slice(),
            "{id} untouched"
        );
    }
    assert!(
        views.nodes_built > nodes_before,
        "the new junction got its discs"
    );

    // Removing a segment despawns its entities (and only those).
    let victim = *segment_ids(&app).last().unwrap();
    let victim_entities = app
        .world()
        .resource::<RoadViews>()
        .segment_entities(victim)
        .to_vec();
    let built = app.world().resource::<RoadViews>().segments_built;
    app.world_mut()
        .resource_mut::<SimHandle>()
        .get_mut()
        .call(|sim, out| {
            sim.roads.remove_segment(victim, out);
        });
    app.update();
    let views = app.world().resource::<RoadViews>();
    assert!(views.segment_entities(victim).is_empty());
    for e in victim_entities {
        assert!(app.world().get_entity(e).is_err());
    }
    // Neighbours the removal merged may be rebuilt (their ids are in the change); the
    // highway, far away, is not.
    for (id, entities) in &before_entities {
        assert_eq!(views.segment_entities(*id), entities.as_slice());
    }
    assert!(
        views.segments_built - built <= 4,
        "only the neighbours were rebuilt"
    );
}

#[test]
fn the_town_builds_its_lots_and_a_road_edit_redraws_few_of_them() {
    let mut app = app();
    app.world_mut().resource_mut::<SimHandle>().replace({
        let mut sim = tf_sim::Simulation::new();
        scenarios::build_town(&mut sim);
        sim
    });
    app.update();
    app.update();
    let sim_lots = app
        .world_mut()
        .resource_mut::<SimHandle>()
        .get_mut()
        .lots
        .lots
        .len();
    assert!(sim_lots > 20);
    assert_eq!(app.world().resource::<LotViews>().len(), sim_lots);
    assert!(
        app.world().resource::<ZoneViews>().len() < sim_lots,
        "zones draw only cells without a building"
    );
    let built = app.world().resource::<LotViews>().built;
    // A short road far from the town's lots changes none of them.
    app.world_mut()
        .resource_mut::<SimHandle>()
        .get_mut()
        .call(road(Vec2::new(256.0, 256.0), Vec2::new(512.0, 256.0)));
    app.update();
    assert_eq!(app.world().resource::<LotViews>().built, built);
}
