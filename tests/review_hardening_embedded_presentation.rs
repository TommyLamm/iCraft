//! Plan06: embedded presentation must not mutate the world.
//!
//! Full `State` construction needs wgpu, so these tests lock the extracted
//! presentation gate. A live join / embedded `State` seam is left to plan 15.
//! ContainerClick conservation itself is covered by plan 02.

use icraft::presentation_inventory_policy::{
    PresentationInventoryAction, PresentationInventoryTarget, PresentationTopology,
};

#[test]
fn embedded_gate_sends_container_op_and_rejects_world_mutations() {
    let topology = PresentationTopology::Embedded;
    assert_eq!(
        topology.inventory_decision(PresentationInventoryTarget::ContainerSlot),
        PresentationInventoryAction::SendAuthorityOp
    );
    for target in [
        PresentationInventoryTarget::Workstation,
        PresentationInventoryTarget::Pickup,
    ] {
        assert_eq!(
            topology.inventory_decision(target),
            PresentationInventoryAction::Reject,
            "{target:?}"
        );
    }
    assert!(!topology.should_writeback_after_inventory_click(Some(
        PresentationInventoryTarget::ContainerSlot
    )));
}

#[test]
fn join_client_never_calls_inventory_writeback() {
    let topology = PresentationTopology::JoinClient;
    assert!(!topology.should_sync_inventory());
    assert!(!topology.should_writeback_after_inventory_click(Some(
        PresentationInventoryTarget::PlayerInventory
    )));
    assert_eq!(
        topology.inventory_decision(PresentationInventoryTarget::Pickup),
        PresentationInventoryAction::Reject
    );
}

#[test]
fn embedded_player_inventory_writeback_is_the_only_exception() {
    let topology = PresentationTopology::Embedded;
    assert_eq!(
        topology.inventory_decision(PresentationInventoryTarget::PlayerInventory),
        PresentationInventoryAction::LocalMutate
    );
    assert!(topology.should_sync_inventory());
    assert!(topology.should_writeback_after_inventory_click(Some(
        PresentationInventoryTarget::PlayerInventory
    )));
}
