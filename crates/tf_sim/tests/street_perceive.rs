//! Perception (pivot plan §5.7 additions 2, 3 and 7, Phase 6 work package 3): the field of
//! view, the gaze a task sets, blind spots, surprise and hearing. The rules are pure
//! functions of numbers, so they are set up here without a run; the effect on a whole
//! run (careful drivers still never touch) is in the other `street_*` tests.

use glam::Vec2;
use tf_sim::street::level::Mode;
use tf_sim::street::run::drive::DT;
use tf_sim::street::run::perceive::{
    self, DriverTask, Focus, Looker, WalkerTask, driver_table, in_blind_spot, listener_hears,
    reaction_s, sound_level, walker_table,
};

const TICKS: u64 = 400;

fn driver(task: DriverTask, mode: Mode, id: u64) -> Looker<'static> {
    Looker {
        mode,
        eye: Vec2::ZERO,
        dir: Vec2::X,
        table: driver_table(task),
        seed: perceive::gaze_seed(1, id as i32),
        fov: 1.0,
        alternate: false,
    }
}

/// Ticks from a thing coming into view until a looker notices it (the delay included),
/// from tick `start`.
fn ticks_to_notice(
    looker: &Looker,
    who: u64,
    target: Vec2,
    start: u64,
    place: impl Fn(Focus, u64) -> Vec2 + Copy,
) -> Option<u64> {
    (0..TICKS).find(|k| {
        looker
            .notices(who, target, start + k, place)
            .is_some_and(|n| *k as f64 * DT >= n.delay_s)
    })
}

fn none(_: Focus, _: u64) -> Vec2 {
    Vec2::ZERO
}

fn mean_wait(task: DriverTask, target: Vec2, place: impl Fn(Focus, u64) -> Vec2 + Copy) -> f64 {
    let mut total = 0.0;
    for id in 0..200 {
        let looker = driver(task, Mode::Car, id);
        // A cap of TICKS for a thing never noticed.
        let k = ticks_to_notice(&looker, id + 1000, target, id * 13, place).unwrap_or(TICKS);
        total += k as f64;
    }
    total / 200.0
}

#[test]
fn a_walker_straight_ahead_is_noticed_sooner_than_one_at_70_degrees() {
    // Looking ahead all the time: the walker at the centre is noticed at once, the one at
    // 70° (cos 0.34, the far periphery) later and sometimes not.
    let ahead = Vec2::new(60.0, 0.0); // beyond SURE_RANGE_M
    let aside = Vec2::new(20.52, 56.37);
    let sure = (0..200u64).all(|id| {
        let looker = driver(DriverTask::Queue, Mode::Car, id);
        looker
            .notices(id, ahead, id * 7, none)
            .is_some_and(|n| n.delay_s == 0.0)
    });
    assert!(sure, "a thing at the centre is noticed at once, every time");
    let mut noticed = 0;
    let mut delay = 0.0;
    for id in 0..200u64 {
        let looker = driver(DriverTask::Queue, Mode::Car, id);
        if let Some(n) = looker.notices(id, aside, id * 7, none) {
            noticed += 1;
            delay += n.delay_s;
        }
    }
    assert!(
        noticed < 200,
        "the far periphery misses some: {noticed}/200"
    );
    assert!(noticed > 0, "but not all: {noticed}/200");
    assert!(delay / f64::from(noticed) > 0.0, "and notices them late");
    let wait_ahead = mean_wait(DriverTask::Queue, ahead, none);
    let wait_aside = mean_wait(DriverTask::Queue, aside, none);
    assert!(
        wait_aside > wait_ahead + 5.0,
        "waits: ahead {wait_ahead} ticks, 70 degrees {wait_aside}"
    );
}

