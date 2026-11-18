//! Phase 6, work package 2: the design of bicycle space. Strips (lane, track, contraflow),
//! separators and who can cross them, the bicycle box and the leading bicycle green. Each
//! command applies, is refused with its reason when it is invalid (and then changes
//! nothing), replays from JSON and undoes.

use tf_sim::street::design::SignalTemplate;
use tf_sim::street::level::{JunctionControl, LevelFile};
use tf_sim::street::model::Street;
use tf_sim::street::run::{Population, RunOptions, StreetRun};
use tf_sim::street::signals::{Light, MAX_BICYCLE_LEAD_S};
use tf_sim::street::space::{Access, Crossability, Direction, Separator, StripUse};
use tf_sim::street::{Design, DesignCommand};

fn level() -> LevelFile {
    LevelFile::load("carrefour-ecole").expect("the test level loads")
}

fn street_with(commands: &[DesignCommand]) -> Street {
    let mut street = Street::from_level(&level()).unwrap();
    for command in commands {
        street.apply(command).unwrap();
    }
    street
}

fn use_(segment: i32, strip: usize, use_: StripUse, direction: Direction) -> DesignCommand {
    DesignCommand::SetStripUse {
        segment,
        strip,
        use_,
        direction,
    }
}

fn separator(segment: i32, boundary: usize, separator: Separator) -> DesignCommand {
    DesignCommand::SetSeparator {
        segment,
        boundary,
        separator,
    }
}

/// Applies `command` to a copy of `street` that must refuse it: the reason contains
/// `reason` and the street is unchanged.
fn refused(street: &Street, command: &DesignCommand, reason: &str) {
    let mut copy = street.clone();
    match copy.apply(command) {
        Err(error) => {
            let message = error.to_string();
            assert!(message.contains(reason), "{message:?} lacks {reason:?}")
        }
        Ok(_) => panic!("expected a refusal with {reason:?}"),
    }
    assert_eq!(&copy, street, "a refused command changes nothing");
}

fn signals(street: &mut Street) {
    street
        .apply(&DesignCommand::SetJunctionControl {
            node: 0,
            control: JunctionControl::Signals,
            template: Some(SignalTemplate::TwoPhase { green_s: 20.0 }),
        })
        .unwrap();
}

// ---- Level 1: the contraflow lane of Rue du Marché ----

#[test]
fn the_contraflow_lane_of_the_market_street_runs_north_and_connects_at_the_junction() {
    let street = Street::from_level(&level()).unwrap();
    // Segment 3 runs from node 3 (north) to node 0 (the junction): cars Forward (south),
    // the bicycle lane (strip 1) Backward (north), on the east side.
    let lane = street
        .segment_lanes(3)
        .find(|l| l.strip == Some(1))
        .expect("segment 3 has its bicycle lane");
    assert_eq!(lane.access, Access::BICYCLE);
    assert_eq!(
        (lane.from_node, lane.to_node),
        (0, 3),
        "it leaves the junction"
    );
    let line = lane.polyline();
    let (first, last) = (line[0], line[line.len() - 1]);
    assert!(
        first.y > last.y,
        "northbound: y decreases ({first} -> {last})"
    );
    assert!(
        (first.x - 77.75).abs() < 0.01,
        "east of the carriageway: {first}"
    );
    let car = street
        .segment_lanes(3)
        .find(|l| l.strip == Some(2))
        .unwrap();
    assert_eq!((car.from_node, car.to_node), (3, 0), "cars go south");
    // The lane that arrives from the south (segment 4, northbound) connects straight
    // into it through the junction.
    let arriving = street
        .segment_lanes(4)
        .find(|l| l.strip == Some(1))
        .expect("segment 4 has a bicycle lane");
    assert_eq!((arriving.from_node, arriving.to_node), (4, 0));
    let straight_on = street
        .movements(0)
        .find(|m| m.feeder == Some(arriving.id) && m.outgoing == vec![lane.id]);
    let straight_on = straight_on.expect("the bicycle lane continues through the junction");
    assert!(straight_on.access.contains(Access::BICYCLE));
    // The cars' southbound lane also leaves cyclists a way: no bicycle movement is cut.
    assert!(
        street
            .movements(0)
            .any(|m| m.feeder == Some(arriving.id) && m.access.intersects(Access::BICYCLE)),
    );
}

// ---- Strips ----

