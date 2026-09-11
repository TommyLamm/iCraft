//! Inbound network event dispatch extracted from `state.rs`.
//! Join and embedded both deliver [`crate::network::protocol::Packet`] (via
//! thin `ClientToGame` for StatusUpdate | Packet).

use super::*;
use crate::network::protocol::Packet;

impl State {
    pub(super) fn handle_single_network_event(&mut self, event: NetworkInbound) {
        match event {
            NetworkInbound::StatusUpdate { message } => {
                self.network_status = Some(message);
            }
            NetworkInbound::Packet(packet) => self.handle_inbound_packet(packet),
        }
    }

    pub(super) fn handle_inbound_packet(&mut self, packet: Packet) {
        match packet {
            Packet::GameplayResponse { response, .. } => {
                // Embedded /gamemode: SessionContract is authoritative; refresh
                // presentation mode when an accepted response arrives in-process.
                if matches!(
                    response.outcome,
                    crate::network::protocol::GameplayOutcome::Accepted { .. }
                ) {
                    if let Some(mode) = self
                        .embedded_runtime
                        .as_ref()
                        .and_then(EmbeddedRuntimeBridge::session_game_mode)
                    {
                        self.set_game_mode(mode);
                    }
                }
            }
            Packet::LoginSuccess {
                player_id,
                seed,
                gamemode,
                ..
            } => {
                self.local_player_id = Some(player_id);
                self.world_seed = seed as u32;
                let game_mode = match gamemode {
                    0 => GameMode::Creative,
                    2 => GameMode::Adventure,
                    3 => GameMode::Spectator,
                    _ => GameMode::Survival,
                };
                self.set_game_mode(game_mode);
                self.inventory = match self.game_mode {
                    GameMode::Creative => Inventory::new_creative(),
                    GameMode::Survival | GameMode::Adventure | GameMode::Spectator => {
                        Inventory::new()
                    }
                };
                self.weather = crate::weather::WeatherPresentation::new(self.world_seed);
                self.chunk_manager.chunks.clear();
                self.teardown_terrain_runtime("network connect/reset");
                self.pending_chunk_payloads.clear();
                self.pending_block_changes.clear();
                self.client_chunk_revisions.clear();
                self.clear_replicated_entities();
                self.client_player_health_sequence = 0;
                self.client_player_effect_sequence = 0;
                self.client_session_projection = None;
                self.network_ready = true;
                self.network_status = None;
                self.connection_lost = false;
                push_chat_history(
                    &mut self.chat_messages,
                    "[Network]".into(),
                    format!("Connected to server as player #{player_id}"),
                );
            }
            Packet::Disconnect { reason, .. } => {
                eprintln!("[State] Network disconnected: {reason}");
                self.teardown_terrain_runtime("network disconnect");
                self.network_ready = false;
                let disconnected = self.translate("disconnect.generic");
                self.network_status = Some(format!("{disconnected}: {reason}"));
                self.connection_lost = true;
                self.is_chat_open = false;
                self.chat_input.clear();
                clear_remote_players(&mut self.remote_players, &mut self.entity_manager);
                self.force_close_inventory();
                self.clear_replicated_entities();
                self.client_session_projection = None;
                self.set_paused(true);
                push_chat_history(
                    &mut self.chat_messages,
                    "[Network]".into(),
                    format!("{disconnected}: {reason}"),
                );
            }
            Packet::PlayerJoin { id, username, .. } => {
                if self.local_player_id != Some(id) {
                    if let Some(remote) = self.remote_players.get_mut(&id) {
                        remote.username = username.clone();
                        if let Some(entity) = self.entity_manager.get_by_id_mut(remote.entity_id) {
                            entity.username = username.clone();
                        }
                    } else {
                        let entity_id = self.entity_manager.spawn(
                            crate::entity::EntityType::RemotePlayer,
                            self.player_physics.position,
                        );
                        if let Some(entity) = self.entity_manager.get_by_id_mut(entity_id) {
                            entity.player_id = id;
                            entity.username = username.clone();
                        }
                        self.remote_players
                            .insert(id, RemotePlayerState::new(entity_id, username.clone()));
                    }
                    push_chat_history(
                        &mut self.chat_messages,
                        "[Network]".into(),
                        format!("{username} joined the game"),
                    );
                }
            }
            Packet::PlayerLeave { id, .. } => {
                if let Some(remote) = self.remote_players.remove(&id) {
                    push_chat_history(
                        &mut self.chat_messages,
                        "[Network]".into(),
                        format!("{} left the game", remote.username),
                    );
                    self.entity_manager.remove_by_id(remote.entity_id);
                } else {
                    push_chat_history(
                        &mut self.chat_messages,
                        "[Network]".into(),
                        format!("Player #{id} left the game"),
                    );
                }
            }
            Packet::PlayerPosition {
                id,
                sequence,
                sender_time_millis,
                x,
                y,
                z,
                yaw,
                pitch,
                ..
            } => {
                if self.local_player_id == Some(id) {
                    let authoritative = Vec3::new(x, y, z);
                    if self.player_physics.position.distance(authoritative)
                        > PLAYER_CORRECTION_SNAP_DISTANCE
                    {
                        self.player_physics.position = authoritative;
                        self.player_physics.velocity = Vec3::ZERO;
                        self.prev_player_position = authoritative;
                        self.camera.yaw = yaw;
                        self.camera.pitch = pitch;
                        self.perf_counters.prediction_rollback =
                            self.perf_counters.prediction_rollback.saturating_add(1);
                    }
                    return;
                }
                let candidate = Vec3::new(x, y, z);
                let position = if matches!(self.role, MultiplayerRole::Host { .. }) {
                    validated_remote_position(
                        self.remote_players
                            .get(&id)
                            .and_then(|remote| remote.snapshots.back()),
                        candidate,
                        sender_time_millis,
                    )
                } else {
                    candidate
                };
                if !self.remote_players.contains_key(&id) {
                    let username = String::new();
                    let entity_id = self
                        .entity_manager
                        .spawn(crate::entity::EntityType::RemotePlayer, position);
                    if let Some(entity) = self.entity_manager.get_by_id_mut(entity_id) {
                        entity.player_id = id;
                    }
                    self.remote_players
                        .insert(id, RemotePlayerState::new(entity_id, username));
                }

                if let Some(remote) = self.remote_players.get_mut(&id) {
                    let arrival = self.network_time;
                    let result = remote.push_snapshot(
                        position,
                        yaw,
                        pitch,
                        sequence,
                        sender_time_millis,
                        arrival,
                    );

                    if let Some(entity) = self.entity_manager.get_by_id_mut(remote.entity_id) {
                        let (snap_pos, snap_yaw, snap_pitch) =
                            if result == SnapshotPushResult::Snapped {
                                (position, yaw, pitch)
                            } else if let Some(samp) =
                                remote.sample(arrival - REMOTE_INTERPOLATION_DELAY)
                            {
                                (samp.position, samp.yaw, samp.pitch)
                            } else {
                                (Vec3::new(x, y, z), yaw, pitch)
                            };
                        entity.position = snap_pos;
                        entity.yaw = snap_yaw;
                        entity.pitch = snap_pitch;
                    }
                }
            }
            Packet::PlayerAction { id, action, .. } => {
                if let Some(remote) = self.remote_players.get(&id) {
                    if let Some(entity) = self.entity_manager.get_by_id_mut(remote.entity_id) {
                        entity.action_cooldown = match action {
                            crate::network::protocol::Action::Place
                            | crate::network::protocol::Action::Break
                            | crate::network::protocol::Action::Use => 0.25,
                        };
                    }
                }
            }
            Packet::BlockChange {
                dimension,
                revision,
                x,
                y,
                z,
                block,
                state,
                raw_fluid,
                ..
            } => {
                self.apply_remote_block_change(
                    dimension, revision, x, y, z, block, state, raw_fluid,
                );
            }
            Packet::BlockEntityDelta {
                dimension,
                revision,
                x,
                y,
                z,
                entity,
                ..
            } => {
                self.apply_remote_block_entity_delta(dimension, revision, x, y, z, entity);
            }
            Packet::ChunkData {
                dimension,
                cx,
                cz,
                revision,
                min_section_y,
                section_count,
                blocks,
                block_states,
                fluid_levels,
                block_entities,
                ..
            } => {
                self.apply_remote_chunk_data(
                    dimension,
                    cx,
                    cz,
                    revision,
                    min_section_y,
                    section_count,
                    blocks,
                    block_states,
                    fluid_levels,
                    block_entities,
                );
            }
            Packet::EntitySpawn {
                dimension,
                sequence,
                state,
                ..
            }
            | Packet::EntityState {
                dimension,
                sequence,
                state,
                ..
            } => {
                self.apply_replicated_entity_state(dimension, sequence, state);
            }
            Packet::EntityDespawn {
                dimension,
                sequence,
                entity_id,
                ..
            } => {
                self.apply_replicated_entity_despawn(dimension, sequence, entity_id);
            }
            Packet::PlayerHealth {
                sequence,
                player_id,
                health,
                max_health,
                hunger,
                saturation,
                oxygen,
                is_dead,
                death_reason,
                ..
            } => {
                if self.local_player_id == Some(player_id)
                    && self.presentation_topology().is_join_client()
                    && sequence > self.client_player_health_sequence
                {
                    self.client_player_health_sequence = sequence;
                    self.player_state.health = health.clamp(0.0, max_health.max(0.0));
                    self.player_state.max_health = max_health.max(0.0);
                    self.player_state.hunger = hunger.clamp(0.0, 20.0);
                    self.player_state.saturation = saturation.clamp(0.0, 20.0);
                    self.player_state.oxygen = oxygen.clamp(0.0, 300.0);
                    self.player_state.is_dead = is_dead;
                    self.player_state.death_reason = DamageSource::from_wire(death_reason);
                    if is_dead {
                        self.clear_movement_input();
                        self.sync_cursor_mode();
                    }
                }
            }
            Packet::PlayerEffect {
                sequence,
                player_id,
                effects,
                ..
            } => {
                if self.local_player_id == Some(player_id)
                    && self.presentation_topology().is_join_client()
                    && sequence > self.client_player_effect_sequence
                {
                    self.client_player_effect_sequence = sequence;
                    self.potion_effects.active =
                        effects.into_iter().filter_map(effect_from_wire).collect();
                }
            }
            Packet::PlayerSessionUpdate {
                sequence,
                player_id,
                dimension,
                state,
                ..
            } => {
                if self.local_player_id != Some(player_id) {
                    return;
                }
                if self.accept_session_projection(dimension, sequence, state.revision) {
                    self.client_player_health_sequence =
                        self.client_player_health_sequence.max(sequence);
                    self.project_gameplay_state(
                        dimension,
                        crate::authority::contract::SessionGameplayState::from(state),
                    );
                }
            }
            Packet::TimeSync {
                ticks,
                weather,
                weather_remaining_ticks,
                ..
            } => {
                if self.presentation_topology().is_join_client() {
                    self.world_time.ticks = ticks;
                    self.world_time.tick_accumulator = 0.0;
                    if !self.weather.apply_wire(weather) {
                        // Ignore malformed weather bytes; keep last good phase.
                    }
                    let _ = weather_remaining_ticks;
                }
            }
            Packet::WorldRulesSync { rules, .. } => {
                if self.presentation_topology().is_join_client() {
                    self.set_world_rules(rules);
                }
            }
            Packet::LightningStrike { strike, .. } => {
                if self.presentation_topology().is_join_client()
                    && self.current_dimension == crate::dimension::Dimension::Overworld
                {
                    self.apply_lightning_strike(strike);
                }
            }
            Packet::ChatMessage { sender, message, .. } => {
                let Some(message) = normalized_chat_message(&message) else {
                    return;
                };
                push_chat_history(&mut self.chat_messages, sender, message);
            }
            Packet::ContainerClose {
                dimension, x, y, z, ..
            } => {
                // Join client already filtered to the active container; embedded
                // session targeting is done before this call. No player-id field
                // on the wire Packet — match by open target only.
                if dimension == self.current_dimension as u8
                    && self.container_target == Some((x, y, z))
                {
                    self.force_close_inventory();
                }
            }
            Packet::ContainerOpenResult {
                dimension,
                success,
                x,
                y,
                z,
                slots,
                revision,
                ..
            } => {
                if !success {
                    if dimension == self.current_dimension as u8
                        && self.container_target == Some((x, y, z))
                    {
                        self.force_close_inventory();
                    }
                } else if dimension == self.current_dimension as u8 {
                    let current_revision = self
                        .chunk_manager
                        .get_block_entity(x, y, z)
                        .map(crate::block_entity::BlockEntity::revision)
                        .unwrap_or(0);
                    if !container_revision_is_newer(current_revision, revision)
                        && current_revision != 0
                    {
                        return;
                    }
                    let mut committed = true;
                    if !slots.is_empty() {
                        let stacks: Vec<Option<crate::inventory::ItemStack>> = slots
                            .iter()
                            .map(|slot| slot.as_ref().and_then(|wire| wire.to_stack()))
                            .collect();
                        committed = self.chunk_manager.set_container_slots(x, y, z, &stacks);
                    }
                    if !committed {
                        return;
                    }
                    if let Some(entity) = self.chunk_manager.get_block_entity_mut(x, y, z) {
                        entity.set_revision(revision);
                    }
                    self.container_target = Some((x, y, z));
                    self.open_inventory();
                }
            }
            Packet::ContainerClickResult {
                dimension,
                success,
                slot_index: _,
                slot: _,
                dragged,
                ..
            } => {
                if success
                    && dimension == self.current_dimension as u8
                    && self.container_target.is_some()
                {
                    // The click result intentionally carries no authoritative
                    // revision.  The paired ContainerSlotUpdate/BlockEntityDelta
                    // is the only source allowed to mutate mirrored slots;
                    // applying this payload here could reintroduce an older value
                    // when reliable packets are retried or reordered.
                    self.inventory.dragged = dragged.and_then(|w| w.to_stack());
                }
            }
            Packet::ContainerSlotUpdate {
                dimension,
                revision,
                x,
                y,
                z,
                slot_index,
                slot,
                ..
            } => {
                if dimension != self.current_dimension as u8
                    || self.container_target != Some((x, y, z))
                {
                    return;
                }
                let current_revision = self
                    .chunk_manager
                    .get_block_entity(x, y, z)
                    .map(crate::block_entity::BlockEntity::revision)
                    .unwrap_or(0);
                if !container_revision_is_newer(current_revision, revision) {
                    return;
                }
                if let Some(mut slots) = self.chunk_manager.container_slots(x, y, z) {
                    if (slot_index as usize) < slots.len() {
                        slots[slot_index as usize] = slot.and_then(|wire| wire.to_stack());
                        if !self.chunk_manager.set_container_slots(x, y, z, &slots) {
                            return;
                        }
                        if let Some(entity) = self.chunk_manager.get_block_entity_mut(x, y, z) {
                            entity.set_revision(revision);
                        }
                    }
                }
            }
            Packet::PlayerRespawnResult {
                position,
                dimension,
                ..
            } => {
                let target_vec = Vec3::from_array(position);
                self.player_physics.position = target_vec;
                self.player_physics.velocity = Vec3::ZERO;
                self.player_physics.on_ground = false;
                self.player_physics.highest_y = target_vec.y;

                let target_dim = crate::dimension::Dimension::from_wire(dimension)
                    .unwrap_or(crate::dimension::Dimension::Overworld);
                if self.current_dimension != target_dim {
                    self.switch_dimension(target_dim);
                }

                self.player_state.reset_for_respawn();
                self.sync_cursor_mode();
            }
            Packet::SleepStateSync {
                player_id,
                is_sleeping,
                ..
            } => {
                if matches!(&self.network, NetworkHandle::Client { .. }) {
                    if self.local_player_id == Some(player_id) {
                        self.player_state.is_sleeping = is_sleeping;
                        if !is_sleeping {
                            self.player_state.sleep_timer = 0.0;
                        }
                    } else if let Some(remote) = self.remote_players.get_mut(&player_id) {
                        remote.is_sleeping = is_sleeping;
                    }
                }
            }
            Packet::DimensionTransfer {
                player_id,
                dimension,
                position,
                ..
            } => {
                if self.local_player_id.is_some_and(|id| id != player_id)
                    && !matches!(&self.network, NetworkHandle::None)
                {
                    return;
                }
                if let Some(target) = crate::dimension::Dimension::from_wire(dimension) {
                    self.reset_presented_dimension(target);
                    self.player_physics.position = Vec3::from_array(position);
                    self.camera.position = Vec3::from_array(position) + Vec3::new(0.0, 1.6, 0.0);
                    self.portal_contact_time = 0.0;
                    self.portal_cooldown = 3.0;
                }
            }
            // Client→server or otherwise non-presentation packets: ignore.
            _ => {}
        }
    }
}