#[test]
fn nothing_behind_is_seen() {
    let behind = Vec2::new(-15.0, 2.0);
    for task in [
        DriverTask::Cruise,
        DriverTask::Queue,
        DriverTask::ZebraScan,
        DriverTask::LeftTurn,
    ] {
        for id in 0..50 {
            let looker = driver(task, Mode::Car, id);
            for tick in 0..100 {
                // Even a glance at a place behind (the places a caller gives) is only a
                // gaze: what is more than 90° from it is not seen, and every gaze of a
                // driver is within the half plane ahead (the places here are ahead).
                let n = looker.notices(id, behind, tick, |_, _| Vec2::new(1.0, 0.0));
                assert!(n.is_none(), "{task:?}: saw what is behind, at tick {tick}");
            }
        }
    }
}

#[test]
fn a_left_turning_driver_notices_a_walker_on_the_exit_zebra_later_than_a_straight_one() {
    // The zebra is straight ahead for the driver going on, and about 65° to the left of
    // the heading for the one who turns left: left is -perp in this frame (y down).
    let straight = Vec2::new(15.0, 0.0);
    let exit = Vec2::new(6.3, -13.6);
    let place = move |focus: Focus, _: u64| {
        if focus == Focus::Exit {
            exit
        } else {
            Vec2::ZERO
        }
    };
    let going_on = mean_wait(DriverTask::Cruise, straight, place);
    let turning = mean_wait(DriverTask::LeftTurn, exit, place);
    assert!(
        turning > going_on + 1.0,
        "straight {going_on} ticks, left turn {turning} ticks"
    );
}

#[test]
fn a_right_turning_driver_looks_right_first() {
    // Priority to the right: the glances at the traffic from the right are more than the
    // ones at the exit.
    let table = driver_table(DriverTask::RightTurn);
    let weight = |f: Focus| table.iter().find(|(x, _)| *x == f).map_or(0.0, |(_, w)| *w);
    assert!(weight(Focus::CrossRight) > weight(Focus::Exit));
    assert!(weight(Focus::CrossRight) > weight(Focus::CrossLeft));
}

#[test]
fn a_van_driver_does_not_see_a_body_in_its_right_blind_spot_without_a_shoulder_check() {
    // The right of the heading is the perpendicular (y down). A cyclist is 3 m to the
    // right and half a metre ahead of the eye: 80° to the right.
    let cyclist = Vec2::new(0.5, 3.0);
    assert!(in_blind_spot(Mode::Van, Vec2::X, cyclist));
    assert!(
        !in_blind_spot(Mode::Van, Vec2::X, Vec2::new(0.5, -3.0)),
        "not on the left"
    );
    assert!(
        !in_blind_spot(Mode::Van, Vec2::X, Vec2::new(20.0, 3.0)),
        "not far ahead"
    );
    for id in 0..100 {
        // No task without a shoulder check sees it.
        let looker = driver(DriverTask::Cruise, Mode::Van, id);
        for tick in 0..200 {
            assert!(looker.notices(id, cyclist, tick, none).is_none());
        }
        // A shoulder check shows it.
        let check = Looker {
            table: &[(Focus::Shoulder, 1.0)],
            ..driver(DriverTask::Cruise, Mode::Van, id)
        };
        assert!(check.notices(id, cyclist, 5, none).is_some());
        // A right turn has shoulder checks in its table: it sees it in some glances.
        let turn = driver(DriverTask::RightTurn, Mode::Van, id);
        let seen = (0..TICKS)
            .filter(|t| turn.notices(id, cyclist, *t, none).is_some())
            .count();
        assert!(seen > 0, "a right turn includes shoulder checks");
    }
    // A car's pillars: a narrow band only.
    assert!(in_blind_spot(Mode::Car, Vec2::X, Vec2::new(10.0, 8.0)));
    assert!(!in_blind_spot(Mode::Car, Vec2::X, Vec2::new(10.0, 1.0)));
}

#[test]
fn a_surprise_gives_a_longer_reaction_than_an_expected_event() {
    assert!(reaction_s(0.7, true) > reaction_s(0.7, false));
    assert!((reaction_s(0.7, true) - 1.5).abs() < 1e-9);
    assert!((reaction_s(0.7, false) - 0.7).abs() < 1e-9);
}

