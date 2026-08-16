//! Client interpolation buffers for remote players and replicated entities.
//! Presentation-only: these types never own world authority.

use crate::physics::{block_placement_decision, player_aabb_at, BlockPlacementDecision, AABB};
use crate::world::BlockType;
use glam::Vec3;

pub(crate) const REMOTE_SNAPSHOT_CAPACITY: usize = 32;
pub(crate) const REMOTE_INTERPOLATION_DELAY: f64 = 0.1;
pub(crate) const ENTITY_SNAPSHOT_CAPACITY: usize = 8;
pub(crate) const ENTITY_INTERPOLATION_DELAY: f64 = 0.1;
pub(crate) const ENTITY_SNAP_DISTANCE: f32 = 6.0;
pub(crate) const PLAYER_CORRECTION_SNAP_DISTANCE: f32 = 4.0;
pub(crate) const REMOTE_MAX_EXTRAPOLATION: f64 = 0.1;
pub(crate) const REMOTE_MAX_EXTRAPOLATION_SPEED: f32 = 40.0;
pub(crate) const REMOTE_AUTHORITY_MAX_SPEED: f32 = 12.0;
pub(crate) const REMOTE_AUTHORITY_POSITION_TOLERANCE: f32 = 1.0;
pub(crate) const REMOTE_MAX_ANGULAR_SPEED: f32 = std::f32::consts::TAU * 2.0;
pub(crate) const REMOTE_TELEPORT_DISTANCE: f32 = 8.0;
pub(crate) const REMOTE_TELEPORT_GAP: f64 = 0.5;