#[test]
fn a_parking_strip_becomes_a_painted_lane_with_paint_on_the_car_side() {
    let base = Street::from_level(&level()).unwrap();
    let mut street = base.clone();
    let before = street.segment_lanes(1).count();
    let change = street
        .apply(&use_(1, 3, StripUse::BikeLane, Direction::Forward))
        .unwrap();
    assert_eq!(change.segment_ids, vec![1]);
    assert_eq!(street.segment_lanes(1).count(), before + 1);
    let lane = street
        .segment_lanes(1)
        .find(|l| l.strip == Some(3))
        .unwrap();
    assert_eq!(lane.access, Access::BICYCLE);
    assert_eq!((lane.from_node, lane.to_node), (1, 0));
    // The line between the car lane and the new lane got paint; the kerb stays.
    let strips = &street.segments[&1].profile.strips;
    assert_eq!(strips[2].right, Separator::Paint);
    assert_eq!(strips[3].left, Separator::Paint);
    assert_eq!(strips[4].left, Separator::Kerb);
    // Back to parking: the lane goes.
    street
        .apply(&use_(1, 3, StripUse::Parking, Direction::None))
        .unwrap();
    assert_eq!(street.segment_lanes(1).count(), before);
}

#[test]
fn a_track_gets_a_kerb_and_a_lane_a_car_lane_becomes_a_lane() {
    let street = street_with(&[use_(1, 3, StripUse::BikeTrack, Direction::Forward)]);
    let strips = &street.segments[&1].profile.strips;
    assert_eq!(strips[2].right, Separator::Kerb);
    assert_eq!(strips[3].left, Separator::Kerb);
    // A car lane (the westbound one, strip 1) turned into a bicycle lane, same direction.
    let street = street_with(&[use_(1, 1, StripUse::BikeLane, Direction::Backward)]);
    let lane = street
        .segment_lanes(1)
        .find(|l| l.strip == Some(1))
        .unwrap();
    assert_eq!(lane.access, Access::BICYCLE);
    assert_eq!((lane.from_node, lane.to_node), (0, 1));
}

#[test]
fn bicycle_strips_are_refused_when_too_narrow() {
    let street = street_with(&[DesignCommand::MoveBoundary {
        segment: 1,
        boundary: 3,
        delta: 0.4,
    }]);
    // Strip 3 is 1.6 m now: a lane fits, a track does not.
    refused(
        &street,
        &use_(1, 3, StripUse::BikeTrack, Direction::Forward),
        "at least 2 m",
    );
    let mut with_lane = street.clone();
    with_lane
        .apply(&use_(1, 3, StripUse::BikeLane, Direction::Forward))
        .unwrap();
    // A lane cannot be squeezed under 1.5 m (parking is on its left: boundary 3).
    refused(
        &with_lane,
        &DesignCommand::MoveBoundary {
            segment: 1,
            boundary: 3,
            delta: 0.3,
        },
        "at least 1.5 m",
    );
    // Widening it is fine, and so is a squeeze that stays above the minimum.
    let mut wider = with_lane.clone();
    wider
        .apply(&DesignCommand::MoveBoundary {
            segment: 1,
            boundary: 3,
            delta: -0.05,
        })
        .unwrap();
}

#[test]
fn a_bicycle_strip_keeps_to_the_right_of_its_own_direction() {
    let street = Street::from_level(&level()).unwrap();
    // Segment 1 is two-way: Forward traffic is on the right (strips 2 and 3).
    refused(
        &street,
        &use_(1, 3, StripUse::BikeLane, Direction::Backward),
        "right of the car lanes",
    );
    let mut street = street;
    street
        .apply(&use_(1, 3, StripUse::BikeLane, Direction::Forward))
        .unwrap();
    refused(
        &street,
        &DesignCommand::SetStripDirection {
            segment: 1,
            strip: 3,
            direction: Direction::Backward,
        },
        "right of the car lanes",
    );
}

#[test]
fn contraflow_needs_a_one_way_street_at_30_or_less() {
    let street = Street::from_level(&level()).unwrap();
    // The level's own contraflow lane stands at 30 km/h and 20 km/h but not at 50.
    for segment in [3, 4] {
        refused(
            &street,
            &DesignCommand::SetSpeedLimit { segment, kmh: 50.0 },
            "30 km/h or less",
        );
    }
    let mut street = street;
    street
        .apply(&DesignCommand::SetSpeedLimit {
            segment: 3,
            kmh: 20.0,
        })
        .unwrap();
    // Make segment 1 one-way (both car lanes Forward); a lane against the traffic on the
    // left (the old sidewalk, strip 0) is contraflow, allowed at 30 km/h.
    street
        .apply(&DesignCommand::SetStripDirection {
            segment: 1,
            strip: 1,
            direction: Direction::Forward,
        })
        .unwrap();
    refused(
        &street,
        &use_(1, 3, StripUse::BikeLane, Direction::Backward),
        "right of the car lanes",
    );
    street
        .apply(&use_(1, 0, StripUse::BikeLane, Direction::Backward))
        .unwrap();
    refused(
        &street,
        &DesignCommand::SetSpeedLimit {
            segment: 1,
            kmh: 50.0,
        },
        "30 km/h or less",
    );
    // A with-the-traffic lane is fine at 50, and so is a lane in a two-way street.
    let mut two_way = Street::from_level(&level()).unwrap();
    two_way
        .apply(&DesignCommand::SetSpeedLimit {
            segment: 1,
            kmh: 50.0,
        })
        .unwrap();
    two_way
        .apply(&use_(1, 3, StripUse::BikeLane, Direction::Forward))
        .unwrap();
}

