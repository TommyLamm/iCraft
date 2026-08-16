//! Presentation-root gates for inventory, pickup, and world mutation.
//!
//! Embedded Singleplayer / Listen Host and join clients must not locally
//! mutate authority-owned world, containers, XP, or entities. Session
//! inventory writeback is a narrow exception for embedded hotbar/inventory
//! UI and is never used on a join client.

/// Desktop / embedded session topology. Kept out of the wgpu menu so the
/// dedicated server and join-projection tests can use it without compiling
/// the presentation UI.
#[derive(Debug, Clone)]
pub enum MultiplayerRole {
    Singleplayer,
    Host {
        port: u16,
    },
    Client {
        server_addr: String,
        port: u16,
        username: String,
    },
}

impl MultiplayerRole {
    pub fn is_join_client(&self) -> bool {
        matches!(self, MultiplayerRole::Client { .. })
    }
}

/// How the presentation root may populate a chunk column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationChunkLoadPolicy {
    /// Singleplayer / host: generate locally (save overlay is separate).
    GenerateLocally,
    /// Join client: never generate. Wait for revision-gated `ChunkData`.
    AwaitAuthoritativePayload,
}

pub fn presentation_chunk_load_policy(role: &MultiplayerRole) -> PresentationChunkLoadPolicy {
    if role.is_join_client() {
        PresentationChunkLoadPolicy::AwaitAuthoritativePayload
    } else {
        PresentationChunkLoadPolicy::GenerateLocally
    }
}

pub fn presentation_may_generate_chunks(role: &MultiplayerRole) -> bool {
    !role.is_join_client()
}

/// Join clients must not write presentation chunks (farmland, unsupported-break,
/// or any other local `set_block`). Plan 06 can AND this with "no embedded".
pub fn presentation_may_mutate_chunks(role: &MultiplayerRole) -> bool {
    !role.is_join_client()
}

/// Testable load-schedule gate. `generate` is invoked only when the role is
/// allowed to materialize a local column.
pub fn schedule_presentation_chunk_load<T>(
    policy: PresentationChunkLoadPolicy,
    generate: impl FnOnce() -> T,
) -> Option<T> {
    match policy {
        PresentationChunkLoadPolicy::GenerateLocally => Some(generate()),
        PresentationChunkLoadPolicy::AwaitAuthoritativePayload => None,
    }
}

/// True when the presentation `ChunkManager` / local player may still own
/// world, container, entity, and XP mutations (legacy path without a runtime).
pub fn should_mutate_presentation_world(has_in_process_runtime: bool, is_client: bool) -> bool {
    !has_in_process_runtime && !is_client
}

/// What a presentation click / pickup / trampling / unsupported-break should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationInventoryAction {
    /// Queue a typed authority operation and wait for a projection.
    SendAuthorityOp,
    /// Mutate the presentation copy (legacy world owner, or the documented
    /// embedded session-inventory writeback exception).
    LocalMutate,
    /// Do nothing. Rejection must not consume items or spawn drops.
    Reject,
}

/// Click / interaction class used by the presentation gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationInventoryTarget {
    /// Chest / furnace / hopper / other world container slot.
    ContainerSlot,
    /// Player hotbar, backpack, armor, offhand, or crafting grid.
    PlayerInventory,
    /// Enchanting / anvil / recipe-book furnace fills that spend levels or
    /// write workstation block entities.
    Workstation,
    /// Walking over a dropped stack or XP orb.
    Pickup,
    /// Sprint / fall trampling of farmland.
    FarmlandTrample,
    /// Gravity-unsupported block break while integrating a loaded chunk.
    UnsupportedBreak,
}

/// Decide how the presentation root should handle one interaction.
pub fn presentation_inventory_decision(
    has_in_process_runtime: bool,
    is_client: bool,
    target: PresentationInventoryTarget,
) -> PresentationInventoryAction {
    match target {
        PresentationInventoryTarget::ContainerSlot => {
            if has_in_process_runtime || is_client {
                PresentationInventoryAction::SendAuthorityOp
            } else {
                PresentationInventoryAction::LocalMutate
            }
        }
        PresentationInventoryTarget::PlayerInventory => {
            if is_client {
                PresentationInventoryAction::Reject
            } else {
                // Embedded inventory UI may still mutate the session inventory
                // and write back only inventory + selected hotbar.
                PresentationInventoryAction::LocalMutate
            }
        }
        PresentationInventoryTarget::Workstation
        | PresentationInventoryTarget::Pickup
        | PresentationInventoryTarget::FarmlandTrample
        | PresentationInventoryTarget::UnsupportedBreak => {
            if has_in_process_runtime || is_client {
                PresentationInventoryAction::Reject
            } else {
                PresentationInventoryAction::LocalMutate
            }
        }
    }
}

