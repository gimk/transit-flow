//! The street space model (pivot plan Phase 1) on the test level "Carrefour école": the
//! level loads from its file, strip edits rebuild lanes and movements, and level + design
//! round-trip unchanged.

use tf_sim::street::level::{LevelFile, TurnBan, level_ids};
use tf_sim::street::space::{Access, Direction, StripUse};
use tf_sim::street::{Design, DesignCommand, EditError, Street, StreetChange};

const LEVEL: &str = "carrefour-ecole";
/// The junction of the test level.
const JUNCTION: i32 = 0;

fn level() -> LevelFile {
    LevelFile::load(LEVEL).expect("the test level loads")
}

fn street() -> Street {
    Street::from_level(&level()).expect("the test level builds")
}

#[test]
fn the_test_level_is_shipped_and_loads() {
    assert!(level_ids().any(|id| id == LEVEL));
    let level = level();
    assert_eq!(level.name, "Carrefour école");
    assert_eq!((level.nodes.len(), level.segments.len()), (5, 4));
    assert_eq!(level.demand.len(), 4, "four time slices");
    assert!(matches!(
        LevelFile::load("nowhere"),
        Err(tf_sim::street::level::LevelError::Unknown(_))
    ));
}

#[test]
fn the_level_file_round_trips() {
    let level = level();
    let again = LevelFile::from_ron(&level.to_ron()).expect("written RON parses");
    assert_eq!(again, level);
}

#[test]
fn a_bad_level_is_refused() {
    let mut level = level();
    level.segments[0].to = 99;
    assert!(level.validate().is_err());
    let mut level = self::level();
    level.junctions.clear();
    assert!(matches!(
        Street::from_level(&level),
        Err(EditError::Invalid(_))
    ));
}

#[test]
fn lanes_come_from_the_travel_strips_and_stop_at_the_junction() {
    let street = street();
    for segment in [1, 2, 3, 4] {
        assert_eq!(
            street.segment_lanes(segment).count(),
            2,
            "segment {segment}"
        );
    }
    // The junction square spans x 69..81 and y 68.5..81.5 around (75, 75).
    assert_eq!(street.segments[&1].end_trim, 6.0);
    assert_eq!(street.segments[&2].start_trim, 6.0);
    assert_eq!(street.segments[&3].end_trim, 6.5);
    let bike = street
        .segment_lanes(3)
        .find(|lane| lane.access == Access::BICYCLE)
        .expect("contraflow bicycle lane");
    assert_eq!((bike.from_node, bike.to_node), (JUNCTION, 3));
    assert_eq!(bike.width, 1.5);
    assert!((bike.speed_limit - 30.0 / 3.6).abs() < 1e-9);
}

#[test]
fn movements_follow_the_rules_of_a_one_way_street() {
    let street = street();
    assert_eq!(street.movements(JUNCTION).count(), 12);
    let access = |from, to| street.movement_access(JUNCTION, from, to);
    // Cars cannot drive north up the one-way Rue du Marché (segment 3)...
    assert!(!access(1, 3).intersects(Access::CAR));
    assert!(!access(2, 3).intersects(Access::CAR));
    // ...but cyclists can, into the contraflow lane.
    assert_eq!(access(1, 3), Access::BICYCLE);
    assert_eq!(access(4, 3), Access::BICYCLE);
    // Southbound cars go every way.
    for to in [1, 2, 4] {
        assert!(access(3, to).contains(Access::CAR), "3 -> {to}");
    }
    // Conflicts are symmetric and exist (the junction is shared).
    let mut conflicts = 0;
    for movement in street.movements(JUNCTION) {
        for other in movement.conflict_zones.keys() {
            assert!(
                street.lanes[other]
                    .conflict_zones
                    .contains_key(&movement.id)
            );
            conflicts += 1;
        }
    }
    assert!(conflicts > 0);
}

#[test]
fn a_strip_change_rebuilds_lanes_and_movements() {
    let mut street = street();
    let old_lanes: Vec<_> = street.segments[&2].lane_ids.clone();
    let old_movements = street.junctions[&JUNCTION].movement_ids.clone();
    // The parking strip east of the junction becomes an eastbound bicycle lane.
    let change = street
        .apply(&DesignCommand::SetStripUse {
            segment: 2,
            strip: 3,
            use_: StripUse::BikeLane,
            direction: Direction::Forward,
        })
        .expect("the edit is allowed");
    assert_eq!(
        change,
        StreetChange {
            node_ids: vec![JUNCTION],
            segment_ids: vec![2],
        }
    );
    assert_eq!(street.version, 1);
    let lanes: Vec<_> = street.segment_lanes(2).collect();
    assert_eq!(lanes.len(), 3);
    assert!(lanes.iter().all(|lane| !old_lanes.contains(&lane.id)));
    let movements = &street.junctions[&JUNCTION].movement_ids;
    assert!(movements.iter().all(|id| !old_movements.contains(id)));
    assert!(!street.lanes.keys().any(|id| old_lanes.contains(id)));
    // Cyclists from the contraflow lane now turn right into the new bicycle lane.
    let new_bike = lanes
        .iter()
        .find(|lane| lane.access == Access::BICYCLE)
        .expect("new bicycle lane")
        .id;
    let into_bike: Vec<_> = street
        .movements(JUNCTION)
        .filter(|m| m.outgoing[0] == new_bike)
        .collect();
    assert!(!into_bike.is_empty());
    assert!(into_bike.iter().all(|m| m.access == Access::BICYCLE));
}

#[test]
fn reversing_the_one_way_street_reverses_its_movements() {
    let mut street = street();
    for segment in [3, 4] {
        street
            .apply(&DesignCommand::SetStripDirection {
                segment,
                strip: 2,
                direction: Direction::Backward,
            })
            .expect("allowed");
    }
    assert!(
        !street
            .movement_access(JUNCTION, 3, 4)
            .intersects(Access::CAR)
    );
    assert!(street.movement_access(JUNCTION, 4, 3).contains(Access::CAR));
}

