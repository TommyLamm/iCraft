//! Authority projection delivery for embedded and join clients (Plan 27).
//! Child of `state` via `#[path]`.

use super::*;

impl State {

    /// Advance the in-process authority by one fixed 20 Hz tick.  Dedicated
    /// mode has no `State`, while a network client correctly returns `None`.
    pub fn tick_authority_boundary(
        &mut self,
    ) -> Option<crate::authority::contract::AuthoritySnapshot> {
        let sequence = self.network_pose_sequence;
        let position = self.player_physics.position;
        let yaw = self.camera.yaw;
        let pitch = self.camera.pitch;
        let runtime = self.embedded_runtime.as_mut()?;
        let session_id = runtime.session_id();
        let _ = runtime.queue_position(sequence, position, yaw, pitch);
        let output = match runtime.tick() {
            Ok(output) => output,
            Err(error) => {
                self.save_error
                    .get_or_insert_with(|| format!("embedded runtime tick failed: {error}"));
                return None;
            }
        };
        for event in output.presentation_events {
            self.deliver_projection_event(event, session_id);
        }
        self.project_authority_mutations(&output.snapshot.mutations);
        self.project_authority_sessions(&output.snapshot.session_updates);
        self.world_time.ticks = output.snapshot.tick;
        Some(output.snapshot)
    }

    /// Deliver a session-targeted runtime projection. Wire packets share the
    /// join-client handler; embedded columns insert `Arc<Chunk>` without dense
    /// decode or full-column lighting recompute.
    pub(super) fn deliver_projection_event(
        &mut self,
        event: crate::server_runtime::PresentationEvent,
        session_id: crate::network::protocol::PlayerId,
    ) {
        use crate::server_runtime::{PresentationEvent, ProjectionDest};

        match event {
            PresentationEvent::Packet(event) => {
                let ProjectionDest::Session(target) = event.dest else {
                    return;
                };
                if target != session_id {
                    return;
                }
                self.handle_inbound_packet(event.packet);
            }
            PresentationEvent::ChunkColumn {
                to,
                dimension,
                cx,
                cz,
                revision,
                chunk,
            } => {
                if to != session_id {
                    return;
                }
                self.apply_embedded_chunk_column(dimension, cx, cz, revision, chunk);
            }
        }
    }

    /// Insert an in-process authority column. Lighting is already computed on
    /// the authority `Chunk`; presentation must not call
    /// `recompute_direct_column_lighting`.
    pub(super) fn apply_embedded_chunk_column(
        &mut self,
        dimension_wire: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        chunk: std::sync::Arc<crate::world::Chunk>,
    ) {
        let Some(dimension) = crate::dimension::Dimension::from_wire(dimension_wire) else {
            return;
        };
        if dimension != self.current_dimension {
            return;
        }
        let revision_key = (dimension, cx, cz);
        if revision
            < self
                .client_chunk_revisions
                .get(&revision_key)
                .copied()
                .unwrap_or(0)
        {
            return;
        }
        self.client_chunk_revisions.insert(revision_key, revision);
        let inserted_new = !self.chunk_manager.chunks.contains_key(&(cx, cz));
        // Clone out of the Arc so presentation owns a mutable CPU copy while
        // authority keeps mutating its resident map. One structural clone —
        // no dense flatten / palette rebuild / column lighting.
        self.chunk_manager
            .insert_resident_chunk((cx, cz), (*chunk).clone());
        if inserted_new {
            let lifetime = self.next_chunk_lifetime();
            self.chunk_lifetimes.insert((cx, cz), lifetime);
            self.chunk_meshes.insert((cx, cz), ChunkMesh::pending());
        }
        self.invalidate_chunk_mesh(
            (cx, cz),
            if inserted_new {
                DependencyReason::ChunkLoad
            } else {
                DependencyReason::Network
            },
        );
        // Drop any buffered join-style pending changes; the Arc column is the
        // authority snapshot at `revision`.
        self.pending_block_changes.remove(&(cx, cz));
    }