#[test]
fn a_track_next_to_a_car_lane_keeps_a_physical_separator() {
    let mut street = street_with(&[use_(1, 3, StripUse::BikeTrack, Direction::Forward)]);
    refused(
        &street,
        &separator(1, 3, Separator::Paint),
        "kerb, bollards or a planter",
    );
    refused(
        &street,
        &separator(1, 3, Separator::None),
        "kerb, bollards or a planter",
    );
    refused(&street, &separator(1, 0, Separator::Kerb), "building line");
    refused(&street, &separator(1, 5, Separator::Kerb), "building line");
    refused(
        &street,
        &separator(9, 1, Separator::Kerb),
        "unknown segment",
    );
    street.apply(&separator(1, 3, Separator::Bollards)).unwrap();
    let strips = &street.segments[&1].profile.strips;
    assert_eq!(strips[2].right, Separator::Bollards);
    assert_eq!(strips[3].left, Separator::Bollards);
    // A painted lane may have any separator.
    let mut lane = street_with(&[use_(1, 3, StripUse::BikeLane, Direction::Forward)]);
    lane.apply(&separator(1, 3, Separator::None)).unwrap();
}

// ---- Who can cross a separator ----

#[test]
fn separators_decide_who_can_cross() {
    let street = Street::from_level(&level()).unwrap();
    let [ped, bike, car] = [Access::PEDESTRIAN, Access::BICYCLE, Access::CAR];
    // Segment 3: sidewalk | kerb | bicycle lane | paint | car lane | none | parking | kerb |
    // sidewalk.
    assert_eq!(
        street.boundary_crossability(3, 1, bike),
        Crossability::Blocked
    );
    assert_eq!(
        street.boundary_crossability(3, 1, car),
        Crossability::Blocked
    );
    assert_eq!(street.boundary_crossability(3, 1, ped), Crossability::Free);
    assert_eq!(
        street.boundary_crossability(3, 2, bike),
        Crossability::Paint
    );
    assert_eq!(street.boundary_crossability(3, 2, car), Crossability::Paint);
    assert!(
        street.crossable_by(3, 2, car),
        "a careless driver can cross paint"
    );
    assert_eq!(street.boundary_crossability(3, 3, car), Crossability::Free);
    // Building lines and unknown places are walls.
    assert!(!street.crossable_by(3, 0, ped));
    assert!(!street.crossable_by(3, 5, ped));
    assert!(!street.crossable_by(99, 1, car));
    // Bollards stop cars, not bicycles; a planter stops all.
    let mut street = street;
    street.apply(&separator(3, 2, Separator::Bollards)).unwrap();
    assert!(!street.crossable_by(3, 2, car));
    assert!(street.crossable_by(3, 2, bike));
    assert!(street.crossable_by(3, 2, ped));
    assert!(
        !street.crossable_by(3, 2, bike | car),
        "the hardest class counts"
    );
    street.apply(&separator(3, 2, Separator::Planter)).unwrap();
    assert!(!street.crossable_by(3, 2, bike));
    assert!(!street.crossable_by(3, 2, ped));
}

