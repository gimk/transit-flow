//! The street game's level view (pivot plan Phase 1): loading a level draws it from its
//! data and frames the camera on it; a strip edit redraws it; unloading clears it.

mod common;

use bevy::prelude::*;
use common::Harness;
use tf_game::camera::MapCamera;
use tf_game::level::{LoadedLevel, PX_PER_M};
use tf_game::views::street::{StreetShape, build_layers};
use tf_sim::street::DesignCommand;
use tf_sim::street::space::{Direction, StripUse};

fn shape_count(harness: &mut Harness) -> usize {
    harness
        .app
        .world_mut()
        .query_filtered::<(), With<StreetShape>>()
        .iter(harness.app.world())
        .count()
}

fn camera(harness: &mut Harness) -> MapCamera {
    harness
        .app
        .world_mut()
        .query::<&MapCamera>()
        .single(harness.app.world())
        .expect("one camera")
        .clone()
}

#[test]
fn a_loaded_level_is_drawn_and_framed() {
    let mut harness = Harness::new();
    harness.app.world_mut().spawn(MapCamera::default());
    harness.frames(1);
    assert_eq!(shape_count(&mut harness), 0, "no level, nothing drawn");

    let street = LoadedLevel::load("carrefour-ecole").expect("loads");
    let center = street.bounds.center() * PX_PER_M;
    harness.app.world_mut().resource_mut::<LoadedLevel>().street = Some(street);
    harness.frames(2);
    assert_eq!(
        shape_count(&mut harness),
        4,
        "ground, surfaces, markings, things"
    );
    let camera = camera(&mut harness);
    assert_eq!((camera.center, camera.zoom), (center, 0.5));

    harness.app.world_mut().resource_mut::<LoadedLevel>().street = None;
    harness.frames(2);
    assert_eq!(shape_count(&mut harness), 0);
}

#[test]
fn a_strip_edit_changes_the_drawing() {
    let mut street = LoadedLevel::load("carrefour-ecole").expect("loads");
    let before = build_layers(&street).map(|(shape, _)| shape.triangle_count());
    street
        .apply(&DesignCommand::SetStripUse {
            segment: 2,
            strip: 3,
            use_: StripUse::BikeLane,
            direction: Direction::Forward,
        })
        .expect("allowed");
    let after = build_layers(&street).map(|(shape, _)| shape.triangle_count());
    // Same strips, so the same surfaces; one more lane gives more arrows and movements.
    assert_eq!(before[1], after[1]);
    assert!(after[2] > before[2]);
}
