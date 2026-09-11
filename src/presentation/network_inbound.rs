//! Desktop network inbound staging and NetworkHandle send/drain.
//! Production handles are `None` (embedded) or `Client` (join). Listen-host
//! TCP is owned by `ServerRuntime`, not a second GPU-thread server.

use crate::presentation::interpolation::sequence_is_newer;
use glam::Vec3;
use std::time::{Duration, Instant};

pub enum NetworkHandle {
    None,
    Client {
        client_to_game: std::sync::mpsc::Receiver<crate::network::client::ClientToGame>,
        game_to_client: std::sync::mpsc::Sender<crate::network::client::GameToClient>,
        thread: Option<std::thread::JoinHandle<()>>,
    },
}

pub(crate) trait TrackedNetworkSender<T, E> {
    fn tracked_send(&self, value: T) -> Result<(), E>;
}

impl<T> TrackedNetworkSender<T, std::sync::mpsc::SendError<T>> for std::sync::mpsc::Sender<T> {
    fn tracked_send(&self, value: T) -> Result<(), std::sync::mpsc::SendError<T>> {
        crate::perf::tracked_send(
            self,
            value,
            std::mem::size_of::<T>() as u64,
            &crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
        )
    }
}

pub(crate) enum NetworkInbound {
    Connected {
        player_id: crate::network::protocol::PlayerId,
        seed: u64,
        gamemode: u8,
    },
    Disconnected(String),
    PlayerJoin {
        id: crate::network::protocol::PlayerId,
        username: String,
    },
    PlayerLeave(crate::network::protocol::PlayerId),
    PlayerPosition {
        id: crate::network::protocol::PlayerId,
        sequence: u32,
        sender_time_millis: u64,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
    },
    PlayerAction {
        id: crate::network::protocol::PlayerId,
        action: crate::network::protocol::Action,
    },
    AuthoritativeBlockChange {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
        raw_fluid: u8,
    },
    BlockEntityDelta {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        entity: Option<crate::block_entity::BlockEntity>,
    },
    ChunkData {
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        min_section_y: i8,
        section_count: u16,
        blocks: Vec<u8>,
        block_states: Vec<u8>,
        fluid_levels: Vec<u8>,
        block_entities: Vec<u8>,
    },
    EntitySpawn {
        dimension: u8,
        sequence: u64,
        state: crate::network::protocol::EntityStateWire,
    },
    EntityState {
        dimension: u8,
        sequence: u64,
        state: crate::network::protocol::EntityStateWire,
    },
    EntityDespawn {
        dimension: u8,
        sequence: u64,
        entity_id: u64,
    },
    PlayerHealth {
        sequence: u64,
        player_id: crate::network::protocol::PlayerId,
        health: f32,
        max_health: f32,
        hunger: f32,
        saturation: f32,
        oxygen: f32,
        is_dead: bool,
        death_reason: u8,
    },
    PlayerEffect {
        sequence: u64,
        player_id: crate::network::protocol::PlayerId,
        effects: Vec<crate::network::protocol::PlayerEffectWire>,
    },
    PlayerSessionUpdate {
        sequence: u64,
        player_id: crate::network::protocol::PlayerId,
        dimension: u8,
        state: crate::network::protocol::SessionGameplayWire,
    },
    TimeSync {
        ticks: u64,
        weather: u8,
        weather_remaining_ticks: f32,
    },
    WorldRulesSync {
        rules: crate::game_rules::WorldRules,
    },
    LightningStrike(crate::network::protocol::LightningStrike),
    Chat {
        sender: String,
        message: String,
    },
    StatusUpdate(String),
    GameplayResponse {
        response: crate::network::protocol::GameplayResponse,
    },
    ContainerClose {
        id: crate::network::protocol::PlayerId,
        dimension: u8,
        x: i32,
        y: i32,
        z: i32,
    },
    ContainerOpenResult {
        dimension: u8,
        success: bool,
        x: i32,
        y: i32,
        z: i32,
        slots: Vec<Option<crate::network::protocol::ItemWire>>,
        revision: u64,
    },
    ContainerClickResult {
        dimension: u8,
        success: bool,
        slot_index: u16,
        slot: Option<crate::network::protocol::ItemWire>,
        dragged: Option<crate::network::protocol::ItemWire>,
    },
    ContainerSlotUpdate {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        slot_index: u16,
        slot: Option<crate::network::protocol::ItemWire>,
    },
    PlayerRespawnResult {
        position: [f32; 3],
        dimension: u8,
    },
    SleepStateSync {
        player_id: crate::network::protocol::PlayerId,
        is_sleeping: bool,
    },
    DimensionTransfer {
        dimension: u8,
        position: [f32; 3],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkDeliveryClass {
    Reliable,
    LatestPosition,
    LatestEntity,
    LatestHealth,
    LatestEffect,
    LatestTimeSync,
}

pub(crate) fn classify_network_event(event: &NetworkInbound) -> NetworkDeliveryClass {
    match event {
        NetworkInbound::PlayerPosition { .. } => NetworkDeliveryClass::LatestPosition,
        NetworkInbound::EntityState { .. } => NetworkDeliveryClass::LatestEntity,
        NetworkInbound::PlayerHealth { .. } => NetworkDeliveryClass::LatestHealth,
        NetworkInbound::PlayerEffect { .. } => NetworkDeliveryClass::LatestEffect,
        NetworkInbound::TimeSync { .. } => NetworkDeliveryClass::LatestTimeSync,
        _ => NetworkDeliveryClass::Reliable,
    }
}

impl NetworkInbound {
    pub(crate) fn estimated_bytes(&self) -> usize {
        let inline = std::mem::size_of_val(self);
        let heap = match self {
            Self::Disconnected(reason) | Self::StatusUpdate(reason) => reason.len(),
            Self::GameplayResponse { response } => std::mem::size_of_val(response),
            Self::PlayerJoin { username, .. } => username.len(),
            Self::ChunkData {
                blocks,
                block_states,
                ..
            } => blocks.len().saturating_add(block_states.len()),
            Self::PlayerEffect { effects, .. } => {
                effects.len() * std::mem::size_of::<crate::network::protocol::PlayerEffectWire>()
            }
            Self::Chat { sender, message } => sender.len().saturating_add(message.len()),
            Self::ContainerClose { .. } => 0,
            Self::ContainerOpenResult { slots, .. } => {
                slots.len() * std::mem::size_of::<Option<crate::network::protocol::ItemWire>>()
            }
            Self::ContainerClickResult { slot, dragged, .. } => {
                slot.as_ref().map_or(0, |w| std::mem::size_of_val(w))
                    + dragged.as_ref().map_or(0, |w| std::mem::size_of_val(w))
            }
            Self::ContainerSlotUpdate { slot, .. } => {
                slot.as_ref().map_or(0, |w| std::mem::size_of_val(w))
            }
            _ => 0,
        };
        inline.saturating_add(heap)
    }
}

#[derive(Default)]
pub(crate) struct NetworkStaging {
    pub(crate) reliable: std::collections::VecDeque<NetworkInbound>,
    pub(crate) latest_positions:
        std::collections::HashMap<crate::network::protocol::PlayerId, NetworkInbound>,
    pub(crate) latest_entities: std::collections::HashMap<(u8, u64), NetworkInbound>,
    pub(crate) latest_health:
        std::collections::HashMap<crate::network::protocol::PlayerId, NetworkInbound>,
    pub(crate) latest_effects:
        std::collections::HashMap<crate::network::protocol::PlayerId, NetworkInbound>,
    pub(crate) latest_time_sync: Option<NetworkInbound>,
}

impl NetworkStaging {
    pub(crate) fn stage(&mut self, event: NetworkInbound) {
        match classify_network_event(&event) {
            NetworkDeliveryClass::Reliable => self.reliable.push_back(event),
            NetworkDeliveryClass::LatestPosition => {
                let NetworkInbound::PlayerPosition { id, sequence, .. } = &event else {
                    unreachable!("position delivery class must contain a position event");
                };
                let replace = self.latest_positions.get(id).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::PlayerPosition {
                            sequence: old_sequence,
                            ..
                        } if sequence_is_newer(*sequence, *old_sequence)
                    )
                });
                if replace {
                    self.latest_positions.insert(*id, event);
                }
            }
            NetworkDeliveryClass::LatestEntity => {
                let NetworkInbound::EntityState {
                    dimension,
                    sequence,
                    state,
                } = &event
                else {
                    unreachable!("entity delivery class must contain an entity-state event");
                };
                let key = (*dimension, state.entity_id);
                let replace = self.latest_entities.get(&key).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::EntityState {
                            sequence: old_sequence,
                            ..
                        } if sequence > old_sequence
                    )
                });
                if replace {
                    self.latest_entities.insert(key, event);
                }
            }
            NetworkDeliveryClass::LatestHealth => {
                let NetworkInbound::PlayerHealth {
                    player_id,
                    sequence,
                    ..
                } = &event
                else {
                    unreachable!("health delivery class must contain a health event");
                };
                let replace = self.latest_health.get(player_id).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::PlayerHealth {
                            sequence: old_sequence,
                            ..
                        } if sequence > old_sequence
                    )
                });
                if replace {
                    self.latest_health.insert(*player_id, event);
                }
            }
            NetworkDeliveryClass::LatestEffect => {
                let NetworkInbound::PlayerEffect {
                    player_id,
                    sequence,
                    ..
                } = &event
                else {
                    unreachable!("effect delivery class must contain an effect event");
                };
                let replace = self.latest_effects.get(player_id).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::PlayerEffect {
                            sequence: old_sequence,
                            ..
                        } if sequence > old_sequence
                    )
                });
                if replace {
                    self.latest_effects.insert(*player_id, event);
                }
            }
            NetworkDeliveryClass::LatestTimeSync => {
                let NetworkInbound::TimeSync { ticks, .. } = &event else {
                    unreachable!("time-sync delivery class must contain a time-sync event");
                };
                let replace = self.latest_time_sync.as_ref().map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::TimeSync {
                            ticks: old_ticks,
                            ..
                        } if ticks > old_ticks
                    )
                });
                if replace {
                    self.latest_time_sync = Some(event);
                }
            }
        }
    }

    pub(crate) fn take_smallest_if_fits<K>(
        map: &mut std::collections::HashMap<K, NetworkInbound>,
        remaining_bytes: usize,
    ) -> Option<(NetworkInbound, usize)>
    where
        K: Copy + Ord + std::hash::Hash + Eq,
    {
        let key = map.keys().min().copied()?;
        let event_bytes = map.get(&key)?.estimated_bytes();
        if event_bytes > remaining_bytes {
            return None;
        }
        map.remove(&key).map(|event| (event, event_bytes))
    }

    /// Remove one event only when its full estimated footprint fits. Reliable
    /// events are considered first and never skipped, preserving strict FIFO.
    pub(crate) fn pop_next_if_fits(
        &mut self,
        remaining_bytes: usize,
    ) -> Option<(NetworkInbound, usize)> {
        if let Some(event) = self.reliable.front() {
            let event_bytes = event.estimated_bytes();
            if event_bytes > remaining_bytes {
                return None;
            }
            return self.reliable.pop_front().map(|event| (event, event_bytes));
        }

        if !self.latest_positions.is_empty() {
            return Self::take_smallest_if_fits(&mut self.latest_positions, remaining_bytes);
        }
        if !self.latest_entities.is_empty() {
            return Self::take_smallest_if_fits(&mut self.latest_entities, remaining_bytes);
        }
        if !self.latest_health.is_empty() {
            return Self::take_smallest_if_fits(&mut self.latest_health, remaining_bytes);
        }
        if !self.latest_effects.is_empty() {
            return Self::take_smallest_if_fits(&mut self.latest_effects, remaining_bytes);
        }
        let event_bytes = self.latest_time_sync.as_ref()?.estimated_bytes();
        if event_bytes > remaining_bytes {
            return None;
        }
        self.latest_time_sync
            .take()
            .map(|event| (event, event_bytes))
    }

    pub(crate) fn reliable_len(&self) -> usize {
        self.reliable.len()
    }

    pub(crate) fn latest_len(&self) -> usize {
        self.latest_positions.len()
            + self.latest_entities.len()
            + self.latest_health.len()
            + self.latest_effects.len()
            + usize::from(self.latest_time_sync.is_some())
    }

    pub(crate) fn len(&self) -> usize {
        self.reliable_len() + self.latest_len()
    }

    pub(crate) fn reliable_bytes(&self) -> u64 {
        self.reliable
            .iter()
            .map(|event| event.estimated_bytes() as u64)
            .sum()
    }

    pub(crate) fn latest_bytes(&self) -> u64 {
        self.latest_positions
            .values()
            .chain(self.latest_entities.values())
            .chain(self.latest_health.values())
            .chain(self.latest_effects.values())
            .chain(self.latest_time_sync.iter())
            .map(|event| event.estimated_bytes() as u64)
            .sum()
    }
}

