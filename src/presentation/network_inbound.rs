//! Desktop network inbound staging and NetworkHandle send/drain.
//! Production handles are `None` (embedded) or `Client` (join). Listen-host
//! TCP is owned by `ServerRuntime`, not a second GPU-thread server.
//!
//! Join and embedded both deliver wire `Packet` into staging / handlers.
//! `ClientToGame` is only a thin local wrapper (`StatusUpdate` | `Packet`);
//! there is no second field-mirrored presentation enum.

use crate::network::client::ClientToGame;
use crate::network::protocol::{Packet, PlayerId};
use crate::presentation::interpolation::sequence_is_newer;
use glam::Vec3;
use std::time::{Duration, Instant};

pub enum NetworkHandle {
    None,
    Client {
        client_to_game: std::sync::mpsc::Receiver<ClientToGame>,
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

/// Presentation inbound = join-client `ClientToGame` (StatusUpdate | Packet).
pub(crate) type NetworkInbound = ClientToGame;

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
        NetworkInbound::Packet(Packet::PlayerPosition { .. }) => {
            NetworkDeliveryClass::LatestPosition
        }
        NetworkInbound::Packet(Packet::EntityState { .. }) => NetworkDeliveryClass::LatestEntity,
        NetworkInbound::Packet(Packet::PlayerHealth { .. }) => NetworkDeliveryClass::LatestHealth,
        NetworkInbound::Packet(Packet::PlayerEffect { .. }) => NetworkDeliveryClass::LatestEffect,
        NetworkInbound::Packet(Packet::TimeSync { .. }) => NetworkDeliveryClass::LatestTimeSync,
        _ => NetworkDeliveryClass::Reliable,
    }
}

pub(crate) fn estimated_inbound_bytes(event: &NetworkInbound) -> usize {
    event.estimated_bytes()
}

#[derive(Default)]
pub(crate) struct NetworkStaging {
    pub(crate) reliable: std::collections::VecDeque<NetworkInbound>,
    pub(crate) latest_positions: std::collections::HashMap<PlayerId, NetworkInbound>,
    pub(crate) latest_entities: std::collections::HashMap<(u8, u64), NetworkInbound>,
    pub(crate) latest_health: std::collections::HashMap<PlayerId, NetworkInbound>,
    pub(crate) latest_effects: std::collections::HashMap<PlayerId, NetworkInbound>,
    pub(crate) latest_time_sync: Option<NetworkInbound>,
}

impl NetworkStaging {
    pub(crate) fn stage(&mut self, event: NetworkInbound) {
        match classify_network_event(&event) {
            NetworkDeliveryClass::Reliable => self.reliable.push_back(event),
            NetworkDeliveryClass::LatestPosition => {
                let NetworkInbound::Packet(Packet::PlayerPosition { id, sequence, .. }) = &event
                else {
                    unreachable!("position delivery class must contain a position packet");
                };
                let replace = self.latest_positions.get(id).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::Packet(Packet::PlayerPosition {
                            sequence: old_sequence,
                            ..
                        }) if sequence_is_newer(*sequence, *old_sequence)
                    )
                });
                if replace {
                    self.latest_positions.insert(*id, event);
                }
            }
            NetworkDeliveryClass::LatestEntity => {
                let NetworkInbound::Packet(Packet::EntityState {
                    dimension,
                    sequence,
                    state,
                    ..
                }) = &event
                else {
                    unreachable!("entity delivery class must contain an entity-state packet");
                };
                let key = (*dimension, state.entity_id);
                let replace = self.latest_entities.get(&key).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::Packet(Packet::EntityState {
                            sequence: old_sequence,
                            ..
                        }) if sequence > old_sequence
                    )
                });
                if replace {
                    self.latest_entities.insert(key, event);
                }
            }
            NetworkDeliveryClass::LatestHealth => {
                let NetworkInbound::Packet(Packet::PlayerHealth {
                    player_id,
                    sequence,
                    ..
                }) = &event
                else {
                    unreachable!("health delivery class must contain a health packet");
                };
                let replace = self.latest_health.get(player_id).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::Packet(Packet::PlayerHealth {
                            sequence: old_sequence,
                            ..
                        }) if sequence > old_sequence
                    )
                });
                if replace {
                    self.latest_health.insert(*player_id, event);
                }
            }
            NetworkDeliveryClass::LatestEffect => {
                let NetworkInbound::Packet(Packet::PlayerEffect {
                    player_id,
                    sequence,
                    ..
                }) = &event
                else {
                    unreachable!("effect delivery class must contain an effect packet");
                };
                let replace = self.latest_effects.get(player_id).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::Packet(Packet::PlayerEffect {
                            sequence: old_sequence,
                            ..
                        }) if sequence > old_sequence
                    )
                });
                if replace {
                    self.latest_effects.insert(*player_id, event);
                }
            }
            NetworkDeliveryClass::LatestTimeSync => {
                let NetworkInbound::Packet(Packet::TimeSync { ticks, .. }) = &event else {
                    unreachable!("time-sync delivery class must contain a time-sync packet");
                };
                let replace = self.latest_time_sync.as_ref().map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::Packet(Packet::TimeSync {
                            ticks: old_ticks,
                            ..
                        }) if ticks > old_ticks
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
        let event_bytes = estimated_inbound_bytes(map.get(&key)?);
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
            let event_bytes = estimated_inbound_bytes(event);
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
        let event_bytes = estimated_inbound_bytes(self.latest_time_sync.as_ref()?);
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
            .map(|event| estimated_inbound_bytes(event) as u64)
            .sum()
    }

    pub(crate) fn latest_bytes(&self) -> u64 {
        self.latest_positions
            .values()
            .chain(self.latest_entities.values())
            .chain(self.latest_health.values())
            .chain(self.latest_effects.values())
            .chain(self.latest_time_sync.iter())
            .map(|event| estimated_inbound_bytes(event) as u64)
            .sum()
    }
}

impl NetworkHandle {
    /// Drain join-client inbound without a second field-mirrored map.
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
                        std::mem::size_of::<ClientToGame>() as u64,
                        &crate::perf::queue_stats(crate::perf::QueueCategory::Inbound),
                    ) {
                        Ok(event) => raw.push(event),
                        Err(_) => break,
                    }
                }
                raw
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
