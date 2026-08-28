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

/// Presentation-root runtime topology. Derived from role + in-process runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationTopology {
    /// Singleplayer / listen host with in-process ServerRuntime.
    Embedded,
    /// Join client: projection only.
    JoinClient,
    /// No runtime and not Join. Leftover world owner; current menu launch should not reach this.
    LegacyOwner,
}

impl PresentationTopology {
    pub fn from(role: &MultiplayerRole, has_in_process_runtime: bool) -> Self {
        if role.is_join_client() {
            Self::JoinClient
        } else if has_in_process_runtime {
            Self::Embedded
        } else {
            Self::LegacyOwner
        }
    }

    /// Test-compat mapping of the old `(has_runtime, is_client)` pair.
    pub fn from_bools(has_in_process_runtime: bool, is_client: bool) -> Self {
        if is_client {
            Self::JoinClient
        } else if has_in_process_runtime {
            Self::Embedded
        } else {
            Self::LegacyOwner
        }
    }

    pub fn is_embedded(self) -> bool {
        matches!(self, Self::Embedded)
    }

    pub fn is_join_client(self) -> bool {
        matches!(self, Self::JoinClient)
    }

    pub fn is_legacy_owner(self) -> bool {
        matches!(self, Self::LegacyOwner)
    }

    /// Leftover world owner may still mutate the presentation copy.
    pub fn should_mutate_world(self) -> bool {
        matches!(self, Self::LegacyOwner)
    }

    /// Embedded session inventory may write back into the in-process runtime.
    pub fn should_sync_inventory(self) -> bool {
        matches!(self, Self::Embedded)
    }

    pub fn chunk_load_policy(self) -> PresentationChunkLoadPolicy {
        if self.is_join_client() {
            PresentationChunkLoadPolicy::AwaitAuthoritativePayload
        } else {
            PresentationChunkLoadPolicy::GenerateLocally
        }
    }

