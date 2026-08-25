//! Inbound network event dispatch extracted from `state.rs`.

use super::*;

impl State {
    pub(super) fn handle_single_network_event(&mut self, event: NetworkInbound) {
        match event {
            NetworkInbound::StatusUpdate(msg) => {
                self.network_status = Some(msg);
            }
            NetworkInbound::GameplayResponse { response } => {
                self.last_gameplay_response = Some(response);
            }
            NetworkInbound::GameplayRequest { id, mut request } => {
                // Embedded listen transport is owned by ServerRuntime.  A
                // State-side inbound GameplayRequest would be a second
                // authority path; retain this legacy arm only for clients,
                // where NetworkClient remains the presentation transport.
                request.session_id = id;
                let _ = request;
            }
            NetworkInbound::Connected {
                player_id,
                seed,
                gamemode,
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
                self.weather = crate::weather::WeatherSystem::new(self.world_seed);
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
            NetworkInbound::Disconnected(reason) => {
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
                self.container_sessions.sessions.clear();
                self.clear_replicated_entities();
                self.client_session_projection = None;
                self.set_paused(true);
                push_chat_history(
                    &mut self.chat_messages,
                    "[Network]".into(),
                    format!("{disconnected}: {reason}"),
                );
            }
            NetworkInbound::PlayerJoin { id, username } => {
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
                if matches!(self.role, MultiplayerRole::Host { .. }) {
                    self.remote_player_health
                        .entry(id)
                        .or_insert_with(PlayerState::new);
                    self.remote_player_effects.entry(id).or_default();
                    self.network.notify_player_join(id, username);
                    self.send_time_sync_to(id);
                    self.network.send_world_rules_to(self.world_rules, id);
                    self.schedule_player_catchup(id);
                }
            }
            NetworkInbound::PlayerLeave(id) => {
                self.pending_player_catchups.remove(&id);
                self.remote_player_health.remove(&id);
                self.remote_player_effects.remove(&id);
                self.container_sessions.close_by_player(id);
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
            NetworkInbound::PlayerPosition {
                id,
                sequence,
                sender_time_millis,
                x,
                y,
                z,
                yaw,
                pitch,
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

                let mut canonical_snapshot = None;
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
                    if result != SnapshotPushResult::Rejected {
                        canonical_snapshot = remote.snapshots.back().copied();
                    }
                }
                if matches!(self.role, MultiplayerRole::Host { .. }) {
                    if let Some(snapshot) = canonical_snapshot {
                        self.network.broadcast_player_position(
                            id,
                            snapshot.sequence,
                            snapshot.sender_time_millis,
                            snapshot.position,
                            snapshot.yaw,
                            snapshot.pitch,
                        );
                    }
                }
            }
            NetworkInbound::PlayerAction { id, action } => {
                if let Some(remote) = self.remote_players.get(&id) {
                    if let Some(entity) = self.entity_manager.get_by_id_mut(remote.entity_id) {
                        entity.action_cooldown = match action {
                            crate::network::protocol::Action::Place
                            | crate::network::protocol::Action::Break
                            | crate::network::protocol::Action::Use => 0.25,
                        };
                    }
                }
                if matches!(self.role, MultiplayerRole::Host { .. }) {
                    if let NetworkHandle::Host { host_to_server, .. } = &self.network {
                        let _ = host_to_server.tracked_send(
                            crate::network::server::HostToServer::BroadcastPlayerAction {
                                id,
                                action,
                            },
                        );
                    }
                }
            }
            NetworkInbound::ClientBlockChange {
                id,
                x,
                y,
                z,
                block,
                state,
            } => {
                if self.has_in_process_runtime() {
                    let _ = self.submit_remote_authority_block_use(id, x, y, z, block);
                } else {
                    #[cfg(any(test, feature = "legacy_owner"))]
                    self.set_block_and_broadcast(id, x, y, z, block, state);
                    #[cfg(not(any(test, feature = "legacy_owner")))]
                    {
                        let _ = (id, x, y, z, block, state);
                    }
                }
            }
            NetworkInbound::ClientBlockAction {
                id,
                action,
                x,
                y,
                z,
                block,
                held_item,
            } => {
                if self.has_in_process_runtime() {
                    self.handle_authority_client_block_action(id, action, x, y, z, block);
                } else {
                    self.handle_client_block_action(id, action, x, y, z, block, held_item);
                }
            }
            NetworkInbound::BlockActionResult {
                x,
                y,
                z,
                success,
                consumed_item,
                drops,
            } => {
                if success {
                    if consumed_item {
                        self.inventory
                            .use_selected_item(self.game_mode == GameMode::Creative);
                    }
                    for drop_wire in drops {
                        if let Some(stack) = drop_wire.to_stack() {
                            if let Some(leftover) = self.inventory.add_stack(stack) {
                                let sound_pos =
                                    glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                                self.spawn_dropped_item(leftover.item, sound_pos);
                            }
                        }
                    }
                    let mined_block = self.chunk_manager.get_block(x, y, z);
                    self.trigger_advancement(crate::advancements::AdvancementTrigger::MineBlock(
                        mined_block,
                    ));
                    self.damage_selected_tool(
                        (x as u32) ^ (y as u32).rotate_left(11) ^ (z as u32).rotate_left(22),
                    );
                }
            }
            NetworkInbound::AuthoritativeBlockChange {
                dimension,
                revision,
                x,
                y,
                z,
                block,
                state,
                raw_fluid,
            } => {
                self.apply_remote_block_change(
                    dimension, revision, x, y, z, block, state, raw_fluid,
                );
            }
            NetworkInbound::BlockEntityDelta {
                dimension,
                revision,
                x,
                y,
                z,
                entity,
            } => {
                self.apply_remote_block_entity_delta(dimension, revision, x, y, z, entity);
            }
            NetworkInbound::ChunkData {
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
            NetworkInbound::EntitySpawn {
                dimension,
                sequence,
                state,
            }
            | NetworkInbound::EntityState {
                dimension,
                sequence,
                state,
            } => {
                self.apply_replicated_entity_state(dimension, sequence, state);
            }
            NetworkInbound::EntityDespawn {
                dimension,
                sequence,
                entity_id,
            } => {
                self.apply_replicated_entity_despawn(dimension, sequence, entity_id);
            }
            NetworkInbound::PlayerHealth {
                sequence,
                player_id,
                health,
                max_health,
                hunger,
                saturation,
                oxygen,
                is_dead,
                death_reason,
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
            NetworkInbound::PlayerEffect {
                sequence,
                player_id,
                effects,
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
            NetworkInbound::PlayerSessionUpdate {
                sequence,
                player_id,
                dimension,
                state,
            } => {
                if self.local_player_id != Some(player_id)
                    || (self.presentation_topology().is_legacy_owner())
                {
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
            NetworkInbound::TimeSync {
                ticks,
                weather,
                weather_remaining_ticks,
            } => {
                if self.presentation_topology().is_join_client() {
                    self.world_time.ticks = ticks;
                    self.world_time.tick_accumulator = 0.0;
                    if let Some(current) = crate::weather::Weather::from_wire(weather) {
                        self.weather
                            .apply_snapshot(crate::weather::WeatherSnapshot {
                                current,
                                remaining_ticks: weather_remaining_ticks,
                            });
                    }
                }
            }
            NetworkInbound::WorldRulesSync { rules } => {
                if self.presentation_topology().is_join_client() {
                    self.set_world_rules(rules);
                }
            }
            NetworkInbound::LightningStrike(strike) => {
                if self.presentation_topology().is_join_client()
                    && self.current_dimension == crate::dimension::Dimension::Overworld
                {
                    self.apply_lightning_strike(strike);
                }
            }
            NetworkInbound::ChatFromClient { id, message } => {
                let sender = self
                    .remote_players
                    .get(&id)
                    .map(|remote| remote.username.clone())
                    .filter(|username| !username.is_empty())
                    .unwrap_or_else(|| format!("Player {id}"));
                let Some(message) = normalized_chat_message(&message) else {
                    return;
                };
                push_chat_history(&mut self.chat_messages, sender.clone(), message.clone());
                self.network.send_chat(sender, message);
            }
            NetworkInbound::Chat { sender, message } => {
                let Some(message) = normalized_chat_message(&message) else {
                    return;
                };
                push_chat_history(&mut self.chat_messages, sender, message);
            }
            NetworkInbound::CatchupAccepted {
                id,
                dimension,
                cx,
                cz,
                revision,
            } => {
                let key = crate::dimension::Dimension::from_wire(dimension).map(|dimension| {
                    crate::save::NetworkSnapshotKey {
                        player_id: id,
                        dimension,
                        cx,
                        cz,
                        revision,
                    }
                });
                if let Some(key) = key {
                    if let Some(entry) = self
                        .pending_player_catchups
                        .get_mut(&id)
                        .and_then(|entries| entries.iter_mut().find(|entry| entry.key == key))
                    {
                        entry.status = CatchupStatus::AwaitingAck {
                            since: Instant::now(),
                        };
                    }
                }
            }
            NetworkInbound::CatchupBackpressured {
                id,
                dimension,
                cx,
                cz,
                revision,
                mailbox_full_count,
            } => {
                self.perf_counters.network_catchup_mailbox_full = self
                    .perf_counters
                    .network_catchup_mailbox_full
                    .max(mailbox_full_count);
                let key = crate::dimension::Dimension::from_wire(dimension).map(|dimension| {
                    crate::save::NetworkSnapshotKey {
                        player_id: id,
                        dimension,
                        cx,
                        cz,
                        revision,
                    }
                });
                if let Some(key) = key {
                    if let Some(entry) = self
                        .pending_player_catchups
                        .get_mut(&id)
                        .and_then(|entries| entries.iter_mut().find(|entry| entry.key == key))
                    {
                        entry.retries = entry.retries.saturating_add(1);
                        entry.status = CatchupStatus::Pending;
                    }
                }
            }
            NetworkInbound::CatchupAck {
                id,
                dimension,
                cx,
                cz,
                revision,
            } => {
                if let Some(dimension) = crate::dimension::Dimension::from_wire(dimension) {
                    if let Some(entries) = self.pending_player_catchups.get_mut(&id) {
                        entries.retain(|entry| {
                            entry.key
                                != (crate::save::NetworkSnapshotKey {
                                    player_id: id,
                                    dimension,
                                    cx,
                                    cz,
                                    revision,
                                })
                        });
                    }
                    self.pending_player_catchups
                        .retain(|_, entries| !entries.is_empty());
                }
            }
            NetworkInbound::ContainerOpenRequest {
                id,
                dimension,
                x,
                y,
                z,
            } => {
                if matches!(self.role, MultiplayerRole::Host { .. }) {
                    let block = self.chunk_manager.get_block(x, y, z);
                    let mut valid = dimension == self.current_dimension as u8
                        && matches!(
                            block,
                            BlockType::Chest
                                | BlockType::EndCityChest
                                | BlockType::Furnace
                                | BlockType::FurnaceLit
                                | BlockType::Hopper
                                | BlockType::Dispenser
                                | BlockType::Dropper
                        )
                        && self.chunk_manager.get_block_entity(x, y, z).is_some();
                    if valid && matches!(block, BlockType::Chest | BlockType::EndCityChest) {
                        if self
                            .chunk_manager
                            .get_block(x, y + 1, z)
                            .properties()
                            .is_solid
                        {
                            valid = false;
                        }
                        if let Some(partner_pos) =
                            crate::container_sessions::ContainerSessionManager::get_double_chest_partner(
                                &self.chunk_manager,
                                x,
                                y,
                                z,
                            )
                        {
                            if self
                                .chunk_manager
                                .get_block(partner_pos.0, partner_pos.1 + 1, partner_pos.2)
                                .properties()
                                .is_solid
                            {
                                valid = false;
                            }
                        }
                    }
                    if valid {
                        if let Some(remote) = self.remote_players.get(&id) {
                            if let Some(snap) = remote.snapshots.back() {
                                let chest_center = glam::Vec3::new(
                                    x as f32 + 0.5,
                                    y as f32 + 0.5,
                                    z as f32 + 0.5,
                                );
                                if (snap.position - chest_center).length_squared() > 64.0 {
                                    valid = false;
                                }
                            }
                        }
                    }

                    if valid {
                        let had_viewer = self.legacy_chest_viewer_count(dimension, (x, y, z)) > 0;
                        let replaced = self.container_sessions.close_by_player(id);
                        for old in replaced {
                            if old.dimension == dimension && old.x == x && old.y == y && old.z == z
                            {
                                continue;
                            }
                            if let NetworkHandle::Host { host_to_server, .. } = &self.network {
                                let _ = host_to_server.tracked_send(
                                    crate::network::server::HostToServer::SendContainerClose {
                                        to: old.player_id,
                                        dimension: old.dimension,
                                        x: old.x,
                                        y: old.y,
                                        z: old.z,
                                    },
                                );
                            }
                            let old_position = (old.x, old.y, old.z);
                            if old.dimension == self.current_dimension as u8
                                && matches!(
                                    self.chunk_manager.get_block(old.x, old.y, old.z),
                                    BlockType::Chest | BlockType::EndCityChest
                                )
                                && self.legacy_chest_viewer_count(old.dimension, old_position) == 0
                            {
                                self.set_local_chest_open_state(old_position, false);
                            }
                        }
                        if matches!(block, BlockType::Chest | BlockType::EndCityChest) {
                            crate::container_sessions::ContainerSessionManager::ensure_chest_loot_generated(
                                &mut self.chunk_manager,
                                x,
                                y,
                                z,
                                self.world_seed,
                            );
                        }
                        self.container_sessions.open(id, dimension, x, y, z);
                        if matches!(block, BlockType::Chest | BlockType::EndCityChest)
                            && !had_viewer
                        {
                            self.set_local_chest_open_state((x, y, z), true);
                        }
                        if let Some(session) = self.container_sessions.find_by_player_mut(id) {
                            session.revision = self
                                .chunk_manager
                                .get_block_entity(x, y, z)
                                .map(crate::block_entity::BlockEntity::revision)
                                .unwrap_or(0);
                        }
                        if let Some(slots_vec) =
                            crate::container_sessions::ContainerSessionManager::get_container_slots(
                                &self.chunk_manager,
                                x,
                                y,
                                z,
                            )
                        {
                            let slots: Vec<Option<crate::network::protocol::ItemWire>> = slots_vec
                                .iter()
                                .map(|s| {
                                    s.as_ref()
                                        .map(crate::network::protocol::ItemWire::from_stack)
                                })
                                .collect();
                            let slot_count = slots_vec.len();
                            self.container_is_double = slot_count > 27;
                            let revision = self
                                .chunk_manager
                                .get_block_entity(x, y, z)
                                .map(crate::block_entity::BlockEntity::revision)
                                .unwrap_or(0);
                            if let NetworkHandle::Host { host_to_server, .. } = &self.network {
                                let _ = host_to_server.tracked_send(
                                    crate::network::server::HostToServer::SendContainerOpenResult {
                                        to: id,
                                        dimension,
                                        success: true,
                                        x,
                                        y,
                                        z,
                                        slots,
                                        revision,
                                    },
                                );
                            }
                        }
                        self.container_target = Some((x, y, z));
                    } else {
                        if let NetworkHandle::Host { host_to_server, .. } = &self.network {
                            let _ = host_to_server.tracked_send(
                                crate::network::server::HostToServer::SendContainerOpenResult {
                                    to: id,
                                    dimension,
                                    success: false,
                                    x,
                                    y,
                                    z,
                                    slots: vec![],
                                    revision: 0,
                                },
                            );
                        }
                    }
                }
            }
            NetworkInbound::ContainerClickRequest {
                id,
                dimension,
                revision,
                slot_index,
                is_left,
                dragged,
            } => {
                // Restoring NetworkHandle::Host requires deleting this path or
                // wiring it to ServerRuntime. Container clicks are authority
                // transactions; an in-process runtime already owns them.
                if self.has_in_process_runtime() {
                    return;
                }
                if matches!(self.role, MultiplayerRole::Host { .. }) {
                    if let Some(session) = self.container_sessions.find_by_player(id) {
                        let session = session.clone();
                        let mut valid = session.dimension == self.current_dimension as u8
                            && dimension == self.current_dimension as u8
                            && revision == session.revision;
                        if let Some(remote) = self.remote_players.get(&id) {
                            if let Some(snap) = remote.snapshots.back() {
                                let chest_center = glam::Vec3::new(
                                    session.x as f32 + 0.5,
                                    session.y as f32 + 0.5,
                                    session.z as f32 + 0.5,
                                );
                                if (snap.position - chest_center).length_squared() > 64.0 {
                                    valid = false;
                                }
                            }
                        }
                        if valid {
                            if let Some(mut slots_vec) =
                                crate::container_sessions::ContainerSessionManager::get_container_slots(
                                    &self.chunk_manager,
                                    session.x,
                                    session.y,
                                    session.z,
                                )
                            {
                                if (slot_index as usize) < slots_vec.len() {
                                    let slot_item = slots_vec[slot_index as usize];
                                    let dragged_stack = dragged.and_then(|w| w.to_stack());
                                    let Some(entity) = self
                                        .chunk_manager
                                        .get_block_entity(session.x, session.y, session.z)
                                    else {
                                        return;
                                    };
                                    let Some(access) =
                                        crate::block_entity::ContainerAccess::for_entity(entity)
                                    else {
                                        return;
                                    };
                                    // A double chest is exposed as one 54-slot
                                    // view, while each half's capability still
                                    // owns 27 physical slots.
                                    let capability_slot = if access.kind
                                        == crate::block_entity::ContainerKind::Chest
                                    {
                                        slot_index as usize % 27
                                    } else {
                                        slot_index as usize
                                    };
                                    if dragged_stack
                                        .as_ref()
                                        .is_some_and(|stack| {
                                            !access.can_insert(capability_slot, stack, None)
                                        })
                                        || (dragged_stack.is_none()
                                            && slot_item.is_some()
                                            && !access.can_extract(capability_slot, None))
                                    {
                                        return;
                                    }
                                    let (new_slot, new_dragged) =
                                        crate::container_sessions::simulate_container_click(
                                            slot_item,
                                            dragged_stack,
                                            is_left,
                                        );
                                    slots_vec[slot_index as usize] = new_slot;
                                    let slot_wire = new_slot
                                        .as_ref()
                                        .map(crate::network::protocol::ItemWire::from_stack);
                                    let committed =
                                        crate::container_sessions::ContainerSessionManager::set_container_slots(
                                            &mut self.chunk_manager,
                                            session.x,
                                            session.y,
                                            session.z,
                                            &slots_vec,
                                        );
                                    if !committed {
                                        return;
                                    }
                                    let dragged_wire = new_dragged
                                        .as_ref()
                                        .map(crate::network::protocol::ItemWire::from_stack);
                                    let current_revision = self
                                        .chunk_manager
                                        .get_block_entity(session.x, session.y, session.z)
                                        .map(crate::block_entity::BlockEntity::revision)
                                        .unwrap_or(session.revision);
                                    if let Some(active) =
                                        self.container_sessions.find_by_player_mut(id)
                                    {
                                        active.revision = current_revision;
                                    }
                                    self.redstone.mark_container_changed(
                                        &self.chunk_manager,
                                        (session.x, session.y, session.z),
                                    );
                                    if let NetworkHandle::Host { host_to_server, .. } =
                                        &self.network
                                    {
                                        let _ = host_to_server.tracked_send(
                                            crate::network::server::HostToServer::SendContainerClickResult {
                                                to: id,
                                                dimension: session.dimension,
                                                success: true,
                                                slot_index,
                                                slot: slot_wire,
                                                dragged: dragged_wire,
                                            },
                                        );
                                        let _ = host_to_server.tracked_send(
                                            crate::network::server::HostToServer::BroadcastContainerSlotUpdate {
                                                dimension: session.dimension,
                                                revision: current_revision,
                                                x: session.x,
                                                y: session.y,
                                                z: session.z,
                                                slot_index,
                                                slot: slot_wire,
                                            },
                                        );
                                    }
                                }
                            }
                        } else {
                            self.container_sessions.close_by_player(id);
                        }
                    }
                }
            }
            NetworkInbound::ContainerClose {
                id,
                dimension,
                x,
                y,
                z,
            } => {
                if matches!(self.role, MultiplayerRole::Host { .. }) {
                    let closed = self.container_sessions.close_exact(id, dimension, x, y, z);
                    if let Some(session) = closed {
                        let block = self
                            .chunk_manager
                            .get_block(session.x, session.y, session.z);
                        if session.dimension == self.current_dimension as u8
                            && matches!(block, BlockType::Chest | BlockType::EndCityChest)
                            && self.legacy_chest_viewer_count(
                                session.dimension,
                                (session.x, session.y, session.z),
                            ) == 0
                            && self.container_target != Some((session.x, session.y, session.z))
                        {
                            self.set_local_chest_open_state(
                                (session.x, session.y, session.z),
                                false,
                            );
                        }
                    }
                }
                if self.local_player_id == Some(id)
                    && dimension == self.current_dimension as u8
                    && self.container_target == Some((x, y, z))
                {
                    self.force_close_inventory();
                }
            }
            NetworkInbound::ContainerOpenResult {
                dimension,
                success,
                x,
                y,
                z,
                slots,
                revision,
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
                        committed =
                            crate::container_sessions::ContainerSessionManager::set_container_slots(
                                &mut self.chunk_manager,
                                x,
                                y,
                                z,
                                &stacks,
                            );
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
            NetworkInbound::ContainerClickResult {
                dimension,
                success,
                slot_index: _,
                slot: _,
                dragged,
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
            NetworkInbound::ContainerSlotUpdate {
                dimension,
                revision,
                x,
                y,
                z,
                slot_index,
                slot,
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
                if let Some(mut slots) =
                    crate::container_sessions::ContainerSessionManager::get_container_slots(
                        &self.chunk_manager,
                        x,
                        y,
                        z,
                    )
                {
                    if (slot_index as usize) < slots.len() {
                        slots[slot_index as usize] = slot.and_then(|wire| wire.to_stack());
                        if !crate::container_sessions::ContainerSessionManager::set_container_slots(
                            &mut self.chunk_manager,
                            x,
                            y,
                            z,
                            &slots,
                        ) {
                            return;
                        }
                        if let Some(entity) = self.chunk_manager.get_block_entity_mut(x, y, z) {
                            entity.set_revision(revision);
                        }
                    }
                }
            }
            NetworkInbound::ClientRespawnRequest { id } => {
                if matches!(&self.network, NetworkHandle::Host { .. }) {
                    if let Some(remote) = self.remote_players.get_mut(&id) {
                        if remote.is_dead || remote.health <= 0.0 {
                            let mut spawn_pos = None;
                            let mut spawn_dim = crate::dimension::Dimension::Overworld;

                            if let (Some(bed_p), Some(dim)) =
                                (remote.spawn_point, remote.spawn_dimension)
                            {
                                let chunk_pos = (bed_p[0], bed_p[1], bed_p[2]);
                                if self.chunk_manager.get_block(
                                    chunk_pos.0,
                                    chunk_pos.1,
                                    chunk_pos.2,
                                ) == crate::world::BlockType::Bed
                                {
                                    let (safe_p, safe) = crate::world::find_safe_spawn_position(
                                        &self.chunk_manager,
                                        chunk_pos,
                                    );
                                    if safe {
                                        spawn_pos = Some(safe_p);
                                        spawn_dim = dim;
                                    }
                                }
                                if spawn_pos.is_none() {
                                    remote.spawn_point = None;
                                    remote.spawn_dimension = None;
                                }
                            }

                            if spawn_pos.is_none() {
                                spawn_dim = crate::dimension::Dimension::Overworld;
                                let target_p =
                                    (self.world_spawn.0, self.world_spawn.1, self.world_spawn.2);
                                let (safe_p, safe) = crate::world::find_safe_spawn_position(
                                    &self.chunk_manager,
                                    target_p,
                                );
                                if safe {
                                    spawn_pos = Some(safe_p);
                                } else {
                                    spawn_pos = Some(Vec3::new(
                                        target_p.0 as f32 + 0.5,
                                        target_p.1 as f32,
                                        target_p.2 as f32 + 0.5,
                                    ));
                                }
                            }

                            let respawn_p = spawn_pos.unwrap_or_else(|| Vec3::new(8.0, 80.0, 8.0));
                            remote.health = 20.0;
                            remote.hunger = 20.0;
                            remote.is_dead = false;
                            remote.is_sleeping = false;

                            if let Some(entity) =
                                self.entity_manager.get_by_id_mut(remote.entity_id)
                            {
                                entity.position = respawn_p;
                                entity.health = 20.0;
                            }

                            self.container_sessions.close_by_player(id);
                            self.network.send_respawn_result(
                                id,
                                respawn_p.to_array(),
                                spawn_dim as u8,
                            );
                            let mut respawn_state = PlayerState::new();
                            respawn_state.health = 20.0;
                            respawn_state.hunger = 20.0;
                            respawn_state.is_dead = false;
                            self.network.broadcast_player_health(0, id, &respawn_state);
                        }
                    }
                }
            }
            NetworkInbound::ClientSleepRequest {
                id,
                bed_x,
                bed_y,
                bed_z,
            } => {
                if matches!(&self.network, NetworkHandle::Host { .. }) {
                    if let Some(remote) = self.remote_players.get_mut(&id) {
                        let bed_pos =
                            Vec3::new(bed_x as f32 + 0.5, bed_y as f32 + 0.5, bed_z as f32 + 0.5);
                        let ent_pos = self
                            .entity_manager
                            .get_by_id(remote.entity_id)
                            .map(|e| e.position)
                            .unwrap_or(bed_pos);

                        let clicked_block = self.chunk_manager.get_block(bed_x, bed_y, bed_z);
                        if clicked_block == crate::world::BlockType::Bed
                            && ent_pos.distance(bed_pos) <= 8.0
                            && remote.dimension == crate::dimension::Dimension::Overworld
                        {
                            let bstate = crate::world::BlockState::decode(
                                self.chunk_manager.get_block_state(bed_x, bed_y, bed_z),
                            );
                            let head_pos = if bstate.is_top {
                                (bed_x, bed_y, bed_z)
                            } else {
                                (
                                    bed_x + bstate.facing.dx(),
                                    bed_y,
                                    bed_z + bstate.facing.dz(),
                                )
                            };

                            remote.spawn_point = Some([head_pos.0, head_pos.1, head_pos.2]);
                            remote.spawn_dimension = Some(crate::dimension::Dimension::Overworld);

                            let time_of_day = self.world_time.ticks % 24000;
                            let is_night = time_of_day >= 12541 && time_of_day <= 23458;
                            let is_storm = self.weather.is_thundering();

                            if is_night || is_storm {
                                let nearby_hostiles = self
                                    .entity_manager
                                    .query_radius(bed_pos, 8.0)
                                    .any(|e| e.entity_type.is_hostile() && e.health > 0.0);
                                if !nearby_hostiles {
                                    remote.is_sleeping = true;
                                    remote.bed_pos = Some([bed_x, bed_y, bed_z]);
                                    self.network.broadcast_sleep_state_sync(id, true);
                                }
                            }
                        }
                    }
                }
            }
            NetworkInbound::PlayerRespawnResult {
                position,
                dimension,
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
                self.void_damage_timer = 0.0;
                self.sync_cursor_mode();
            }
            NetworkInbound::SleepStateSync {
                player_id,
                is_sleeping,
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
            NetworkInbound::DimensionTransfer {
                dimension,
                position,
            } => {
                if let Some(target) = crate::dimension::Dimension::from_wire(dimension) {
                    self.reset_presented_dimension(target);
                    self.player_physics.position = Vec3::from_array(position);
                    self.camera.position = Vec3::from_array(position) + Vec3::new(0.0, 1.6, 0.0);
                    self.portal_contact_time = 0.0;
                    self.portal_cooldown = 3.0;
                }
            }
        }
    }
}