impl NetworkHandle {
    pub(crate) fn drain_inbound(&self) -> Vec<NetworkInbound> {
        const MAX_EVENTS: usize = 256;
        let started = Instant::now();
        match self {
            NetworkHandle::None => Vec::new(),
            NetworkHandle::Client { client_to_game, .. } => {
                let mut raw = Vec::with_capacity(MAX_EVENTS);
                while raw.len() < MAX_EVENTS && started.elapsed() < Duration::from_millis(2) {
                    match crate::perf::tracked_try_recv(
                        client_to_game,
                        std::mem::size_of::<crate::network::client::ClientToGame>() as u64,
                        &crate::perf::queue_stats(crate::perf::QueueCategory::Inbound),
                    ) {
                        Ok(event) => raw.push(event),
                        Err(_) => break,
                    }
                }
                raw.into_iter()
                    .map(|event| match event {
                        crate::network::client::ClientToGame::Connected {
                            player_id,
                            seed,
                            gamemode,
                        } => NetworkInbound::Connected {
                            player_id,
                            seed,
                            gamemode,
                        },
                        crate::network::client::ClientToGame::Disconnected { reason } => {
                            NetworkInbound::Disconnected(reason)
                        }
                        crate::network::client::ClientToGame::PlayerJoin { id, username } => {
                            NetworkInbound::PlayerJoin { id, username }
                        }
                        crate::network::client::ClientToGame::PlayerLeave { id } => {
                            NetworkInbound::PlayerLeave(id)
                        }
                        crate::network::client::ClientToGame::PlayerPosition {
                            id,
                            sequence,
                            sender_time_millis,
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        } => NetworkInbound::PlayerPosition {
                            id,
                            sequence,
                            sender_time_millis,
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        },
                        crate::network::client::ClientToGame::PlayerAction { id, action } => {
                            NetworkInbound::PlayerAction { id, action }
                        }
                        crate::network::client::ClientToGame::BlockChange {
                            dimension,
                            revision,
                            x,
                            y,
                            z,
                            block,
                            state,
                            raw_fluid,
                        } => NetworkInbound::AuthoritativeBlockChange {
                            dimension,
                            revision,
                            x,
                            y,
                            z,
                            block,
                            state,
                            raw_fluid,
                        },
                        crate::network::client::ClientToGame::BlockEntityDelta {
                            dimension,
                            revision,
                            x,
                            y,
                            z,
                            entity,
                        } => NetworkInbound::BlockEntityDelta {
                            dimension,
                            revision,
                            x,
                            y,
                            z,
                            entity,
                        },
                        crate::network::client::ClientToGame::ChunkData {
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
                        } => NetworkInbound::ChunkData {
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
                        },
                        crate::network::client::ClientToGame::EntitySpawn {
                            dimension,
                            sequence,
                            state,
                        } => NetworkInbound::EntitySpawn {
                            dimension,
                            sequence,
                            state,
                        },
                        crate::network::client::ClientToGame::EntityState {
                            dimension,
                            sequence,
                            state,
                        } => NetworkInbound::EntityState {
                            dimension,
                            sequence,
                            state,
                        },
                        crate::network::client::ClientToGame::EntityDespawn {
                            dimension,
                            sequence,
                            entity_id,
                        } => NetworkInbound::EntityDespawn {
                            dimension,
                            sequence,
                            entity_id,
                        },
                        crate::network::client::ClientToGame::PlayerHealth {
                            sequence,
                            player_id,
                            health,
                            max_health,
                            hunger,
                            saturation,
                            oxygen,
                            is_dead,
                            death_reason,
                        } => NetworkInbound::PlayerHealth {
                            sequence,
                            player_id,
                            health,
                            max_health,
                            hunger,
                            saturation,
                            oxygen,
                            is_dead,
                            death_reason,
                        },
                        crate::network::client::ClientToGame::PlayerEffect {
                            sequence,
                            player_id,
                            effects,
                        } => NetworkInbound::PlayerEffect {
                            sequence,
                            player_id,
                            effects,
                        },
                        crate::network::client::ClientToGame::PlayerSessionUpdate {
                            sequence,
                            player_id,
                            dimension,
                            state,
                        } => NetworkInbound::PlayerSessionUpdate {
                            sequence,
                            player_id,
                            dimension,
                            state,
                        },
                        crate::network::client::ClientToGame::TimeSync {
                            ticks,
                            weather,
                            weather_remaining_ticks,
                        } => NetworkInbound::TimeSync {
                            ticks,
                            weather,
                            weather_remaining_ticks,
                        },
                        crate::network::client::ClientToGame::WorldRulesSync { rules } => {
                            NetworkInbound::WorldRulesSync { rules }
                        }
                        crate::network::client::ClientToGame::GameplayResponse { response } => {
                            NetworkInbound::GameplayResponse { response }
                        }
                        crate::network::client::ClientToGame::LightningStrike(strike) => {
                            NetworkInbound::LightningStrike(strike)
                        }
                        crate::network::client::ClientToGame::Chat { sender, message } => {
                            NetworkInbound::Chat { sender, message }
                        }
                        crate::network::client::ClientToGame::StatusUpdate { message } => {
                            NetworkInbound::StatusUpdate(message)
                        }
                        crate::network::client::ClientToGame::ContainerOpenResult {
                            dimension,
                            success,
                            x,
                            y,
                            z,
                            slots,
                            revision,
                        } => NetworkInbound::ContainerOpenResult {
                            dimension,
                            success,
                            x,
                            y,
                            z,
                            slots,
                            revision,
                        },
                        crate::network::client::ClientToGame::ContainerClose {
                            id,
                            dimension,
                            x,
                            y,
                            z,
                        } => NetworkInbound::ContainerClose {
                            id,
                            dimension,
                            x,
                            y,
                            z,
                        },
                        crate::network::client::ClientToGame::ContainerClickResult {
                            dimension,
                            success,
                            slot_index,
                            slot,
                            dragged,
                        } => NetworkInbound::ContainerClickResult {
                            dimension,
                            success,
                            slot_index,
                            slot,
                            dragged,
                        },
                        crate::network::client::ClientToGame::ContainerSlotUpdate {
                            dimension,
                            revision,
                            x,
                            y,
                            z,
                            slot_index,
                            slot,
                        } => NetworkInbound::ContainerSlotUpdate {
                            dimension,
                            revision,
                            x,
                            y,
                            z,
                            slot_index,
                            slot,
                        },
                        crate::network::client::ClientToGame::PlayerRespawnResult {
                            position,
                            dimension,
                        } => NetworkInbound::PlayerRespawnResult {
                            position,
                            dimension,
                        },
                        crate::network::client::ClientToGame::SleepStateSync {
                            player_id,
                            is_sleeping,
                        } => NetworkInbound::SleepStateSync {
                            player_id,
                            is_sleeping,
                        },
                        crate::network::client::ClientToGame::DimensionTransfer {
                            dimension,
                            position,
                        } => NetworkInbound::DimensionTransfer {
                            dimension,
                            position,
                        },
                    })
                    .collect()
            }
        }
    }