    pub fn inventory_decision(
        self,
        target: PresentationInventoryTarget,
    ) -> PresentationInventoryAction {
        match target {
            PresentationInventoryTarget::ContainerSlot => {
                if self.should_mutate_world() {
                    PresentationInventoryAction::LocalMutate
                } else {
                    PresentationInventoryAction::SendAuthorityOp
                }
            }
            PresentationInventoryTarget::PlayerInventory => {
                if self.is_join_client() {
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
                if self.should_mutate_world() {
                    PresentationInventoryAction::LocalMutate
                } else {
                    PresentationInventoryAction::Reject
                }
            }
        }
    }

    pub fn should_writeback_after_inventory_click(
        self,
        target: Option<PresentationInventoryTarget>,
    ) -> bool {
        self.should_sync_inventory()
            && matches!(target, Some(PresentationInventoryTarget::PlayerInventory))
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
///
/// Deprecated/test-compat wrapper. Production code should use
/// [`PresentationTopology::should_mutate_world`].
pub fn should_mutate_presentation_world(has_in_process_runtime: bool, is_client: bool) -> bool {
    PresentationTopology::from_bools(has_in_process_runtime, is_client).should_mutate_world()
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
///
/// Deprecated/test-compat wrapper. Production code should use
/// [`PresentationTopology::inventory_decision`].
pub fn presentation_inventory_decision(
    has_in_process_runtime: bool,
    is_client: bool,
    target: PresentationInventoryTarget,
) -> PresentationInventoryAction {
    PresentationTopology::from_bools(has_in_process_runtime, is_client).inventory_decision(target)
}

/// Join clients never write presentation inventory into authority.
/// Without an in-process runtime the writeback is a no-op and must not be called.
///
/// Deprecated/test-compat wrapper. Production code should use
/// [`PresentationTopology::should_sync_inventory`].
pub fn should_sync_authority_inventory_from_local(
    has_in_process_runtime: bool,
    is_client: bool,
) -> bool {
    PresentationTopology::from_bools(has_in_process_runtime, is_client).should_sync_inventory()
}

/// After an inventory click, write back session inventory only when the click
/// locally mutated player inventory on an embedded presentation.
///
/// Deprecated/test-compat wrapper. Production code should use
/// [`PresentationTopology::should_writeback_after_inventory_click`].
pub fn should_writeback_after_inventory_click(
    has_in_process_runtime: bool,
    is_client: bool,
    target: Option<PresentationInventoryTarget>,
) -> bool {
    PresentationTopology::from_bools(has_in_process_runtime, is_client)
        .should_writeback_after_inventory_click(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_does_not_mutate_world_or_spend_levels() {
        let topology = PresentationTopology::Embedded;
        assert_eq!(
            PresentationTopology::from(&MultiplayerRole::Singleplayer, true),
            PresentationTopology::Embedded
        );
        assert_eq!(
            PresentationTopology::from_bools(true, false),
            PresentationTopology::Embedded
        );
        assert!(!topology.should_mutate_world());
        for target in [
            PresentationInventoryTarget::ContainerSlot,
            PresentationInventoryTarget::Workstation,
            PresentationInventoryTarget::Pickup,
            PresentationInventoryTarget::FarmlandTrample,
            PresentationInventoryTarget::UnsupportedBreak,
        ] {
            let action = topology.inventory_decision(target);
            assert_ne!(
                action,
                PresentationInventoryAction::LocalMutate,
                "{target:?} must not locally mutate on embedded"
            );
        }
        assert_eq!(
            topology.inventory_decision(PresentationInventoryTarget::ContainerSlot),
            PresentationInventoryAction::SendAuthorityOp
        );
        assert_eq!(
            topology.inventory_decision(PresentationInventoryTarget::Workstation),
            PresentationInventoryAction::Reject
        );
        assert_eq!(
            topology.inventory_decision(PresentationInventoryTarget::Pickup),
            PresentationInventoryAction::Reject
        );
    }

    #[test]
    fn join_client_never_writeback_or_local_world_mutate() {
        let topology = PresentationTopology::JoinClient;
        assert_eq!(
            PresentationTopology::from(
                &MultiplayerRole::Client {
                    server_addr: "127.0.0.1".into(),
                    port: 25565,
                    username: "JOINER".into(),
                },
                false,
            ),
            PresentationTopology::JoinClient
        );
        assert_eq!(
            PresentationTopology::from(
                &MultiplayerRole::Client {
                    server_addr: "127.0.0.1".into(),
                    port: 25565,
                    username: "JOINER".into(),
                },
                true,
            ),
            PresentationTopology::JoinClient
        );
        assert_eq!(
            PresentationTopology::from_bools(false, true),
            PresentationTopology::JoinClient
        );
        assert!(!topology.should_mutate_world());
        assert!(!topology.should_sync_inventory());
        assert!(!topology.should_writeback_after_inventory_click(Some(
            PresentationInventoryTarget::PlayerInventory
        )));
        assert_eq!(
            topology.inventory_decision(PresentationInventoryTarget::ContainerSlot),
            PresentationInventoryAction::SendAuthorityOp
        );
        assert_eq!(
            topology.inventory_decision(PresentationInventoryTarget::PlayerInventory),
            PresentationInventoryAction::Reject
        );
        assert_eq!(
            topology.inventory_decision(PresentationInventoryTarget::Pickup),
            PresentationInventoryAction::Reject
        );
        assert_eq!(
            topology.inventory_decision(PresentationInventoryTarget::FarmlandTrample),
            PresentationInventoryAction::Reject
        );
        assert_eq!(
            topology.inventory_decision(PresentationInventoryTarget::UnsupportedBreak),
            PresentationInventoryAction::Reject
        );
    }

    #[test]
    fn no_runtime_non_client_keeps_legacy_local_mutate() {
        let topology = PresentationTopology::LegacyOwner;
        assert_eq!(
            PresentationTopology::from(&MultiplayerRole::Singleplayer, false),
            PresentationTopology::LegacyOwner
        );
        assert_eq!(
            PresentationTopology::from_bools(false, false),
            PresentationTopology::LegacyOwner
        );
        assert!(topology.should_mutate_world());
        assert!(!topology.should_sync_inventory());
        assert_eq!(
            topology.inventory_decision(PresentationInventoryTarget::ContainerSlot),
            PresentationInventoryAction::LocalMutate
        );
        assert_eq!(
            topology.inventory_decision(PresentationInventoryTarget::Pickup),
            PresentationInventoryAction::LocalMutate
        );
        assert!(!topology.should_writeback_after_inventory_click(Some(
            PresentationInventoryTarget::PlayerInventory
        )));
    }

    #[test]
    fn embedded_player_inventory_is_the_writeback_exception() {
        let topology = PresentationTopology::Embedded;
        assert_eq!(
            topology.inventory_decision(PresentationInventoryTarget::PlayerInventory),
            PresentationInventoryAction::LocalMutate
        );
        assert!(topology.should_sync_inventory());
        assert!(topology.should_writeback_after_inventory_click(Some(
            PresentationInventoryTarget::PlayerInventory
        )));
        assert!(!topology.should_writeback_after_inventory_click(Some(
            PresentationInventoryTarget::ContainerSlot
        )));
        assert!(!topology.should_writeback_after_inventory_click(None));
    }
}