    pub(super) fn session_slot_from_stack(
        stack: Option<crate::inventory::ItemStack>,
    ) -> Option<crate::authority::contract::SessionInventorySlot> {
        stack
            .as_ref()
            .and_then(crate::authority::contract::SessionInventorySlot::from_stack)
    }

    pub(super) fn stack_from_session_slot(
        slot: crate::authority::contract::SessionInventorySlot,
    ) -> Option<crate::inventory::ItemStack> {
        slot.to_stack()
    }

    pub(super) fn local_inventory_writeback(
        &self,
    ) -> (
        [Option<crate::authority::contract::SessionInventorySlot>;
            crate::authority::contract::SESSION_INVENTORY_SLOTS],
        Option<crate::authority::contract::SessionInventorySlot>,
        u8,
    ) {
        use crate::authority::contract::SESSION_INVENTORY_SLOTS;
        let mut inventory = [None; SESSION_INVENTORY_SLOTS];
        let mut index = 0;
        for stack in self
            .inventory
            .hotbar
            .iter()
            .chain(self.inventory.main.iter())
            .chain(self.inventory.armor.iter())
        {
            if index >= SESSION_INVENTORY_SLOTS - 1 {
                break;
            }
            inventory[index] = Self::session_slot_from_stack(*stack);
            index += 1;
        }
        inventory[index] = Self::session_slot_from_stack(self.inventory.offhand);
        (
            inventory,
            Self::session_slot_from_stack(self.inventory.dragged),
            self.inventory.selected.min(8) as u8,
        )
    }

    pub(crate) fn sync_authority_gameplay_from_local(&mut self) {
        let (inventory, cursor, selected_hotbar_slot) = self.local_inventory_writeback();
        let Some(runtime) = self.embedded_runtime.as_mut() else {
            return;
        };
        let _ = runtime.sync_local_inventory(inventory, cursor, selected_hotbar_slot);
    }

    pub(super) fn project_authority_sessions(
        &mut self,
        updates: &[crate::authority::contract::SessionGameplayUpdate],
    ) {
        let Some(session_id) = self
            .embedded_runtime
            .as_ref()
            .map(EmbeddedRuntimeBridge::session_id)
        else {
            return;
        };
        let Some(update) = updates.iter().find(|update| update.player_id == session_id) else {
            return;
        };
        self.project_authority_session(update);
    }

    pub(super) fn project_authority_session(
        &mut self,
        update: &crate::authority::contract::SessionGameplayUpdate,
    ) {
        let gameplay = update.state;
        if !self.accept_session_projection(update.dimension, gameplay.revision, gameplay.revision) {
            return;
        }
        self.project_gameplay_state(update.dimension, gameplay);
    }

    /// Shared one-way presentation projection for embedded and socket session
    /// updates. Ordering is checked before any renderer-owned cache is touched:
    /// sequence orders dimension transfers, while revision orders snapshots in
    /// one dimension. The authority remains the sole writer of gameplay state.
    pub(super) fn accept_session_projection(&mut self, dimension: u8, sequence: u64, revision: u64) -> bool {
        if let Some((latest_dimension, latest_sequence, latest_revision)) =
            self.client_session_projection
        {
            if sequence <= latest_sequence {
                return false;
            }
            if latest_dimension == dimension && revision <= latest_revision {
                return false;
            }
        }
        self.client_session_projection = Some((dimension, sequence, revision));
        true
    }