#[test]
fn a_walker_with_headphones_does_not_hear_a_car_behind_it() {
    let car = sound_level(Mode::Car, 10.0, false);
    let background = perceive::background_noise(6);
    // The direction does not matter for hearing: 8 m behind or ahead.
    assert!(listener_hears(false, car, 8.0, background));
    assert!(!listener_hears(true, car, 8.0, background));
}

#[test]
fn a_silent_vehicle_is_not_heard() {
    let background = perceive::background_noise(10);
    let bicycle = sound_level(Mode::Bicycle, 5.0, false);
    assert!(!listener_hears(false, bicycle, 5.0, background));
    let electric = sound_level(Mode::Car, 15.0 / 3.6, true);
    let diesel = sound_level(Mode::Car, 15.0 / 3.6, false);
    assert!(
        !listener_hears(false, electric, 10.0, background),
        "electric, 15 km/h"
    );
    assert!(
        listener_hears(false, diesel, 10.0, background),
        "the same car with an engine"
    );
    // A van is loud, and a faster electric car is louder.
    assert!(listener_hears(
        false,
        sound_level(Mode::Van, 8.0, false),
        15.0,
        background
    ));
    assert!(sound_level(Mode::Car, 14.0, true) > sound_level(Mode::Car, 4.0, true));
}

#[test]
fn walkers_at_a_kerb_look_both_ways() {
    let table = walker_table(WalkerTask::Kerb);
    let weight = |f: Focus| table.iter().find(|(x, _)| *x == f).map_or(0.0, |(_, w)| *w);
    assert!(weight(Focus::Left) > 0.3 && weight(Focus::Right) > 0.3);
    let total: f64 = table.iter().map(|(_, w)| w).sum();
    assert!((total - 1.0).abs() < 1e-9);
}

#[test]
fn a_narrow_useful_field_loses_the_periphery_first() {
    let full = perceive::look_in(perceive::Zone::Far, 1.0);
    let narrow = perceive::look_in(perceive::Zone::Far, perceive::useful_fov(0.5, 0.5));
    assert!(narrow.0 < full.0 && narrow.1 > full.1);
    assert_eq!(perceive::look_in(perceive::Zone::Centre, 0.5).0, 1.0);
    assert_eq!(perceive::useful_fov(1.0, 0.0), 1.0);
}

/// The right-turn case: a cyclist rides straight on the right of a van that turns right, in
/// the large blind spot of the van. Rate over many drivers and ticks: a van driver who only
/// cruises never notices it (even a fully attentive one, at 3 m); a turning one notices it
/// only in the glances over the shoulder, which is a minority of the time; a car driver in
/// the same place (the pillar band does not cover it) sees it at once.
#[test]
fn a_van_driver_notices_a_cyclist_in_its_blind_spot_only_by_shoulder_checks() {
    let cyclist = Vec2::new(0.5, 3.0);
    let rate = |task: DriverTask, mode: Mode| {
        let (mut seen, mut total) = (0u32, 0u32);
        for id in 0..200u64 {
            let looker = driver(task, mode, id);
            for tick in (0..TICKS).step_by(5) {
                total += 1;
                seen += u32::from(looker.notices(id, cyclist, tick, none).is_some());
            }
        }
        f64::from(seen) / f64::from(total)
    };
    assert_eq!(
        rate(DriverTask::Cruise, Mode::Van),
        0.0,
        "no shoulder check"
    );
    let turning = rate(DriverTask::RightTurn, Mode::Van);
    assert!(
        (0.1..0.7).contains(&turning),
        "a right turn sees it in some glances only: {turning}"
    );
    let car = rate(DriverTask::RightTurn, Mode::Car);
    assert!(
        car > turning,
        "a car has no large blind spot: {car} against {turning}"
    );
}