pub(crate) fn validated_remote_position(
    latest: Option<&PlayerSnapshot>,
    candidate: Vec3,
    sender_time_millis: u64,
) -> Vec3 {
    let Some(latest) = latest else {
        return candidate;
    };
    if sender_time_millis <= latest.sender_time_millis {
        return latest.position;
    }
    let elapsed =
        ((sender_time_millis - latest.sender_time_millis) as f32 / 1_000.0).clamp(0.0, 0.5);
    let max_distance = REMOTE_AUTHORITY_MAX_SPEED * elapsed + REMOTE_AUTHORITY_POSITION_TOLERANCE;
    let delta = candidate - latest.position;
    if delta.length_squared() <= max_distance * max_distance {
        candidate
    } else {
        latest.position + delta.normalize_or_zero() * max_distance
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlayerSnapshot {
    pub(crate) position: Vec3,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) time: f64,
    pub(crate) sequence: u32,
    pub(crate) sender_time_millis: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct RemotePlayerState {
    pub(crate) entity_id: u64,
    pub(crate) snapshots: std::collections::VecDeque<PlayerSnapshot>,
    pub(crate) username: String,
    pub(crate) health: f32,
    pub(crate) hunger: f32,
    pub(crate) is_dead: bool,
    pub(crate) spawn_point: Option<[i32; 3]>,
    pub(crate) spawn_dimension: Option<crate::dimension::Dimension>,
    pub(crate) is_sleeping: bool,
    pub(crate) bed_pos: Option<[i32; 3]>,
    pub(crate) dimension: crate::dimension::Dimension,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EntitySnapshot {
    pub(crate) state: crate::network::protocol::EntityStateWire,
    pub(crate) time: f64,
    pub(crate) sequence: u64,
}

#[derive(Debug)]
pub(crate) struct ReplicatedEntityState {
    pub(crate) local_entity_id: u64,
    pub(crate) snapshots: std::collections::VecDeque<EntitySnapshot>,
}

impl ReplicatedEntityState {
    pub(crate) fn new(local_entity_id: u64) -> Self {
        Self {
            local_entity_id,
            snapshots: std::collections::VecDeque::with_capacity(ENTITY_SNAPSHOT_CAPACITY),
        }
    }

    pub(crate) fn push(
        &mut self,
        state: crate::network::protocol::EntityStateWire,
        sequence: u64,
        arrival_time: f64,
    ) -> bool {
        if !state.position.iter().all(|value| value.is_finite())
            || !state.velocity.iter().all(|value| value.is_finite())
            || !state.yaw.is_finite()
            || !state.pitch.is_finite()
            || !state.health.is_finite()
        {
            return false;
        }
        if self
            .snapshots
            .back()
            .is_some_and(|latest| sequence <= latest.sequence)
        {
            return false;
        }
        let position = Vec3::from_array(state.position);
        let should_snap = self.snapshots.back().is_some_and(|latest| {
            position.distance(Vec3::from_array(latest.state.position)) > ENTITY_SNAP_DISTANCE
        });
        if should_snap {
            self.snapshots.clear();
        } else if self.snapshots.len() == ENTITY_SNAPSHOT_CAPACITY {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(EntitySnapshot {
            state,
            time: arrival_time,
            sequence,
        });
        should_snap
    }

    pub(crate) fn sample(&self, target_time: f64) -> Option<crate::network::protocol::EntityStateWire> {
        let first = self.snapshots.front().copied()?;
        if self.snapshots.len() == 1 || target_time <= first.time {
            return Some(first.state);
        }
        for index in 1..self.snapshots.len() {
            let next = self.snapshots[index];
            if target_time <= next.time {
                let prev = self.snapshots[index - 1];
                let span = (next.time - prev.time).max(f64::EPSILON);
                let t = ((target_time - prev.time) / span).clamp(0.0, 1.0) as f32;
                let mut state = next.state;
                state.position = Vec3::from_array(prev.state.position)
                    .lerp(Vec3::from_array(next.state.position), t)
                    .to_array();
                state.velocity = Vec3::from_array(prev.state.velocity)
                    .lerp(Vec3::from_array(next.state.velocity), t)
                    .to_array();
                state.yaw = prev.state.yaw
                    + ((next.state.yaw - prev.state.yaw + std::f32::consts::PI)
                        .rem_euclid(std::f32::consts::TAU)
                        - std::f32::consts::PI)
                        * t;
                state.pitch = prev.state.pitch + (next.state.pitch - prev.state.pitch) * t;
                state.health = prev.state.health + (next.state.health - prev.state.health) * t;
                return Some(state);
            }
        }
        self.snapshots.back().map(|snapshot| snapshot.state)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapshotPushResult {
    Accepted,
    Snapped,
    Rejected,
}

impl RemotePlayerState {
    pub(crate) fn new(entity_id: u64, username: String) -> Self {
        Self {
            entity_id,
            snapshots: std::collections::VecDeque::with_capacity(REMOTE_SNAPSHOT_CAPACITY),
            username,
            health: 20.0,
            hunger: 20.0,
            is_dead: false,
            spawn_point: None,
            spawn_dimension: None,
            is_sleeping: false,
            bed_pos: None,
            dimension: crate::dimension::Dimension::Overworld,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_snapshot(
        &mut self,
        position: Vec3,
        yaw: f32,
        pitch: f32,
        sequence: u32,
        sender_time_millis: u64,
        arrival_time: f64,
    ) -> SnapshotPushResult {
        if !position.is_finite()
            || !yaw.is_finite()
            || !pitch.is_finite()
            || !arrival_time.is_finite()
        {
            return SnapshotPushResult::Rejected;
        }

        let Some(latest) = self.snapshots.back().copied() else {
            self.snapshots.push_back(PlayerSnapshot {
                position,
                yaw,
                pitch,
                time: arrival_time,
                sequence,
                sender_time_millis,
            });
            return SnapshotPushResult::Snapped;
        };

        if !sequence_is_newer(sequence, latest.sequence)
            || sender_time_millis <= latest.sender_time_millis
        {
            return SnapshotPushResult::Rejected;
        }

        let sender_delta = (sender_time_millis - latest.sender_time_millis) as f64 / 1000.0;
        let should_snap = sender_delta > REMOTE_TELEPORT_GAP
            || position.distance(latest.position) > REMOTE_TELEPORT_DISTANCE;
        let local_time = if should_snap {
            arrival_time
        } else {
            latest.time + sender_delta
        };

        if should_snap {
            self.snapshots.clear();
        } else if self.snapshots.len() == REMOTE_SNAPSHOT_CAPACITY {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(PlayerSnapshot {
            position,
            yaw,
            pitch,
            time: local_time,
            sequence,
            sender_time_millis,
        });

        if should_snap {
            SnapshotPushResult::Snapped
        } else {
            SnapshotPushResult::Accepted
        }
    }

    pub(crate) fn sample(&self, target_time: f64) -> Option<PlayerSnapshot> {
        sample_snapshot_buffer(&self.snapshots, target_time)
    }
}

pub(crate) fn placement_decision_for_players<'a>(
    block: BlockType,
    block_pos: (i32, i32, i32),
    local_player_aabb: AABB,
    remote_players: impl IntoIterator<Item = &'a RemotePlayerState>,
) -> BlockPlacementDecision {
    if !block.properties().is_solid {
        return BlockPlacementDecision::Allowed;
    }

    let mut player_aabbs = vec![local_player_aabb];
    for remote in remote_players {
        let Some(latest) = remote.snapshots.back() else {
            // Until the host has an authenticated pose for every connected
            // player, conservatively reject solid placement rather than risk
            // creating a block inside an unknown player.
            return BlockPlacementDecision::BlockedByPlayer;
        };
        player_aabbs.push(player_aabb_at(latest.position));
    }

    block_placement_decision(block, 0, block_pos, player_aabbs)
}

pub(crate) fn sequence_is_newer(candidate: u32, previous: u32) -> bool {
    let distance = candidate.wrapping_sub(previous);
    distance != 0 && distance < (1 << 31)
}

pub(crate) fn interpolate_snapshot(
    prev: PlayerSnapshot,
    latest: PlayerSnapshot,
    target_time: f64,
) -> PlayerSnapshot {
    let span = (latest.time - prev.time).max(f64::EPSILON);
    let t = ((target_time - prev.time) / span).clamp(0.0, 1.0) as f32;
    PlayerSnapshot {
        position: prev.position.lerp(latest.position, t),
        yaw: prev.yaw
            + ((latest.yaw - prev.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI)
                * t,
        pitch: prev.pitch + (latest.pitch - prev.pitch) * t,
        time: target_time,
        sequence: latest.sequence,
        sender_time_millis: latest.sender_time_millis,
    }
}

pub(crate) fn sample_snapshot_buffer(
    snapshots: &std::collections::VecDeque<PlayerSnapshot>,
    target_time: f64,
) -> Option<PlayerSnapshot> {
    let first = snapshots.front().copied()?;
    if snapshots.len() == 1 || target_time <= first.time {
        return Some(PlayerSnapshot {
            time: target_time,
            ..first
        });
    }

    for index in 1..snapshots.len() {
        let next = snapshots[index];
        if target_time <= next.time {
            return Some(interpolate_snapshot(
                snapshots[index - 1],
                next,
                target_time,
            ));
        }
    }

    let latest = snapshots.back().copied().unwrap();
    let previous = snapshots[snapshots.len() - 2];
    let span = latest.time - previous.time;
    if span <= f64::EPSILON {
        return Some(PlayerSnapshot {
            time: target_time,
            ..latest
        });
    }

    let extrapolation = (target_time - latest.time).clamp(0.0, REMOTE_MAX_EXTRAPOLATION);
    let mut velocity = (latest.position - previous.position) / span as f32;
    let speed = velocity.length();
    if speed > REMOTE_MAX_EXTRAPOLATION_SPEED {
        velocity *= REMOTE_MAX_EXTRAPOLATION_SPEED / speed;
    }
    let yaw_delta = (latest.yaw - previous.yaw + std::f32::consts::PI)
        .rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    let yaw_rate =
        (yaw_delta / span as f32).clamp(-REMOTE_MAX_ANGULAR_SPEED, REMOTE_MAX_ANGULAR_SPEED);
    let pitch_rate = ((latest.pitch - previous.pitch) / span as f32)
        .clamp(-REMOTE_MAX_ANGULAR_SPEED, REMOTE_MAX_ANGULAR_SPEED);

    Some(PlayerSnapshot {
        position: latest.position + velocity * extrapolation as f32,
        yaw: latest.yaw + yaw_rate * extrapolation as f32,
        pitch: (latest.pitch + pitch_rate * extrapolation as f32)
            .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2),
        time: target_time,
        ..latest
    })
}