    pub(super) fn project_gameplay_state(
        &mut self,
        dimension: u8,
        gameplay: crate::authority::contract::SessionGameplayState,
    ) {
        self.player_state.health = gameplay.health_milli as f32 / 1000.0;
        self.player_state.max_health = gameplay.max_health_milli as f32 / 1000.0;
        self.player_state.hunger = gameplay.hunger_milli as f32 / 1000.0;
        self.player_state.saturation = gameplay.saturation_milli as f32 / 1000.0;
        self.player_state.is_dead = gameplay.is_dead;
        self.player_state.death_reason = gameplay.death_source.and_then(DamageSource::from_wire);
        self.player_state.invulnerable_time = gameplay.invulnerability_ticks as f32 / 20.0;
        self.player_state.experience = gameplay.experience;
        self.player_state.experience_level = gameplay.experience_level;
        self.player_state.attack_cooldown_ticks = u32::from(gameplay.attack_cooldown_ticks);
        self.player_state.attack_cooldown_max_ticks = self
            .player_state
            .attack_cooldown_max_ticks
            .max(self.player_state.attack_cooldown_ticks);
        self.player_state.shield_disable_ticks = u32::from(gameplay.shield_cooldown_ticks);
        self.enchanting.seed = gameplay.enchant_seed.min(u64::from(u32::MAX)) as u32;
        if gameplay.shield_active {
            self.player_state.using_item = Some(crate::player::UsingItemState {
                hand: crate::player::Hand::MainHand,
                action: crate::player::ItemUseAction::Block,
                item: Item::Shield,
                slot: crate::player::HandSlot::MainHand(self.inventory.selected),
                ticks_held: 0,
                max_ticks: None,
            });
        } else if self
            .player_state
            .using_item
            .as_ref()
            .is_some_and(|using| using.action == crate::player::ItemUseAction::Block)
        {
            self.player_state.using_item = None;
        }
        self.player_physics.velocity = Vec3::new(
            gameplay.velocity_milli[0] as f32 / 1000.0,
            gameplay.velocity_milli[1] as f32 / 1000.0,
            gameplay.velocity_milli[2] as f32 / 1000.0,
        );
        self.inventory.selected = usize::from(gameplay.selected_hotbar_slot.min(8));
        let mut index = 0;
        for slot in self
            .inventory
            .hotbar
            .iter_mut()
            .chain(self.inventory.main.iter_mut())
            .chain(self.inventory.armor.iter_mut())
        {
            *slot = gameplay.inventory[index].and_then(Self::stack_from_session_slot);
            index += 1;
        }
        self.inventory.offhand = gameplay.inventory[index].and_then(Self::stack_from_session_slot);
        if let Some(mining) = gameplay.mining {
            if !self.mining_cancel_sent {
                self.mining_target = Some(Vec3::new(
                    mining.target[0] as f32,
                    mining.target[1] as f32,
                    mining.target[2] as f32,
                ));
                self.mining_progress = f32::from(mining.progress_milli) / 1000.0;
                self.mining_held = mining.held;
            }
        } else {
            self.mining_target = None;
            self.mining_progress = 0.0;
            self.mining_held = None;
            self.mining_cancel_sent = false;
        }

        if let Some(dimension) = crate::dimension::Dimension::from_wire(dimension) {
            self.reset_presented_dimension(dimension);
        }

        self.presented_fishing_hook_entity = match gameplay.fishing_hook {
            Some(hook)
                if crate::fishing::FishingHookStage::from_wire(hook.stage).is_some() =>
            {
                Some(hook.entity_id)
            }
            _ => None,
        };

        if let Some(brew) = gameplay.brew {
            self.active_station = Some(StationKind::Brewing);
            self.container_target = Some((brew.station[0], brew.station[1], brew.station[2]));
            self.brewing.progress = (10.0 - brew.remaining_ticks as f32 / 20.0).clamp(0.0, 10.0);
            self.brewing.ingredient = Self::stack_from_session_slot(
                crate::authority::contract::SessionInventorySlot::from_wire(
                    brew.ingredient.expected.item,
                    brew.ingredient.expected.can_break,
                    brew.ingredient.expected.can_place_on,
                ),
            );
            self.brewing.bottles = brew.bottles.map(|source| {
                source.and_then(|source| {
                    Self::stack_from_session_slot(
                        crate::authority::contract::SessionInventorySlot::from_wire(
                            source.expected.item,
                            source.expected.can_break,
                            source.expected.can_place_on,
                        ),
                    )
                })
            });
        } else if self.active_station == Some(StationKind::Brewing) {
            self.active_station = None;
            self.container_target = None;
        }
        if gameplay.is_dead {
            self.clear_movement_input();
            self.sync_cursor_mode();
        }
    }

