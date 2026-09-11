//! Embedded runtime bridge extracted from `state.rs`.

use super::*;

/// Opaque bridge between the GPU presentation root and the shared headless
/// runtime.  Local input is always published through `RuntimeInput`; the
/// runtime owns authority state and only exposes results after a fixed tick.
/// Keeping this adapter in the presentation module prevents the renderer from
/// retaining a second `AuthorityCore` or observing mutable core internals.
pub(super) struct EmbeddedRuntimeBridge {
    pub(super) runtime: crate::server_runtime::ServerRuntime,
    pub(super) input: crate::server_runtime::RuntimeInput,
    pub(super) session_id: crate::network::protocol::PlayerId,
    pub(super) next_request_id: u128,
    pub(super) next_client_sequence: u64,
    pub(super) next_pose_sender_time_millis: u64,
    pub(super) revisions: std::collections::HashMap<crate::dimension::Dimension, u64>,
    pub(super) pending_request_dimensions:
        std::collections::HashMap<u128, crate::dimension::Dimension>,
}

impl EmbeddedRuntimeBridge {
    pub(super) fn new(
        role: &MultiplayerRole,
        world_dir: std::path::PathBuf,
        seed: u32,
        difficulty: Difficulty,
        render_distance: u32,
        pvp: bool,
    ) -> Result<Self, crate::server_runtime::ServerConfigError> {
        let session_id = u64::MAX;
        let options = match role {
            MultiplayerRole::Singleplayer => {
                crate::server_runtime::EmbeddedRuntimeOptions::singleplayer(
                    crate::server_runtime::LocalSessionProfile::new(session_id, "local"),
                )
            }
            MultiplayerRole::Host { .. } => crate::server_runtime::EmbeddedRuntimeOptions::listen(
                crate::server_runtime::LocalSessionProfile::new(session_id, "host"),
            ),
            MultiplayerRole::Client { .. } => {
                return Err(crate::server_runtime::ServerConfigError::Invalid {
                    key: "embedded-runtime".into(),
                    value: "client".into(),
                    reason: "client presentations use NetworkClient transport".into(),
                })
            }
        };
        let mut properties = crate::server_runtime::ServerProperties::default();
        properties.world_dir = world_dir;
        properties.seed = u64::from(seed);
        properties.pvp = pvp;
        properties.view_distance = render_distance.clamp(2, 32) as u8;
        properties.simulation_distance = render_distance.clamp(2, 32) as u8;
        properties.difficulty = match difficulty {
            Difficulty::Peaceful => "peaceful",
            Difficulty::Easy => "easy",
            Difficulty::Normal => "normal",
            Difficulty::Hard => "hard",
        }
        .to_string();
        if let MultiplayerRole::Host { port } = role {
            properties.port = *port;
        }
        let (runtime, input) =
            crate::server_runtime::ServerRuntime::new_embedded(properties, options)?;
        Ok(Self {
            runtime,
            input,
            session_id,
            next_request_id: 1,
            next_client_sequence: 1,
            next_pose_sender_time_millis: 1,
            revisions: std::collections::HashMap::new(),
            pending_request_dimensions: std::collections::HashMap::new(),
        })
    }

    pub(super) fn session_id(&self) -> crate::network::protocol::PlayerId {
        self.session_id
    }

    pub(super) fn session_game_mode(&self) -> Option<crate::inventory::GameMode> {
        self.runtime
            .authority
            .session(self.session_id)
            .map(|session| session.game_mode)
    }

    pub(super) fn revision_for_dimension(&self, dimension: crate::dimension::Dimension) -> u64 {
        self.revisions.get(&dimension).copied().unwrap_or_default()
    }

    pub(super) fn queue_request(
        &mut self,
        mut request: crate::network::protocol::GameplayRequest,
    ) -> Result<(), crate::server_runtime::RuntimeInputError> {
        request.request_id = self.next_request_id;
        request.client_sequence = self.next_client_sequence;
        request.session_id = self.session_id;
        request.client_revision = self.revision_for_dimension(
            crate::dimension::Dimension::from_wire(request.dimension).unwrap_or_default(),
        );
        let request_id = request.request_id;
        let dimension =
            crate::dimension::Dimension::from_wire(request.dimension).unwrap_or_default();
        self.input.submit_request(self.session_id, request)?;
        self.pending_request_dimensions
            .insert(request_id, dimension);
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.next_client_sequence = self.next_client_sequence.saturating_add(1);
        Ok(())
    }

    pub(super) fn queue_position(
        &mut self,
        sequence: u32,
        position: glam::Vec3,
        yaw: f32,
        pitch: f32,
    ) -> Result<(), crate::server_runtime::RuntimeInputError> {
        let sender_time_millis = self.next_pose_sender_time_millis;
        self.next_pose_sender_time_millis = self
            .next_pose_sender_time_millis
            .saturating_add(50)
            .max(sender_time_millis.saturating_add(1));
        self.input
            .try_send(crate::network::server::ServerToHost::ClientPosition {
                id: self.session_id,
                sequence,
                sender_time_millis,
                x: position.x,
                y: position.y,
                z: position.z,
                yaw,
                pitch,
            })
    }

    pub(super) fn tick(&mut self) -> std::io::Result<crate::server_runtime::RuntimeTickOutput> {
        let output = self.runtime.tick_with_output()?;
        for mutation in &output.snapshot.mutations {
            if let Some(dimension) = crate::dimension::Dimension::from_wire(mutation.dimension) {
                self.revisions
                    .entry(dimension)
                    .and_modify(|revision| *revision = (*revision).max(mutation.revision))
                    .or_insert(mutation.revision);
            }
        }
        for event in &output.presentation_events {
            let crate::server_runtime::RuntimePresentationEvent::GameplayResponse {
                response, ..
            } = event
            else {
                continue;
            };
            if let crate::network::protocol::GameplayOutcome::Accepted { revision } =
                &response.outcome
            {
                if let Some(dimension) =
                    self.pending_request_dimensions.remove(&response.request_id)
                {
                    self.revisions
                        .entry(dimension)
                        .and_modify(|current| *current = (*current).max(*revision))
                        .or_insert(*revision);
                }
            } else {
                self.pending_request_dimensions.remove(&response.request_id);
            }
        }
        Ok(output)
    }

    pub(super) fn save_all(&mut self) -> std::io::Result<()> {
        self.runtime.save_all()
    }

    pub(super) fn shutdown(&mut self) -> std::io::Result<()> {
        self.runtime.shutdown()
    }

    /// Copy only presentation session inventory + selected hotbar into the
    /// embedded authority. Health, hunger, XP, mining, and mounts stay
    /// server-owned.
    pub(super) fn sync_local_inventory(
        &mut self,
        inventory: [Option<crate::authority::contract::SessionInventorySlot>;
            crate::authority::contract::SESSION_INVENTORY_SLOTS],
        cursor: Option<crate::authority::contract::SessionInventorySlot>,
        selected_hotbar_slot: u8,
    ) -> bool {
        let Some(mut authoritative) = self
            .runtime
            .authority
            .session(self.session_id)
            .map(|session| session.gameplay)
        else {
            return false;
        };
        authoritative.inventory = inventory;
        authoritative.cursor = cursor;
        authoritative.selected_hotbar_slot = selected_hotbar_slot.min(8);
        self.runtime
            .authority
            .set_session_gameplay(self.session_id, authoritative)
    }
}
