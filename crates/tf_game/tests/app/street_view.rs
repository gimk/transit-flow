//! The street game's level view (pivot plan Phase 1): loading a level draws it from its
//! data and frames the camera on it; a strip edit redraws it; unloading clears it.

use crate::common::Harness;
use bevy::prelude::*;
use tf_game::camera::MapCamera;
use tf_game::level::{LoadedLevel, PX_PER_M};
use tf_game::views::street::{
    StreetShape, ZEBRA_BAR_WIDTH_M, build_layers, carriageway, zebra_bars,
};
use tf_sim::street::DesignCommand;
use tf_sim::street::level::{CrossingDef, CrossingKind};
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

/// The area a polygon covers, m².
fn polygon_area(points: &[Vec2]) -> f32 {
    let mut twice = 0.0;
    for i in 0..points.len() {
        twice += points[i].perp_dot(points[(i + 1) % points.len()]);
    }
    (twice * 0.5).abs()
}

#[test]
fn the_zebra_crossings_are_drawn() {
    let mut street = LoadedLevel::load("carrefour-ecole").expect("loads");
    assert_eq!(street.crossings.len(), 4, "one on each arm of the junction");
    let bars = zebra_bars(&street);
    assert_eq!(bars.len(), 12, "three 0.5 m bars in each 3 m crossing");
    // A bar paints the carriageway of its own street: 8 m on the two-way street, 7 m on
    // the one-way one with its bicycle lane.
    let widths: Vec<f32> = street
        .segments
        .values()
        .filter_map(|segment| carriageway(&segment.profile))
        .map(|(left, right)| right - left)
        .collect();
    for bar in &bars {
        let area = polygon_area(bar);
        assert!(
            widths
                .iter()
                .any(|width| (area - width * ZEBRA_BAR_WIDTH_M).abs() < 0.05),
            "a bar paints 0.5 m of the carriageway, got {area} m²"
        );
    }
    // The markings layer carries them: a fifth crossing adds its three bars.
    let before = build_layers(&street)[2].0.triangle_count();
    street
        .apply(&DesignCommand::AddCrossing {
            crossing: CrossingDef {
                kind: CrossingKind::Zebra,
                segment: 1,
                at: 40.0,
                width: 3.0,
            },
        })
        .expect("fits between the junction and the other crossing");
    let after = build_layers(&street)[2].0.triangle_count();
    assert!(after > before, "the markings gained the bars");
    assert_eq!(zebra_bars(&street).len(), bars.len() + 3);
}