    pub(crate) fn send_position(
        &self,
        sequence: u32,
        sender_time_millis: u64,
        position: Vec3,
        yaw: f32,
        pitch: f32,
    ) {
        match self {
            NetworkHandle::Client { game_to_client, .. } => {
                let _ = crate::perf::tracked_send(
                    game_to_client,
                    crate::network::client::GameToClient::SendPosition {
                        sequence,
                        sender_time_millis,
                        x: position.x,
                        y: position.y,
                        z: position.z,
                        yaw,
                        pitch,
                    },
                    std::mem::size_of::<crate::network::client::GameToClient>() as u64,
                    &crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
                );
            }
            NetworkHandle::None => {}
        }
    }

    /// Publish one already-typed gameplay operation to a joining client.
    /// Embedded hosts use `State::submit_authority_request` so their local
    /// producer shares the runtime FIFO; this method is deliberately a
    /// socket-only egress seam and never mutates a presentation cache.
    pub(crate) fn request_gameplay(&self, request: crate::network::protocol::GameplayRequest) {
        if let NetworkHandle::Client { game_to_client, .. } = self {
            let _ = crate::perf::tracked_send(
                game_to_client,
                crate::network::client::GameToClient::GameplayRequest { request },
                std::mem::size_of::<crate::network::client::GameToClient>() as u64,
                &crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
            );
        }
    }

