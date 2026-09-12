// Tests extracted from state.rs::sprint_policy_tests (Plan 27).

use super::*;

use super::*;

#[test]
fn creative_sprint_ignores_hunger_and_exhaustion() {
    assert!(sprint_allowed(GameMode::Creative, 0.0));
    assert_eq!(
        sprint_exhaustion_amount(GameMode::Creative, true, true, 10.0),
        0.0
    );
}

#[test]
fn survival_sprint_keeps_hunger_and_exhaustion_rules() {
    assert!(!sprint_allowed(GameMode::Survival, 6.0));
    assert!(sprint_allowed(GameMode::Survival, 6.01));
    assert!(
        (sprint_exhaustion_amount(GameMode::Survival, true, true, 10.0) - 1.5).abs()
            < f32::EPSILON
    );
    assert_eq!(
        sprint_exhaustion_amount(GameMode::Survival, false, true, 10.0),
        0.0
    );
    assert_eq!(
        sprint_exhaustion_amount(GameMode::Survival, true, false, 10.0),
        0.0
    );
}
