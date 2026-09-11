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
/// Join wins even if an in-process runtime is present; every other live
/// launch is Embedded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationTopology {
    /// Singleplayer / listen host with in-process ServerRuntime.
    Embedded,
    /// Join client: projection only.
    JoinClient,
}

impl PresentationTopology {
    pub fn from(role: &MultiplayerRole, has_in_process_runtime: bool) -> Self {
        if role.is_join_client() {
            Self::JoinClient
        } else {
            debug_assert!(
                has_in_process_runtime,
                "non-join presentation requires an in-process runtime"
            );
            Self::Embedded
        }
    }

    pub fn is_embedded(self) -> bool {
        matches!(self, Self::Embedded)
    }

    pub fn is_join_client(self) -> bool {
        matches!(self, Self::JoinClient)
    }

    /// Embedded session inventory may write back into the in-process runtime.
    pub fn should_sync_inventory(self) -> bool {
        matches!(self, Self::Embedded)
    }

    /// Sole chunk-load gate: Join awaits authoritative columns; Embedded may
    /// generate locally. Role helpers must go through this method.
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
                PresentationInventoryAction::SendAuthorityOp
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
            PresentationInventoryTarget::Workstation | PresentationInventoryTarget::Pickup => {
                PresentationInventoryAction::Reject
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

/// What a presentation click / pickup should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationInventoryAction {
    /// Queue a typed authority operation and wait for a projection.
    SendAuthorityOp,
    /// Mutate the presentation copy (the documented embedded session-inventory
    /// writeback exception).
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
    /// Walking over a dropped stack or XP orb. Always Reject; pickup is
    /// authority-only (local collection removed in Plan 03).
    Pickup,
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
            PresentationTopology::from(&MultiplayerRole::Host { port: 25565 }, true),
            PresentationTopology::Embedded
        );
        for target in [
            PresentationInventoryTarget::ContainerSlot,
            PresentationInventoryTarget::Workstation,
            PresentationInventoryTarget::Pickup,
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
        assert_eq!(
            topology.chunk_load_policy(),
            PresentationChunkLoadPolicy::GenerateLocally
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
            topology.chunk_load_policy(),
            PresentationChunkLoadPolicy::AwaitAuthoritativePayload
        );
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

    #[test]
    fn container_click_sends_authority_workstation_rejects_on_both_topologies() {
        for topology in [
            PresentationTopology::Embedded,
            PresentationTopology::JoinClient,
        ] {
            assert_eq!(
                topology.inventory_decision(PresentationInventoryTarget::ContainerSlot),
                PresentationInventoryAction::SendAuthorityOp,
                "{topology:?} container"
            );
            assert_eq!(
                topology.inventory_decision(PresentationInventoryTarget::Workstation),
                PresentationInventoryAction::Reject,
                "{topology:?} workstation"
            );
        }
    }
}