/// Join clients never write presentation inventory into authority.
/// Without an in-process runtime the writeback is a no-op and must not be called.
pub fn should_sync_authority_inventory_from_local(
    has_in_process_runtime: bool,
    is_client: bool,
) -> bool {
    has_in_process_runtime && !is_client
}

/// After an inventory click, write back session inventory only when the click
/// locally mutated player inventory on an embedded presentation.
pub fn should_writeback_after_inventory_click(
    has_in_process_runtime: bool,
    is_client: bool,
    target: Option<PresentationInventoryTarget>,
) -> bool {
    if !should_sync_authority_inventory_from_local(has_in_process_runtime, is_client) {
        return false;
    }
    matches!(target, Some(PresentationInventoryTarget::PlayerInventory))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_does_not_mutate_world_or_spend_levels() {
        assert!(!should_mutate_presentation_world(true, false));
        for target in [
            PresentationInventoryTarget::ContainerSlot,
            PresentationInventoryTarget::Workstation,
            PresentationInventoryTarget::Pickup,
            PresentationInventoryTarget::FarmlandTrample,
            PresentationInventoryTarget::UnsupportedBreak,
        ] {
            let action = presentation_inventory_decision(true, false, target);
            assert_ne!(
                action,
                PresentationInventoryAction::LocalMutate,
                "{target:?} must not locally mutate on embedded"
            );
        }
        assert_eq!(
            presentation_inventory_decision(
                true,
                false,
                PresentationInventoryTarget::ContainerSlot
            ),
            PresentationInventoryAction::SendAuthorityOp
        );
        assert_eq!(
            presentation_inventory_decision(true, false, PresentationInventoryTarget::Workstation),
            PresentationInventoryAction::Reject
        );
        assert_eq!(
            presentation_inventory_decision(true, false, PresentationInventoryTarget::Pickup),
            PresentationInventoryAction::Reject
        );
    }

    #[test]
    fn join_client_never_writeback_or_local_world_mutate() {
        assert!(!should_mutate_presentation_world(false, true));
        assert!(!should_sync_authority_inventory_from_local(false, true));
        assert!(!should_writeback_after_inventory_click(
            false,
            true,
            Some(PresentationInventoryTarget::PlayerInventory)
        ));
        assert_eq!(
            presentation_inventory_decision(
                false,
                true,
                PresentationInventoryTarget::ContainerSlot
            ),
            PresentationInventoryAction::SendAuthorityOp
        );
        assert_eq!(
            presentation_inventory_decision(
                false,
                true,
                PresentationInventoryTarget::PlayerInventory
            ),
            PresentationInventoryAction::Reject
        );
        assert_eq!(
            presentation_inventory_decision(false, true, PresentationInventoryTarget::Pickup),
            PresentationInventoryAction::Reject
        );
        assert_eq!(
            presentation_inventory_decision(
                false,
                true,
                PresentationInventoryTarget::FarmlandTrample
            ),
            PresentationInventoryAction::Reject
        );
        assert_eq!(
            presentation_inventory_decision(
                false,
                true,
                PresentationInventoryTarget::UnsupportedBreak
            ),
            PresentationInventoryAction::Reject
        );
    }

    #[test]
    fn no_runtime_non_client_keeps_legacy_local_mutate() {
        assert!(should_mutate_presentation_world(false, false));
        assert!(!should_sync_authority_inventory_from_local(false, false));
        assert_eq!(
            presentation_inventory_decision(
                false,
                false,
                PresentationInventoryTarget::ContainerSlot
            ),
            PresentationInventoryAction::LocalMutate
        );
        assert_eq!(
            presentation_inventory_decision(false, false, PresentationInventoryTarget::Pickup),
            PresentationInventoryAction::LocalMutate
        );
        assert!(!should_writeback_after_inventory_click(
            false,
            false,
            Some(PresentationInventoryTarget::PlayerInventory)
        ));
    }

    #[test]
    fn embedded_player_inventory_is_the_writeback_exception() {
        assert_eq!(
            presentation_inventory_decision(
                true,
                false,
                PresentationInventoryTarget::PlayerInventory
            ),
            PresentationInventoryAction::LocalMutate
        );
        assert!(should_sync_authority_inventory_from_local(true, false));
        assert!(should_writeback_after_inventory_click(
            true,
            false,
            Some(PresentationInventoryTarget::PlayerInventory)
        ));
        assert!(!should_writeback_after_inventory_click(
            true,
            false,
            Some(PresentationInventoryTarget::ContainerSlot)
        ));
        assert!(!should_writeback_after_inventory_click(true, false, None));
    }
}