#[test]
fn a_buffer_strip_blocks_wheels_between_two_strips() {
    let mut street = Street::from_level(&level()).unwrap();
    // Segment 3 from the car lane (strip 2) to the bicycle lane (strip 1): paint only.
    assert!(street.crossable_between(3, 2, 1, Access::CAR));
    // ... to the sidewalk (strip 0): the kerb stops it.
    assert!(!street.crossable_between(3, 2, 0, Access::CAR));
    assert!(street.crossable_between(3, 2, 0, Access::PEDESTRIAN));
    // Over the parking strip: free for a car.
    assert!(
        !street.crossable_between(3, 2, 4, Access::CAR),
        "the far kerb"
    );
    assert!(street.crossable_between(3, 1, 3, Access::BICYCLE));
    // Planting in place of the eastbound lane of segment 1 is a buffer between the
    // westbound lane (strip 1) and the parking (strip 3): paint and no separator on
    // either side, but wheels cannot cross it; walkers can.
    assert!(street.crossable_between(1, 1, 3, Access::CAR));
    street
        .apply(&use_(1, 2, StripUse::Planting, Direction::None))
        .unwrap();
    assert!(!street.crossable_between(1, 1, 3, Access::CAR));
    assert!(!street.crossable_between(1, 1, 3, Access::BICYCLE));
    assert!(street.crossable_between(1, 1, 3, Access::PEDESTRIAN));
    assert!(!street.crossable_between(1, 1, 9, Access::PEDESTRIAN));
}

// ---- The bicycle box ----

#[test]
fn a_bicycle_box_moves_the_car_stop_line_back_and_keeps_the_cyclist_line() {
    let base = Street::from_level(&level()).unwrap();
    let car_lane = |street: &Street| {
        street
            .segment_lanes(1)
            .find(|l| l.strip == Some(2))
            .unwrap()
            .id
    };
    let mut street = base.clone();
    // Without signals: refused.
    refused(
        &street,
        &DesignCommand::AddBicycleBox {
            node: 0,
            segment: 1,
            depth: 4.0,
        },
        "signalled",
    );
    signals(&mut street);
    let before = street.clone();
    let id = car_lane(&street);
    let length = street.lanes[&id].length;
    assert_eq!(street.car_stop_distance(id), length, "no box yet");
    let change = street
        .apply(&DesignCommand::AddBicycleBox {
            node: 0,
            segment: 1,
            depth: 4.0,
        })
        .unwrap();
    assert_eq!((change.node_ids, change.segment_ids), (vec![0], vec![1]));
    assert_eq!(street.car_stop_distance(id), length - 4.0);
    assert_eq!(street.bicycle_stop_distance(id), length);
    assert_eq!(street.bicycle_box(0, 1).unwrap().depth, 4.0);
    // Other approaches and the lane that leaves the junction are not touched.
    let other = street
        .segment_lanes(2)
        .find(|l| l.strip == Some(2))
        .unwrap()
        .id;
    assert_eq!(street.car_stop_distance(other), street.lanes[&other].length);
    let leaving = street
        .segment_lanes(1)
        .find(|l| l.strip == Some(1))
        .unwrap()
        .id;
    assert_eq!(
        street.car_stop_distance(leaving),
        street.lanes[&leaving].length
    );
    // The box goes away again.
    street
        .apply(&DesignCommand::RemoveBicycleBox {
            node: 0,
            segment: 1,
        })
        .unwrap();
    assert_eq!(street.car_stop_distance(id), length);
    assert_eq!(street.bicycle_boxes, before.bicycle_boxes);
}

#[test]
fn bicycle_box_commands_are_validated() {
    let mut street = Street::from_level(&level()).unwrap();
    signals(&mut street);
    let add = |segment, depth| DesignCommand::AddBicycleBox {
        node: 0,
        segment,
        depth,
    };
    refused(&street, &add(1, 2.0), "3 to 5 m");
    refused(&street, &add(1, 6.0), "3 to 5 m");
    refused(&street, &add(1, f32::NAN), "3 to 5 m");
    refused(&street, &add(9, 4.0), "not an arm");
    street.apply(&add(1, 3.0)).unwrap();
    refused(&street, &add(1, 4.0), "already");
    refused(
        &street,
        &DesignCommand::RemoveBicycleBox {
            node: 0,
            segment: 2,
        },
        "no bicycle box",
    );
    refused(
        &street,
        &DesignCommand::AddBicycleBox {
            node: 7,
            segment: 1,
            depth: 4.0,
        },
        "no junction",
    );
    // The signals cannot go while a box stands on them.
    refused(
        &street,
        &DesignCommand::SetJunctionControl {
            node: 0,
            control: JunctionControl::PriorityToRight,
            template: None,
        },
        "remove the box first",
    );
    // An approach with no bicycle lane and no car lane has no box: make segment 2's two
    // car lanes bicycle lanes.
    let mut bare = Street::from_level(&level()).unwrap();
    signals(&mut bare);
    bare.apply(&use_(2, 1, StripUse::BikeLane, Direction::Backward))
        .unwrap();
    bare.apply(&use_(2, 2, StripUse::BikeLane, Direction::Forward))
        .unwrap();
    refused(
        &bare,
        &DesignCommand::AddBicycleBox {
            node: 0,
            segment: 2,
            depth: 4.0,
        },
        "for bicycles and for cars",
    );
}