    pub(crate) fn send_respawn_request(&self) {
        if let NetworkHandle::Client { game_to_client, .. } = self {
            let _ = game_to_client
                .tracked_send(crate::network::client::GameToClient::PlayerRespawnRequest);
        }
    }

    pub(crate) fn send_action(&self, action: crate::network::protocol::Action) {
        match self {
            NetworkHandle::Client { game_to_client, .. } => {
                let _ = game_to_client
                    .tracked_send(crate::network::client::GameToClient::SendAction { action });
            }
            NetworkHandle::None => {}
        }
    }

    pub(crate) fn send_chat(&self, _sender: String, message: String) {
        match self {
            NetworkHandle::Client { game_to_client, .. } => {
                let _ = game_to_client
                    .tracked_send(crate::network::client::GameToClient::SendChat { message });
            }
            NetworkHandle::None => {}
        }
    }

    pub(crate) fn shutdown(&mut self) {
        let thread = match self {
            NetworkHandle::None => None,
            NetworkHandle::Client {
                game_to_client,
                thread,
                ..
            } => {
                let _ =
                    game_to_client.tracked_send(crate::network::client::GameToClient::Disconnect);
                thread.take()
            }
        };
        if let Some(thread) = thread {
            let _ = thread.join();
        }
        crate::perf::reset_network_queue_stats();
    }
}