    /// Apply authority output to the renderer-owned cache.  This is a one-way
    /// projection: the GPU-side chunk manager is never consulted by the core
    /// and never performs an authoritative mutation for Singleplayer/Host.
    pub(super) fn project_authority_mutations(
        &mut self,
        mutations: &[crate::authority::contract::WorldMutation],
    ) {
        let mut dirty_chunks = std::collections::HashSet::new();
        for mutation in mutations {
            let Some(dimension) = crate::dimension::Dimension::from_wire(mutation.dimension) else {
                continue;
            };
            if dimension != self.current_dimension {
                continue;
            }
            let Some(block) = BlockType::from_wire(mutation.block) else {
                continue;
            };
            let (x, y, z) = mutation.position;
            let Some(((cx, cz), _)) = self.chunk_manager.world_to_local(x, y, z) else {
                continue;
            };
            let revision_key = (dimension, cx, cz);
            if mutation.revision
                <= self
                    .client_chunk_revisions
                    .get(&revision_key)
                    .copied()
                    .unwrap_or(0)
            {
                continue;
            }
            self.client_chunk_revisions
                .insert(revision_key, mutation.revision);
            if !self.chunk_manager.chunks.contains_key(&(cx, cz)) {
                // Column not resident yet; Arc/ChunkData projection carries the
                // authority snapshot. Skip buffering — embedded no longer dual-
                // applies via BlockChange.
                continue;
            }
            let previous_block = self.chunk_manager.get_block(x, y, z);
            let previous_state = self.chunk_manager.get_block_state(x, y, z);
            self.play_chest_state_edge(
                (x, y, z),
                previous_block,
                previous_state,
                block,
                mutation.state,
            );
            if let Some(dirty) = apply_synced_block_change(
                &mut self.chunk_manager,
                x,
                y,
                z,
                block,
                mutation.state,
                mutation.raw_fluid,
            ) {
                dirty_chunks.extend(dirty);
            }
            // Block-entity / container payloads still arrive as typed projection
            // events; never query a second local authority as a fallback.
        }
        if !dirty_chunks.is_empty() {
            self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Block);
        }
    }

    pub(super) fn play_chest_state_edge(
        &self,
        position: (i32, i32, i32),
        previous_block: BlockType,
        previous_state: u8,
        block: BlockType,
        state: u8,
    ) {
        if !matches!(previous_block, BlockType::Chest | BlockType::EndCityChest)
            || !matches!(block, BlockType::Chest | BlockType::EndCityChest)
        {
            return;
        }
        let previous = crate::world::BlockState::decode(previous_state);
        let next = crate::world::BlockState::decode(state);
        if previous.is_open == next.is_open {
            return;
        }
        // A double chest emits two block-state mutations. Pick the
        // lexicographically first half so one edge produces one sound.
        if let Some(partner) =
            crate::block_entity::double_chest_partner(&self.chunk_manager, position)
        {
            if position > partner {
                return;
            }
        }
        self.audio_manager.play_sound(if next.is_open {
            crate::audio::SoundId::ChestOpen
        } else {
            crate::audio::SoundId::ChestClose
        });
    }

    pub fn submit_authority_request(
        &mut self,
        request: crate::network::protocol::GameplayRequest,
    ) -> Option<crate::network::protocol::GameplayResponse> {
        if let Some(runtime) = self.embedded_runtime.as_mut() {
            if let Err(error) = runtime.queue_request(request) {
                self.save_error
                    .get_or_insert_with(|| format!("embedded runtime input rejected: {error}"));
            }
            // ACKs are deliberately observed only from `tick_with_output`; a
            // queued request has no synchronous response to project.
            return None;
        }
        if matches!(self.role, MultiplayerRole::Client { .. }) {
            self.network.request_gameplay(request);
        }
        None
    }

    /// Submit a local input envelope through the same authority path used by
    /// a listen/dedicated transport.  Presentation code must not mutate the
    /// renderer cache first and then ask the core for an ACK: a rejected or
    /// stale request is intentionally a no-op on the client.
    pub(super) fn submit_local_authority_operation(
        &mut self,
        operation: crate::network::protocol::GameplayOperation,
    ) -> Option<crate::network::protocol::GameplayResponse> {
        self.submit_authority_request(crate::network::protocol::GameplayRequest {
            request_id: 0,
            client_sequence: 0,
            session_id: 0,
            dimension: self.current_dimension as u8,
            client_revision: self
                .embedded_runtime
                .as_ref()
                .map(|runtime| runtime.revision_for_dimension(self.current_dimension))
                .or_else(|| {
                    self.client_session_projection
                        .map(|(_, _, revision)| revision)
                })
                .unwrap_or_default(),
            operation,
        })
    }

    pub(super) fn submit_local_authority_container_action(
        &mut self,
        position: (i32, i32, i32),
        action: crate::network::protocol::ContainerAction,
        slot: u16,
    ) -> bool {
        let operation = crate::network::protocol::GameplayOperation::Container {
            action,
            x: position.0,
            y: position.1,
            z: position.2,
            slot,
        };
        let Some(response) = self.submit_local_authority_operation(operation) else {
            return false;
        };
        if !matches!(
            response.outcome,
            crate::network::protocol::GameplayOutcome::Accepted { .. }
        ) {
            return false;
        }
        match action {
            crate::network::protocol::ContainerAction::Open => false,
            crate::network::protocol::ContainerAction::Close => {
                self.container_target = None;
                self.container_is_double = false;
                self.inventory.is_open = false;
                self.active_station = None;
                self.sync_cursor_mode();
                true
            }
        }
    }

    /// Cancel the live authority mining latch. `mining_cancel_sent` still
    /// lives in `submit_local_authority_block_action` so duplicate CancelBreak
    /// packets are suppressed.
    pub(super) fn cancel_authority_break(&mut self) {
        if let Some(previous) = self.mining_target {
            let _ = self.submit_local_authority_block_action(
                crate::network::protocol::BlockActionKind::CancelBreak,
                previous.x as i32,
                previous.y as i32,
                previous.z as i32,
                [0, 0, 0],
                BlockType::Air,
            );
        }
        self.mining_target = None;
        self.mining_held = None;
        self.mining_progress = 0.0;
    }

    pub(super) fn submit_local_authority_block_action(
        &mut self,
        action: crate::network::protocol::BlockActionKind,
        x: i32,
        y: i32,
        z: i32,
        face: [i8; 3],
        block: BlockType,
    ) -> Option<crate::network::protocol::GameplayResponse> {
        if matches!(
            action,
            crate::network::protocol::BlockActionKind::CancelBreak
        ) {
            if self.mining_cancel_sent {
                return None;
            }
            self.mining_cancel_sent = true;
        } else if matches!(
            action,
            crate::network::protocol::BlockActionKind::StartBreak
        ) {
            self.mining_cancel_sent = false;
        }
        let look = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        )
        .normalize_or_zero();
        let look_milli = [
            (look.x * 1_000.0).round() as i16,
            (look.y * 1_000.0).round() as i16,
            (look.z * 1_000.0).round() as i16,
        ];
        let hand = 0;
        let held = self.inventory.hotbar[self.inventory.selected].map(|stack| {
            let slot = crate::authority::contract::SessionInventorySlot::from_wire(
                crate::network::protocol::ItemWire::from_stack(&stack),
                stack.can_break,
                stack.can_place_on,
            );
            crate::network::protocol::SessionSlotWire::from(slot)
        });
        self.submit_authority_request(crate::network::protocol::GameplayRequest {
            request_id: 0,
            client_sequence: 0,
            session_id: 0,
            dimension: self.current_dimension as u8,
            client_revision: self
                .embedded_runtime
                .as_ref()
                .map(|runtime| runtime.revision_for_dimension(self.current_dimension))
                .or_else(|| {
                    self.client_session_projection
                        .map(|(_, _, revision)| revision)
                })
                .unwrap_or_default(),
            operation: crate::network::protocol::GameplayOperation::BlockAction {
                action,
                x,
                y,
                z,
                face,
                hand,
                held: if matches!(
                    action,
                    crate::network::protocol::BlockActionKind::CancelBreak
                ) {
                    None
                } else {
                    held
                },
                block: if matches!(
                    action,
                    crate::network::protocol::BlockActionKind::Place
                        | crate::network::protocol::BlockActionKind::IgnitePortal
                        | crate::network::protocol::BlockActionKind::InsertEnderEye
                        | crate::network::protocol::BlockActionKind::EnterPortal
                ) {
                    block.to_wire()
                } else {
                    BlockType::Air.to_wire()
                },
                look_milli,
            },
        })
    }

    pub(super) fn selected_mining_held(&self) -> Option<crate::network::protocol::SessionSlotWire> {
        self.inventory.hotbar[self.inventory.selected].map(|stack| {
            let slot = crate::authority::contract::SessionInventorySlot::from_wire(
                crate::network::protocol::ItemWire::from_stack(&stack),
                stack.can_break,
                stack.can_place_on,
            );
            crate::network::protocol::SessionSlotWire::from(slot)
        })
    }

    /// Route the command domains already understood by `ServerWorld` through
    /// the in-process authority. Unsupported legacy command domains continue
    /// through the existing presentation adapter until the remaining Phase A
    /// cutover lands; they must never be reported as an authority ACK here.
    pub(super) fn submit_local_authority_command(
        &mut self,
        command: &str,
    ) -> Option<crate::network::protocol::GameplayResponse> {
        self.submit_authority_request(crate::network::protocol::GameplayRequest {
            request_id: 0,
            client_sequence: 0,
            session_id: 0,
            dimension: self.current_dimension as u8,
            client_revision: self
                .embedded_runtime
                .as_ref()
                .map(|runtime| runtime.revision_for_dimension(self.current_dimension))
                .unwrap_or_default(),
            operation: crate::network::protocol::GameplayOperation::Command {
                command: command.to_string(),
            },
        })
    }

    pub(super) fn can_place_block_at(&self, x: i32, y: i32, z: i32, block: BlockType) -> bool {
        let policy = self.game_mode_policy();
        if (!policy.can_place && self.game_mode != GameMode::Adventure)
            || !policy.can_place_stack(
                self.inventory.hotbar[self.inventory.selected].as_ref(),
                block,
            )
        {
            return false;
        }
        matches!(
            placement_decision_for_players(
                block,
                (x, y, z),
                self.player_physics.get_aabb(),
                self.remote_players.values(),
            ),
            BlockPlacementDecision::Allowed
        )
    }

    pub(super) fn can_break_current_block(&self, block: BlockType) -> bool {
        can_break_block(block, self.game_mode)
            && self.game_mode_policy().can_break_stack(
                self.inventory.hotbar[self.inventory.selected].as_ref(),
                block,
            )
    }

    pub(super) fn drain_network_events(&mut self) {
        // Transport draining is bounded by `NetworkHandle`; every event it
        // yields is classified immediately so an apply-budget boundary can
        // never demote a latest-wins event into the reliable FIFO.
        const MAX_EVENTS: usize = NETWORK_MAX_EVENTS_PER_PASS;
        const MAX_BYTES: usize = NETWORK_MAX_BYTES_PER_PASS;
        const MAX_TIME: Duration = NETWORK_MAX_TIME_PER_PASS;
        for event in self.network.drain_inbound() {
            self.network_staging.stage(event);
        }

        let apply_started = Instant::now();
        let mut applied = 0usize;
        let mut applied_bytes = 0usize;
        while applied < MAX_EVENTS
            && applied_bytes < MAX_BYTES
            && apply_started.elapsed() < MAX_TIME
        {
            let remaining_bytes = MAX_BYTES.saturating_sub(applied_bytes);
            let Some((event, event_bytes)) = self.network_staging.pop_next_if_fits(remaining_bytes)
            else {
                break;
            };
            applied_bytes = applied_bytes.saturating_add(event_bytes);
            applied += 1;
            self.handle_single_network_event(event);
        }
        self.perf_counters.network_inbound_reliable_pending =
            self.network_staging.reliable_len() as u64;
        self.perf_counters.network_inbound_reliable_bytes = self.network_staging.reliable_bytes();
        self.perf_counters.network_inbound_latest_pending =
            self.network_staging.latest_len() as u64;
        self.perf_counters.network_inbound_latest_bytes = self.network_staging.latest_bytes();
    }

    pub(super) fn clear_replicated_entities(&mut self) {
        let local_ids: Vec<_> = self
            .replicated_entities
            .drain()
            .map(|(_, replicated)| replicated.local_entity_id)
            .collect();
        for local_id in local_ids {
            self.entity_manager.remove_by_id(local_id);
        }
    }

    pub(super) fn apply_replicated_entity_state(
        &mut self,
        dimension_wire: u8,
        sequence: u64,
        state: crate::network::protocol::EntityStateWire,
    ) {
        if !self.presentation_topology().is_join_client()
            || crate::dimension::Dimension::from_wire(dimension_wire)
                != Some(self.current_dimension)
        {
            return;
        }
        let Some(entity_type) = crate::entity::EntityType::from_wire(state.entity_type) else {
            return;
        };
        if !is_replicated_entity_type(entity_type) {
            return;
        }

        let needs_spawn = self
            .replicated_entities
            .get(&state.entity_id)
            .and_then(|replicated| self.entity_manager.get_by_id(replicated.local_entity_id))
            .map_or(true, |entity| entity.entity_type != entity_type);
        if needs_spawn {
            if let Some(previous) = self.replicated_entities.remove(&state.entity_id) {
                self.entity_manager.remove_by_id(previous.local_entity_id);
            }
            let local_entity_id = self
                .entity_manager
                .spawn(entity_type, Vec3::from_array(state.position));
            self.replicated_entities
                .insert(state.entity_id, ReplicatedEntityState::new(local_entity_id));
        }

        let snapped = self
            .replicated_entities
            .get_mut(&state.entity_id)
            .is_some_and(|replicated| replicated.push(state, sequence, self.network_time));
        if snapped {
            self.perf_counters.prediction_rollback =
                self.perf_counters.prediction_rollback.saturating_add(1);
        }
        if let Some(local_id) = self
            .replicated_entities
            .get(&state.entity_id)
            .map(|replicated| replicated.local_entity_id)
        {
            if let Some(entity) = self.entity_manager.get_by_id_mut(local_id) {
                apply_entity_wire_state(entity, state);
            }
        }
    }

    pub(super) fn apply_replicated_entity_despawn(
        &mut self,
        dimension_wire: u8,
        sequence: u64,
        entity_id: u64,
    ) {
        if !self.presentation_topology().is_join_client()
            || crate::dimension::Dimension::from_wire(dimension_wire)
                != Some(self.current_dimension)
        {
            return;
        }
        if self
            .replicated_entities
            .get(&entity_id)
            .and_then(|replicated| replicated.snapshots.back())
            .is_some_and(|latest| sequence <= latest.sequence)
        {
            return;
        }
        if let Some(replicated) = self.replicated_entities.remove(&entity_id) {
            self.entity_manager.remove_by_id(replicated.local_entity_id);
        }
    }

    pub(super) fn update_replicated_entity_interpolation(&mut self) {
        if !self.presentation_topology().is_join_client() {
            return;
        }
        let target = self.network_time - ENTITY_INTERPOLATION_DELAY;
        let samples: Vec<_> = self
            .replicated_entities
            .values()
            .filter_map(|replicated| {
                replicated
                    .sample(target)
                    .map(|state| (replicated.local_entity_id, state))
            })
            .collect();
        let moved_ids: Vec<_> = samples.iter().map(|(local_id, _)| *local_id).collect();
        for (local_id, state) in samples {
            if let Some(entity) = self.entity_manager.get_by_id_mut(local_id) {
                apply_entity_wire_state(entity, state);
            }
        }
        self.entity_manager.sync_entity_positions(&moved_ids);
    }
}

