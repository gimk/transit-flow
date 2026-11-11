//! Port of `tests/unit/test_sim_clock.gd`.

use tf_sim::clock::{Period, SimClock};
use tf_sim::events::{Outbox, SimEvent};

fn advance_game_minutes(clock: &mut SimClock, minutes: f64, out: &mut Outbox) {
    clock.advance(minutes / SimClock::GAME_MINUTES_PER_SIM_SECOND, out);
}

fn hours_changed(out: &Outbox) -> Vec<i32> {
    out.iter()
        .filter_map(|event| match event {
            SimEvent::HourChanged(hour) => Some(*hour),
            _ => None,
        })
        .collect()
}

fn days_started(out: &Outbox) -> Vec<i32> {
    out.iter()
        .filter_map(|event| match event {
            SimEvent::DayStarted(day) => Some(*day),
            _ => None,
        })
        .collect()
}

#[test]
fn starts_on_day_one_at_six() {
    let clock = SimClock::new();
    assert_eq!(clock.day(), 1);
    assert_eq!(clock.format_time(), "06:00");
    assert_eq!(clock.period(), Period::MorningRush);
}

#[test]
fn advance_converts_sim_seconds_to_game_minutes() {
    let mut clock = SimClock::new();
    clock.advance(3.0, &mut Outbox::new());
    assert_eq!(clock.format_time(), "06:15");
}

#[test]
fn hour_changed_emitted_once_per_hour_crossed() {
    let mut clock = SimClock::new();
    let mut out = Outbox::new();
    advance_game_minutes(&mut clock, 150.0, &mut out); // 06:00 -> 08:30
    assert_eq!(hours_changed(&out).len(), 2);
    assert!(hours_changed(&out).contains(&8));
}

#[test]
fn large_step_does_not_skip_hours() {
    let mut clock = SimClock::new();
    let mut out = Outbox::new();
    advance_game_minutes(&mut clock, 5.0 * 60.0, &mut out);
    assert_eq!(hours_changed(&out).len(), 5);
}

#[test]
fn day_rolls_over_at_midnight() {
    let mut clock = SimClock::new();
    let mut out = Outbox::new();
    advance_game_minutes(&mut clock, 18.0 * 60.0, &mut out); // 06:00 -> 00:00 next day
    assert_eq!(clock.day(), 2);
    assert_eq!(clock.format_time(), "00:00");
    assert_eq!(days_started(&out), vec![2]);
}

#[test]
fn period_boundaries() {
    assert_eq!(Period::for_hour(5), Period::Night);
    assert_eq!(Period::for_hour(6), Period::MorningRush);
    assert_eq!(Period::for_hour(10), Period::MiddayLogistics);
    assert_eq!(Period::for_hour(16), Period::EveningRush);
    assert_eq!(Period::for_hour(20), Period::Night);
    assert_eq!(Period::for_hour(23), Period::Night);
}