// ---- The leading bicycle green ----

#[test]
fn a_leading_bicycle_green_needs_a_plan_and_a_short_lead() {
    let mut street = Street::from_level(&level()).unwrap();
    let lead = |lead_s| DesignCommand::SetBicycleLead { node: 0, lead_s };
    refused(&street, &lead(4.0), "signal plan");
    signals(&mut street);
    refused(
        &street,
        &lead(MAX_BICYCLE_LEAD_S + 1.0),
        "leading bicycle green",
    );
    refused(&street, &lead(-1.0), "leading bicycle green");
    refused(
        &street,
        &DesignCommand::SetBicycleLead {
            node: 5,
            lead_s: 4.0,
        },
        "no junction",
    );
    street.apply(&lead(4.0)).unwrap();
    let plan = street.junctions[&0].signal_plan.clone().unwrap();
    assert_eq!(plan.bicycle_lead_s, 4.0);
    // The first approach of the first phase: cyclists go 4 s before cars.
    let approach = plan.phases[0].green[0];
    assert_eq!(plan.bicycle_light(approach, 1.0), Light::Green(1.0));
    assert!(matches!(plan.light(approach, 1.0), Light::Red(_)));
    assert_eq!(plan.light(approach, 5.0), Light::Green(1.0));
    street.apply(&lead(0.0)).unwrap();
    let plan = street.junctions[&0].signal_plan.clone().unwrap();
    assert_eq!(plan.light(approach, 1.0), plan.bicycle_light(approach, 1.0));
    // A new template starts with no lead.
    street.apply(&lead(4.0)).unwrap();
    signals(&mut street);
    assert_eq!(
        street.junctions[&0]
            .signal_plan
            .as_ref()
            .unwrap()
            .bicycle_lead_s,
        0.0
    );
}

// ---- Replay, JSON and undo ----

fn bicycle_design() -> Design {
    let level = level();
    let mut design = Design::new(&level.id);
    design.commands = vec![
        use_(1, 3, StripUse::BikeTrack, Direction::Forward),
        separator(1, 3, Separator::Bollards),
        use_(2, 1, StripUse::BikeLane, Direction::Backward),
        DesignCommand::SetJunctionControl {
            node: 0,
            control: JunctionControl::Signals,
            template: Some(SignalTemplate::TwoPhase { green_s: 20.0 }),
        },
        DesignCommand::AddBicycleBox {
            node: 0,
            segment: 1,
            depth: 4.0,
        },
        DesignCommand::SetBicycleLead {
            node: 0,
            lead_s: 4.0,
        },
        DesignCommand::RemoveBicycleBox {
            node: 0,
            segment: 1,
        },
        DesignCommand::AddBicycleBox {
            node: 0,
            segment: 3,
            depth: 3.5,
        },
    ];
    design
}

#[test]
fn bicycle_commands_replay_from_json_and_undo() {
    let level = level();
    let design = bicycle_design();
    let parsed = Design::from_json(&design.to_json()).unwrap();
    assert_eq!(parsed, design);
    let full = Street::from_level_and_design(&level, &parsed).unwrap();
    assert_eq!(
        full,
        Street::from_level_and_design(&level, &design).unwrap()
    );
    assert_eq!(full.bicycle_boxes.len(), 1);
    assert!(full.bicycle_box(0, 3).is_some());
    // Undo each command in turn: the street before it, then the command again, gives the
    // street after it.
    for n in 1..=design.commands.len() {
        let mut shorter = design.clone();
        shorter.commands.truncate(n - 1);
        let mut street = Street::from_level_and_design(&level, &shorter).unwrap();
        let mut longer = design.clone();
        longer.commands.truncate(n);
        street.apply(&design.commands[n - 1]).unwrap();
        assert_eq!(
            street,
            Street::from_level_and_design(&level, &longer).unwrap(),
            "{n}"
        );
    }
}

#[test]
fn the_empty_design_and_a_bicycle_design_replay_identically_in_a_run() {
    let level = level();
    let options = |seed| RunOptions {
        seed,
        population: Population::Mixed,
        demand_scale: 1.0,
    };
    let hash = |design: &Design, seed| {
        let mut run = StreetRun::new(&level, design, options(seed)).expect("the design builds");
        run.run_to_end();
        run.log_hash()
    };
    let design = bicycle_design();
    assert_eq!(hash(&design, 7), hash(&design, 7));
    let empty = Design::new(&level.id);
    assert_eq!(hash(&empty, 7), hash(&empty, 7));
}