#[test]
fn turn_bans_take_classes_off_a_movement() {
    let mut street = street();
    let ban = TurnBan {
        from_segment: 3,
        to_segment: 2,
        access: Access::MOTOR,
    };
    street
        .apply(&DesignCommand::BanTurn {
            node: JUNCTION,
            ban,
        })
        .expect("allowed");
    assert_eq!(street.movement_access(JUNCTION, 3, 2), Access::BICYCLE);
    assert!(
        street
            .apply(&DesignCommand::BanTurn {
                node: JUNCTION,
                ban
            })
            .is_err()
    );
    street
        .apply(&DesignCommand::UnbanTurn {
            node: JUNCTION,
            ban,
        })
        .expect("allowed");
    assert!(street.movement_access(JUNCTION, 3, 2).contains(Access::CAR));
}

#[test]
fn moving_a_boundary_keeps_the_total_width() {
    let mut street = street();
    let width = street.segments[&1].profile.total_width();
    street
        .apply(&DesignCommand::MoveBoundary {
            segment: 1,
            boundary: 1,
            delta: 1.0,
        })
        .expect("allowed");
    let profile = &street.segments[&1].profile;
    assert_eq!(profile.total_width(), width);
    assert_eq!(
        (profile.strips[0].width, profile.strips[1].width),
        (4.0, 2.0)
    );
}

#[test]
fn refused_edits_change_nothing() {
    let mut street = street();
    let before = street.clone();
    let bad = [
        DesignCommand::MoveBoundary {
            segment: 1,
            boundary: 1,
            delta: 2.8,
        },
        DesignCommand::MoveBoundary {
            segment: 1,
            boundary: 0,
            delta: 1.0,
        },
        DesignCommand::SetStripUse {
            segment: 1,
            strip: 3,
            use_: StripUse::CarLane,
            direction: Direction::None,
        },
        DesignCommand::SetStripDirection {
            segment: 1,
            strip: 0,
            direction: Direction::Forward,
        },
        DesignCommand::SetSpeedLimit {
            segment: 1,
            kmh: 42.0,
        },
        DesignCommand::SetSpeedLimit {
            segment: 9,
            kmh: 20.0,
        },
        DesignCommand::BanTurn {
            node: 1,
            ban: TurnBan {
                from_segment: 1,
                to_segment: 2,
                access: Access::CAR,
            },
        },
    ];
    for command in &bad {
        assert!(street.apply(command).is_err(), "{command:?}");
    }
    assert_eq!(street, before);
}

#[test]
fn locked_strips_cannot_change() {
    let mut level = level();
    level.segments[0].profile.strips[1].locked = true;
    let mut street = Street::from_level(&level).expect("builds");
    let result = street.apply(&DesignCommand::SetStripUse {
        segment: 1,
        strip: 1,
        use_: StripUse::BusLane,
        direction: Direction::Backward,
    });
    assert_eq!(
        result,
        Err(EditError::Locked {
            segment: 1,
            strip: 1
        })
    );
}

fn sample_design() -> Design {
    let mut design = Design::new(LEVEL);
    design.commands = vec![
        DesignCommand::SetStripUse {
            segment: 2,
            strip: 3,
            use_: StripUse::BikeLane,
            direction: Direction::Forward,
        },
        DesignCommand::MoveBoundary {
            segment: 1,
            boundary: 1,
            delta: 0.5,
        },
        DesignCommand::SetSpeedLimit {
            segment: 3,
            kmh: 20.0,
        },
        DesignCommand::BanTurn {
            node: JUNCTION,
            ban: TurnBan {
                from_segment: 2,
                to_segment: 4,
                access: Access::MOTOR,
            },
        },
        DesignCommand::SetSign {
            node: JUNCTION,
            segment: 3,
            sign: tf_sim::street::level::Sign::Yield,
        },
    ];
    design
}

#[test]
fn level_and_design_round_trip_identically() {
    let level = level();
    let design = sample_design();
    let text = design.to_json();
    let loaded = Design::from_json(&text).expect("the saved design loads");
    assert_eq!(loaded, design);
    assert_eq!(loaded.to_json(), text, "saving is stable");
    let built = Street::from_level_and_design(&level, &design).expect("replays");
    let rebuilt = Street::from_level_and_design(
        &LevelFile::from_ron(&level.to_ron()).expect("parses"),
        &loaded,
    )
    .expect("replays");
    assert_eq!(rebuilt, built);
    assert_eq!(built.version, design.commands.len() as u64);
}

#[test]
fn a_design_for_another_level_or_version_is_refused() {
    let mut design = sample_design();
    design.level = "elsewhere".into();
    assert!(Street::from_level_and_design(&level(), &design).is_err());
    let text = sample_design()
        .to_json()
        .replace("\"version\": 1", "\"version\": 99");
    assert!(Design::from_json(&text).is_err());
}

#[test]
fn walkable_space_is_the_sidewalks_and_the_corners() {
    let street = street();
    let walkable = street.walkable_polygons();
    assert_eq!(walkable.len(), 4 * 2 + 4);
    // A point on the school sidewalk, and one on the north-east corner.
    assert!(
        walkable
            .iter()
            .any(|p| p.contains(glam::Vec2::new(100.0, 70.0)))
    );
    assert!(
        walkable
            .iter()
            .any(|p| p.contains(glam::Vec2::new(80.0, 70.0)))
    );
    assert!(
        !walkable
            .iter()
            .any(|p| p.contains(glam::Vec2::new(75.0, 75.0)))
    );
}
