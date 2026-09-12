use super::*;

/// Runtime session overlay kept beside authority `SessionContract`.
/// Interest, the save codec, pose clocks, and teleport allowance live here.
/// Authoritative pose / dimension / username / accepted sequence live on the
/// contract; this record is keyed by `PlayerId` in `ServerRuntime::players`.
#[derive(Debug, Clone)]
pub struct PlayerSessionState {
    pub storage: LocalSessionStorage,
    pub data: PlayerData,
    pub interest: InterestSet,
    pub effects: Vec<PlayerEffectWire>,
    pub(super) pending_initial_chunks: VecDeque<(Dimension, i32, i32)>,
    pub(super) last_projected_session_revision: Option<(Dimension, u64)>,
    /// Last pose/health/anim fingerprint sent as `EntityState` to this session.
    /// Cleared when an entity leaves the simulation set so re-entry is full.
    pub(super) last_projected_entity_states: HashMap<u64, projection::EntityBroadcastFingerprint>,
    /// Dimension under which `interest.chunks` are registered in
    /// `ServerRuntime::chunk_interest_index`. Survives `sync_dimension` so
    /// mutation fanout can remap `(dimension, chunk)` keys correctly.
    pub(super) chunk_index_dimension: Option<Dimension>,
    pub(super) last_pose_sequence: u32,
    pub(super) last_pose_sender_time_millis: u64,
    pub(super) last_pose_received_at: Option<Instant>,
    /// Last pose accepted by the speed/teleport gate. Used only for clocking;
    /// authoritative pose lives on `SessionContract`.
    pub last_pose_position: [f32; 3],
    pub(super) teleport_allowance: Option<[f32; 3]>,
    /// Set when pose / inventory / dimension / gameplay change; cleared after
    /// a successful player-file ack from the save worker.
    pub(super) player_dirty: bool,
}

impl PlayerSessionState {
    fn new(
        storage: LocalSessionStorage,
        data: PlayerData,
        dimension: Dimension,
        view_distance: u8,
        simulation_distance: u8,
    ) -> Self {
        let last_pose_position = data.position;
        Self {
            storage,
            data,
            interest: InterestSet::new(dimension, view_distance, simulation_distance),
            effects: Vec::new(),
            pending_initial_chunks: VecDeque::new(),
            last_projected_session_revision: None,
            last_projected_entity_states: HashMap::new(),
            chunk_index_dimension: None,
            last_pose_sequence: 0,
            last_pose_sender_time_millis: 0,
            last_pose_received_at: None,
            last_pose_position,
            teleport_allowance: None,
            player_dirty: true,
        }
    }

    pub(super) fn queue_initial_chunks(
        &mut self,
        dimension: Dimension,
        chunks: impl IntoIterator<Item = (i32, i32)>,
    ) {
        for (cx, cz) in chunks {
            if self.pending_initial_chunks.len() >= MAX_PENDING_INITIAL_CHUNKS_PER_SESSION {
                break;
            }
            let item = (dimension, cx, cz);
            if !self.pending_initial_chunks.contains(&item) {
                self.pending_initial_chunks.push_back(item);
            }
        }
    }

    pub(super) fn prune_projected_entity_states(&mut self) {
        let stale: Vec<u64> = self
            .last_projected_entity_states
            .keys()
            .copied()
            .filter(|id| !self.interest.simulation_entities.contains(id))
            .collect();
        for id in stale {
            self.last_projected_entity_states.remove(&id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn accept_pose(
        &mut self,
        sequence: u32,
        sender_time_millis: u64,
        position: [f32; 3],
        yaw: f32,
        pitch: f32,
        now: Instant,
    ) -> bool {
        if sequence == 0
            || !position
                .iter()
                .chain([yaw, pitch].iter())
                .all(|value| value.is_finite())
            || position.iter().any(|value| value.abs() > WORLD_BOUND)
        {
            return false;
        }
        if self.last_pose_received_at.is_some() {
            let sequence_delta = sequence.wrapping_sub(self.last_pose_sequence);
            if sequence_delta == 0
                || sequence_delta >= (1 << 31)
                || sender_time_millis <= self.last_pose_sender_time_millis
            {
                return false;
            }
            let target = Vec3::from_array(position);
            let previous = Vec3::from_array(self.last_pose_position);
            let teleport_allowed = self.teleport_allowance.is_some_and(|allowance| {
                target.distance_squared(Vec3::from_array(allowance))
                    <= TELEPORT_ALLOWANCE_RADIUS * TELEPORT_ALLOWANCE_RADIUS
            });
            if !teleport_allowed {
                let sender_delta = sender_time_millis
                    .saturating_sub(self.last_pose_sender_time_millis)
                    .min(MAX_POSE_DELTA_MILLIS);
                let received_delta = self
                    .last_pose_received_at
                    .map(|last| now.saturating_duration_since(last).as_millis() as u64)
                    .unwrap_or_default()
                    .min(MAX_POSE_DELTA_MILLIS);
                let elapsed_seconds = sender_delta.max(received_delta) as f32 / 1_000.0;
                let allowed_distance =
                    POSE_DISTANCE_SLACK_BLOCKS + MAX_POSE_SPEED_BLOCKS_PER_SECOND * elapsed_seconds;
                if previous.distance_squared(target) > allowed_distance * allowed_distance {
                    return false;
                }
            }
        }
        // Pose clocks stay on this overlay; authoritative pose is written by
        // `ServerRuntime::write_pose` after this gate returns true.
        self.last_pose_sequence = sequence;
        self.last_pose_sender_time_millis = sender_time_millis;
        self.last_pose_received_at = Some(now);
        self.last_pose_position = position;
        self.teleport_allowance = None;
        true
    }
}
