//! Plan06: embedded presentation must not mutate the world.
//!
//! Full `State` construction needs wgpu, so these tests lock the extracted
//! presentation gate. A live join / embedded `State` seam is left to plan 15.
//! ContainerClick conservation itself is covered by plan 02.

use icraft::presentation_inventory_policy::{
    presentation_inventory_decision, should_mutate_presentation_world,
    should_sync_authority_inventory_from_local, should_writeback_after_inventory_click,
    PresentationInventoryAction, PresentationInventoryTarget,
};

#[test]
fn embedded_gate_sends_container_op_and_rejects_world_mutations() {
    assert!(!should_mutate_presentation_world(true, false));
    assert_eq!(
        presentation_inventory_decision(true, false, PresentationInventoryTarget::ContainerSlot),
        PresentationInventoryAction::SendAuthorityOp
    );
    for target in [
        PresentationInventoryTarget::Workstation,
        PresentationInventoryTarget::Pickup,
        PresentationInventoryTarget::FarmlandTrample,
        PresentationInventoryTarget::UnsupportedBreak,
    ] {
        assert_eq!(
            presentation_inventory_decision(true, false, target),
            PresentationInventoryAction::Reject,
            "{target:?}"
        );
    }
    assert!(!should_writeback_after_inventory_click(
        true,
        false,
        Some(PresentationInventoryTarget::ContainerSlot)
    ));
}

#[test]
fn join_client_never_calls_inventory_writeback() {
    assert!(!should_sync_authority_inventory_from_local(false, true));
    assert!(!should_writeback_after_inventory_click(
        false,
        true,
        Some(PresentationInventoryTarget::PlayerInventory)
    ));
    assert!(!should_mutate_presentation_world(false, true));
    assert_eq!(
        presentation_inventory_decision(false, true, PresentationInventoryTarget::Pickup),
        PresentationInventoryAction::Reject
    );
}

#[test]
fn no_runtime_must_not_invoke_writeback() {
    assert!(!should_sync_authority_inventory_from_local(false, false));
    assert!(!should_writeback_after_inventory_click(
        false,
        false,
        Some(PresentationInventoryTarget::PlayerInventory)
    ));
    assert!(should_mutate_presentation_world(false, false));
}

#[test]
fn embedded_player_inventory_writeback_is_the_only_exception() {
    assert_eq!(
        presentation_inventory_decision(true, false, PresentationInventoryTarget::PlayerInventory),
        PresentationInventoryAction::LocalMutate
    );
    assert!(should_sync_authority_inventory_from_local(true, false));
    assert!(should_writeback_after_inventory_click(
        true,
        false,
        Some(PresentationInventoryTarget::PlayerInventory)
    ));
}
