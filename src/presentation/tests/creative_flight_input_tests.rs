// Tests extracted from state.rs::creative_flight_input_tests (Plan 27).

use super::*;

use super::*;

#[test]
fn double_tap_toggles_only_inside_the_window() {
    let start = Instant::now();
    let mut tracker = DoubleTapTracker::default();

    assert!(!tracker.register(start, true, false));
    assert!(tracker.register(start + Duration::from_millis(300), true, false));

    assert!(!tracker.register(start + Duration::from_secs(1), true, false));
    assert!(!tracker.register(start + Duration::from_millis(1301), true, false));
}

#[test]
fn repeat_does_not_count_as_a_second_tap() {
    let start = Instant::now();
    let mut tracker = DoubleTapTracker::default();

    assert!(!tracker.register(start, true, false));
    assert!(!tracker.register(start + Duration::from_millis(50), true, true));
    assert!(tracker.register(start + Duration::from_millis(100), true, false));
}

#[test]
fn disabled_or_reset_tracker_cannot_prearm_creative_flight() {
    let start = Instant::now();
    let mut tracker = DoubleTapTracker::default();

    assert!(!tracker.register(start, false, false));
    assert!(!tracker.register(start + Duration::from_millis(100), true, false));
    tracker.reset();
    assert!(!tracker.register(start + Duration::from_millis(200), true, false));
    assert!(tracker.register(start + Duration::from_millis(250), true, false));
}

#[test]
fn successful_double_tap_starts_a_fresh_pair() {
    let start = Instant::now();
    let mut tracker = DoubleTapTracker::default();

    assert!(!tracker.register(start, true, false));
    assert!(tracker.register(start + Duration::from_millis(50), true, false));
    assert!(!tracker.register(start + Duration::from_millis(100), true, false));
    assert!(tracker.register(start + Duration::from_millis(150), true, false));
}

#[test]
fn only_descending_onto_the_ground_exits_flight() {
    assert!(should_exit_creative_flight(true, -1.0, true));
    assert!(!should_exit_creative_flight(true, 0.0, true));
    assert!(!should_exit_creative_flight(true, 1.0, true));
    assert!(!should_exit_creative_flight(true, -1.0, false));
    assert!(!should_exit_creative_flight(false, -1.0, true));
}
