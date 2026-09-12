// Tests extracted from state.rs::camera_input_tests (Plan 27).

use super::*;

use super::{allows_camera_look, allows_continuous_mining, cursor_position_to_ndc, GameMode};

#[test]
fn every_gameplay_blocker_disables_camera_look() {
    assert!(allows_camera_look(
        false, false, false, false, false, false, true
    ));

    assert!(!allows_camera_look(
        true, false, false, false, false, false, true
    ));
    assert!(!allows_camera_look(
        false, true, false, false, false, false, true
    ));
    assert!(!allows_camera_look(
        false, false, true, false, false, false, true
    ));
    assert!(!allows_camera_look(
        false, false, false, true, false, false, true
    ));
    assert!(!allows_camera_look(
        false, false, false, false, true, false, true
    ));
    assert!(!allows_camera_look(
        false, false, false, false, false, true, true
    ));
    assert!(!allows_camera_look(
        false, false, false, false, false, false, false
    ));
}

#[test]
fn cursor_position_still_maps_to_ui_coordinates() {
    assert_eq!(cursor_position_to_ndc(0.0, 0.0, 1280, 720), [-1.0, 1.0]);
    assert_eq!(cursor_position_to_ndc(640.0, 360.0, 1280, 720), [0.0, 0.0]);
    assert_eq!(
        cursor_position_to_ndc(1280.0, 720.0, 1280, 720),
        [1.0, -1.0]
    );
}

#[test]
fn continuous_mining_requires_unblocked_survival_input() {
    assert!(allows_continuous_mining(true, GameMode::Survival, true));
    assert!(!allows_continuous_mining(false, GameMode::Survival, true));
    assert!(!allows_continuous_mining(true, GameMode::Creative, true));
    assert!(!allows_continuous_mining(true, GameMode::Survival, false));
}
