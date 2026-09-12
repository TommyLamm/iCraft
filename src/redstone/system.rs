use super::*;


pub type BlockPos = (i32, i32, i32);

pub(crate) const MAX_PROPAGATION_PASSES: usize = 64;
/// Hard cap for delayed redstone work.  Observer pulses and repeaters are
/// coalesced before this bound is reached so a machine cannot grow an
/// unbounded queue during a single host tick.
pub const MAX_SCHEDULED_REDSTONE_TICKS: usize = 4096;
/// Maximum observer baselines evaluated by one host redstone tick.  The
/// rotating start index below gives large streamed worlds bounded work without
/// starving observers after the first page.
pub const MAX_OBSERVER_CHECKS_PER_TICK: usize = 1024;
/// Maximum block-entity snapshots attached to one redstone update.  Entries for
/// the same position are coalesced so a pulse cannot create an unbounded
/// replication burst.
pub(crate) const MAX_REDSTONE_ENTITY_CHANGES_PER_UPDATE: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeKind {
    Unpowered,
    Weak,
    Strong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedstoneState {
    pub power: u8,
    pub charge: ChargeKind,
}

impl Default for RedstoneState {
    fn default() -> Self {
        Self {
            power: 0,
            charge: ChargeKind::Unpowered,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    North,
    South,
    West,
    East,
    Up,
    Down,
}

impl Default for Direction {
    fn default() -> Self {
        Self::North
    }
}

impl Direction {
    /// All six cardinal directions. Deltas match the historical neighbor table
    /// order: +X, -X, +Y, -Y, +Z, -Z.
    pub const ALL: [Self; 6] = [
        Self::East,
        Self::West,
        Self::Up,
        Self::Down,
        Self::South,
        Self::North,
    ];

    pub fn from_yaw(yaw: f32) -> Self {
        let x = yaw.cos();
        let z = yaw.sin();
        if x.abs() >= z.abs() {
            if x >= 0.0 {
                Self::East
            } else {
                Self::West
            }
        } else if z >= 0.0 {
            Self::South
        } else {
            Self::North
        }
    }

    pub const fn delta(self) -> BlockPos {
        match self {
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
            Self::Up => (0, 1, 0),
            Self::Down => (0, -1, 0),
        }
    }

    /// Six neighbor offsets derived from [`Self::ALL`].
    pub const fn all_deltas() -> [BlockPos; 6] {
        [
            Self::East.delta(),
            Self::West.delta(),
            Self::Up.delta(),
            Self::Down.delta(),
            Self::South.delta(),
            Self::North.delta(),
        ]
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }

    pub fn left(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::South => Self::East,
            Self::West => Self::South,
            Self::East => Self::North,
            Self::Up => Self::Up,
            Self::Down => Self::Down,
        }
    }

    pub fn right(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::South => Self::West,
            Self::West => Self::North,
            Self::East => Self::South,
            Self::Up => Self::Up,
            Self::Down => Self::Down,
        }
    }
}

pub(crate) const NEIGHBORS: [BlockPos; 6] = Direction::all_deltas();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparatorMode {
    Compare,
    Subtract,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ComponentState {
    pub(crate) signal: RedstoneState,
    pub(crate) facing: Direction,
    pub(crate) repeater_delay: u8,
    pub(crate) comparator_mode: ComparatorMode,
    pub(crate) note: u8,
    pub(crate) last_powered: bool,
    /// Absolute tick at which an observer pulse ends.  Zero means idle; the
    /// persisted block entity carries the pending countdown across reloads.
    pub(crate) observer_pulse_until: u64,
}

impl ComponentState {
    pub(crate) fn new(block: BlockType, facing: Direction) -> Self {
        // New components start from the type's resting state (torch lit / others off).
        // Live open/powered bits are applied on the next settle from BlockState.
        let power = match block.canonicalize() {
            BlockType::RedstoneTorch => 15,
            _ => 0,
        };
        Self {
            signal: RedstoneState {
                power,
                charge: if power > 0 {
                    ChargeKind::Strong
                } else {
                    ChargeKind::Unpowered
                },
            },
            facing,
            repeater_delay: 1,
            comparator_mode: ComparatorMode::Compare,
            note: 0,
            last_powered: false,
            observer_pulse_until: 0,
        }
    }

    /// Returns `true` when the component carries non-default metadata that must
    /// survive a chunk unload/reload cycle. The runtime default (`new`) state
    /// does not need to be persisted because `sync_loaded_chunks` already
    /// reconstructs it from the block type alone.
    fn has_persistent_metadata(&self) -> bool {
        self.facing != Direction::North
            || self.repeater_delay != 1
            || self.comparator_mode != ComparatorMode::Compare
            || self.note != 0
            // Dispenser/dropper rising-edge state must survive a powered
            // chunk reload; otherwise the first post-load tick would emit a
            // duplicate action.
            || self.last_powered
    }
}

/// Persistent redstone component metadata for a single block inside a chunk.
///
/// `local_x`/`local_z` are chunk-local (0..16). `local_y` is signed world Y
/// (`i16`), matching block-entity coordinates. Only components whose
/// `ComponentState` differs from the runtime default are serialized, so
/// freshly-placed or never-interacted components round-trip as an empty vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedstoneComponentMetadata {
    pub local_x: u8,
    pub local_y: i16,
    pub local_z: u8,
    pub facing: Direction,
    pub repeater_delay: u8,
    pub comparator_mode: ComparatorMode,
    pub note: u8,
    /// Persisted rising-edge latch. `serde(default)` covers self-describing
    /// formats; `ChunkSaveData::redstone_metadata` also has an explicit
    /// bincode fallback for pre-latch sidecars and treats them as unpowered.
    #[serde(default)]
    pub last_powered: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScheduledKind {
    ReleaseButton,
    Repeater(bool),
    Explode,
    ObserverPulseOn,
    ObserverPulseOff,
}

impl ScheduledKind {
    fn encode(self, bytes: &mut Vec<u8>) {
        let (kind, payload) = match self {
            Self::ReleaseButton => (0, 0),
            Self::Repeater(powered) => (1, powered as u8),
            Self::Explode => (2, 0),
            Self::ObserverPulseOn => (3, 0),
            Self::ObserverPulseOff => (4, 0),
        };
        bytes.push(kind);
        bytes.push(payload);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScheduledTick {
    pub(crate) due: u64,
    pub(crate) pos: BlockPos,
    pub(crate) kind: ScheduledKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockMutation {
    pub pos: BlockPos,
    pub old_block: BlockType,
    pub new_block: BlockType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedstoneAction {
    Explode {
        pos: BlockPos,
    },
    Dispense {
        pos: BlockPos,
        facing: Direction,
        dropper: bool,
    },
    PlayNote {
        pos: BlockPos,
        note: u8,
    },
}

#[derive(Debug, Default)]
pub struct RedstoneUpdate {
    pub mutations: Vec<BlockMutation>,
    pub actions: Vec<RedstoneAction>,
    pub block_entity_changes: Vec<(BlockPos, crate::block_entity::BlockEntity)>,
    pub propagation_overflowed: bool,
    /// Number of observer pulse-on edges emitted by this authoritative update.
    pub observer_pulses: u32,
}

#[derive(Default)]
pub struct RedstoneSystem {
    pub(crate) components: HashMap<BlockPos, ComponentState>,
    pub(crate) known_chunks: HashSet<(i32, i32)>,
    /// `WorldColumns::load_generation` observed by the last successful sync.
    /// `u64::MAX` means never synced, so the first tick always rebuilds.
    pub(crate) known_load_generation: u64,
    pub(crate) scheduled: Vec<ScheduledTick>,
    pub(crate) tick: u64,
    pub(crate) dirty: HashSet<BlockPos>,
    pub(crate) sleeping: bool,
    /// The normalized set of plate positions occupied on the previous tick.
    /// Keeping this separate from raw player positions makes duplicate players
    /// and ordering irrelevant when deciding whether a sleeping system can
    /// return without scanning its pressure plates.
    pub(crate) previous_plate_occupants: HashSet<BlockPos>,
    /// Scratch set reused by plate-occupant normalization (avoids per-tick alloc).
    pub(crate) plate_occupant_scratch: HashSet<BlockPos>,
    /// Comparator positions for container-revision refresh (no full-table filter).
    pub(crate) comparator_positions: HashSet<BlockPos>,
    /// Components that participate in `apply_component_transitions`.
    pub(crate) transition_positions: HashSet<BlockPos>,
    /// Component positions grouped by resident column for O(column) metadata.
    pub(crate) column_components: HashMap<(i32, i32), HashSet<BlockPos>>,
    /// Last revision observed behind each comparator.  This dependency map
    /// lets direct container mutations wake a sleeping redstone system without
    /// rescanning every comparator every tick.
    pub(crate) container_revisions: HashMap<BlockPos, u64>,
    /// Positions whose scheduled ticks fired this tick (transition candidates).
    pub(crate) due_positions_scratch: Vec<BlockPos>,
    #[cfg(test)]
    pub(crate) pressure_plate_scans: u64,
    #[cfg(test)]
    pub(crate) component_sync_scans: u64,
    #[cfg(test)]
    pub(crate) container_revision_scans: u64,
    #[cfg(test)]
    pub(crate) observer_scans: u64,
    #[cfg(test)]
    pub(crate) loaded_chunk_key_probes: u64,
}

#[allow(dead_code)]

impl RedstoneSystem {
    pub fn new() -> Self {
        Self {
            known_load_generation: u64::MAX,
            ..Self::default()
        }
    }

    pub fn is_sleeping(&self) -> bool {
        self.sleeping
    }

    pub fn current_tick(&self) -> u64 {
        self.tick
    }

    /// Bounded delayed-work depth for the debug HUD and host telemetry.
    pub fn scheduled_len(&self) -> usize {
        self.scheduled.len()
    }

    /// Returns a deterministic, versioned byte representation of all runtime
    /// redstone state. HashMap/HashSet-backed collections are sorted before
    /// encoding; the scheduled vector retains queue order because entries with
    /// the same due tick are processed in insertion order.
    ///
    /// The snapshot intentionally contains runtime bookkeeping (loaded chunks,
    /// dirty positions, sleeping state, and the normalized pressure-plate
    /// occupant set) in addition to component signal/metadata. This keeps
    /// checksums sensitive to state that affects the next simulation tick.
    pub fn canonical_snapshot(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"redstone-state-v1\0");
        bytes.extend_from_slice(&self.tick.to_le_bytes());
        bytes.push(self.sleeping as u8);

        let mut known_chunks: Vec<_> = self.known_chunks.iter().copied().collect();
        known_chunks.sort_unstable();
        append_len(&mut bytes, known_chunks.len());
        for (cx, cz) in known_chunks {
            append_i32(&mut bytes, cx);
            append_i32(&mut bytes, cz);
        }

        let mut components: Vec<_> = self.components.iter().collect();
        components.sort_unstable_by_key(|(pos, _)| **pos);
        append_len(&mut bytes, components.len());
        for (&pos, state) in components {
            append_pos(&mut bytes, pos);
            bytes.push(state.signal.power);
            bytes.push(encode_charge(state.signal.charge));
            bytes.push(encode_direction(state.facing));
            bytes.push(state.repeater_delay);
            bytes.push(encode_comparator_mode(state.comparator_mode));
            bytes.push(state.note);
            bytes.push(state.last_powered as u8);
            bytes.extend_from_slice(&state.observer_pulse_until.to_le_bytes());
        }

        // `scheduled` is a queue, so preserve its order while still encoding
        // every entry and its complete discriminator/payload.
        append_len(&mut bytes, self.scheduled.len());
        for scheduled in &self.scheduled {
            bytes.extend_from_slice(&scheduled.due.to_le_bytes());
            append_pos(&mut bytes, scheduled.pos);
            scheduled.kind.encode(&mut bytes);
        }

        let mut dirty: Vec<_> = self.dirty.iter().copied().collect();
        dirty.sort_unstable();
        append_len(&mut bytes, dirty.len());
        for pos in dirty {
            append_pos(&mut bytes, pos);
        }

        let mut occupants: Vec<_> = self.previous_plate_occupants.iter().copied().collect();
        occupants.sort_unstable();
        append_len(&mut bytes, occupants.len());
        for pos in occupants {
            append_pos(&mut bytes, pos);
        }

        bytes
    }

    /// Computes the canonical FNV-1a checksum for [`Self::canonical_snapshot`].
    pub fn canonical_checksum(&self) -> u64 {
        fnv1a(&self.canonical_snapshot())
    }

    pub fn power_at(&self, pos: BlockPos) -> u8 {
        self.components
            .get(&pos)
            .map(|state| state.signal.power)
            .unwrap_or(0)
    }

    pub fn block_state_at(&self, manager: &WorldColumns, pos: BlockPos) -> RedstoneState {
        if let Some(state) = self.components.get(&pos) {
            return state.signal;
        }
        let strong = strong_power_into(manager, &self.components, pos);
        if strong > 0 {
            return RedstoneState {
                power: strong,
                charge: ChargeKind::Strong,
            };
        }
        let weak = incoming_power(manager, &self.components, pos, false);
        RedstoneState {
            power: weak,
            charge: if weak > 0 {
                ChargeKind::Weak
            } else {
                ChargeKind::Unpowered
            },
        }
    }

    pub fn repeater_delay(&self, pos: BlockPos) -> Option<u8> {
        self.components.get(&pos).map(|state| state.repeater_delay)
    }

    pub fn comparator_mode(&self, pos: BlockPos) -> Option<ComparatorMode> {
        self.components.get(&pos).map(|state| state.comparator_mode)
    }

    pub fn set_repeater_delay(&mut self, pos: BlockPos, delay: u8) {
        if let Some(state) = self.components.get_mut(&pos) {
            state.repeater_delay = delay.clamp(1, 4);
            self.mark_dirty(pos);
        }
    }

    pub fn set_comparator_mode(&mut self, pos: BlockPos, mode: ComparatorMode) {
        if let Some(state) = self.components.get_mut(&pos) {
            state.comparator_mode = mode;
            self.mark_dirty(pos);
        }
    }

    /// Collects persistent metadata for every redstone component whose runtime
    /// state differs from the default that `sync_loaded_chunks` would rebuild
    /// after a reload. Callers persist the returned list as a sidecar on the
    /// chunk save data so that repeater delays, comparator modes, note pitches,
    /// and component facings survive chunk unload/reload cycles.
    ///
    /// Only components whose chunk-local coordinates fall inside `(cx, cz)` are
    /// emitted. Components that straddle a chunk boundary are still indexed by
    /// their own block position, so each one belongs to exactly one chunk.
    pub fn collect_chunk_metadata(
        &self,
        manager: &WorldColumns,
        cx: i32,
        cz: i32,
    ) -> Vec<RedstoneComponentMetadata> {
        let mut metadata = Vec::new();
        let origin_x = cx * CHUNK_WIDTH as i32;
        let origin_z = cz * CHUNK_DEPTH as i32;
        let Some(positions) = self.column_components.get(&(cx, cz)) else {
            return metadata;
        };
        for &pos in positions {
            let Some(state) = self.components.get(&pos) else {
                continue;
            };
            let local_x = pos.0 - origin_x;
            let local_z = pos.2 - origin_z;
            if local_x < 0
                || local_x >= CHUNK_WIDTH as i32
                || local_z < 0
                || local_z >= CHUNK_DEPTH as i32
            {
                continue;
            }
            if !manager.dimension.height().contains_y(pos.1) {
                continue;
            }
            if !state.has_persistent_metadata() {
                continue;
            }
            metadata.push(RedstoneComponentMetadata {
                local_x: local_x as u8,
                local_y: pos.1 as i16,
                local_z: local_z as u8,
                facing: state.facing,
                repeater_delay: state.repeater_delay,
                comparator_mode: state.comparator_mode,
                note: state.note,
                last_powered: state.last_powered,
            });
        }
        metadata
    }

    /// Reapplies previously persisted component metadata after a chunk has been
    /// (re)loaded and `sync_loaded_chunks` has rebuilt default `ComponentState`
    /// entries. Entries whose block no longer matches a redstone component are
    /// ignored so stale metadata cannot resurrect facings on unrelated blocks.
    pub(crate) fn restore_chunk_metadata(
        &mut self,
        manager: &WorldColumns,
        cx: i32,
        cz: i32,
        metadata: &[RedstoneComponentMetadata],
    ) {
        if metadata.is_empty() {
            return;
        }
        let origin_x = cx * CHUNK_WIDTH as i32;
        let origin_z = cz * CHUNK_DEPTH as i32;
        for entry in metadata {
            let wx = origin_x + entry.local_x as i32;
            let wy = entry.local_y as i32;
            let wz = origin_z + entry.local_z as i32;
            let pos = (wx, wy, wz);
            let block = get_block(manager, pos);
            if !is_component(block) {
                continue;
            }
            let state = self
                .components
                .entry(pos)
                .or_insert_with(|| ComponentState::new(block, Direction::North));
            state.facing = entry.facing;
            state.repeater_delay = entry.repeater_delay.clamp(1, 4);
            state.comparator_mode = entry.comparator_mode;
            state.note = entry.note.min(24);
            state.last_powered = entry.last_powered;
            self.index_component(pos, block);
            self.mark_dirty(pos);
        }
    }

    pub(crate) fn mark_dirty(&mut self, pos: BlockPos) {
        if self.components.contains_key(&pos) {
            self.dirty.insert(pos);
            self.sleeping = false;
        }
    }

    pub(crate) fn schedule_tick(&mut self, scheduled: ScheduledTick) -> bool {
        if self
            .scheduled
            .iter()
            .any(|existing| existing.pos == scheduled.pos && existing.kind == scheduled.kind)
        {
            return false;
        }
        if self.scheduled.len() >= MAX_SCHEDULED_REDSTONE_TICKS {
            return false;
        }
        self.scheduled.push(scheduled);
        self.scheduled
            .sort_unstable_by_key(|entry| (entry.due, entry.pos, scheduled_kind_key(entry.kind)));
        self.sleeping = false;
        true
    }

    pub(crate) fn mark_neighbors_dirty(&mut self, manager: &WorldColumns, pos: BlockPos) {
        if self.components.contains_key(&pos) {
            self.dirty.insert(pos);
            self.sleeping = false;
        }
        for offset in NEIGHBORS {
            let n1 = add(pos, offset);
            if self.components.contains_key(&n1) {
                self.dirty.insert(n1);
                self.sleeping = false;
            }
            if get_block(manager, n1).properties().is_solid {
                for off2 in NEIGHBORS {
                    let n2 = add(n1, off2);
                    if self.components.contains_key(&n2) {
                        self.dirty.insert(n2);
                        self.sleeping = false;
                    }
                }
            }
        }
    }

    pub fn mark_container_changed(&mut self, manager: &WorldColumns, pos: BlockPos) {
        self.mark_neighbors_dirty(manager, pos);
    }

    pub(crate) fn column_of(pos: BlockPos) -> (i32, i32) {
        (
            pos.0.div_euclid(CHUNK_WIDTH as i32),
            pos.2.div_euclid(CHUNK_DEPTH as i32),
        )
    }

    pub(crate) fn index_component(&mut self, pos: BlockPos, block: BlockType) {
        self.column_components
            .entry(Self::column_of(pos))
            .or_default()
            .insert(pos);
        if is_comparator_block(block) {
            self.comparator_positions.insert(pos);
        } else {
            self.comparator_positions.remove(&pos);
        }
        if is_transition_capable(block) {
            self.transition_positions.insert(pos);
        } else {
            self.transition_positions.remove(&pos);
        }
    }

    pub(crate) fn unindex_component(&mut self, pos: BlockPos) {
        let column = Self::column_of(pos);
        if let Some(set) = self.column_components.get_mut(&column) {
            set.remove(&pos);
            if set.is_empty() {
                self.column_components.remove(&column);
            }
        }
        self.comparator_positions.remove(&pos);
        self.transition_positions.remove(&pos);
    }

    pub fn on_block_changed(&mut self, manager: &WorldColumns, pos: BlockPos, facing: Direction) {
        let block = get_block(manager, pos);
        if is_component(block) {
            self.components
                .entry(pos)
                .and_modify(|state| state.facing = facing)
                .or_insert_with(|| ComponentState::new(block, facing));
            self.index_component(pos, block);
        } else {
            self.components.remove(&pos);
            self.unindex_component(pos);
            self.scheduled.retain(|scheduled| {
                scheduled.pos != pos || scheduled.kind == ScheduledKind::Explode
            });
        }
        self.mark_neighbors_dirty(manager, pos);
    }

    pub fn interact(&mut self, manager: &mut WorldColumns, pos: BlockPos) -> RedstoneUpdate {
        self.sync_loaded_chunks(manager);
        let block = get_block(manager, pos);
        let mut update = RedstoneUpdate::default();
        match block {
            BlockType::Lever => {
                let open = !block_open_at(manager, pos);
                set_open_flag(manager, pos, BlockType::Lever, open, &mut update.mutations);
            }
            BlockType::StoneButton => {
                set_open_flag(
                    manager,
                    pos,
                    BlockType::StoneButton,
                    true,
                    &mut update.mutations,
                );
                self.scheduled.retain(|scheduled| {
                    !(scheduled.pos == pos && scheduled.kind == ScheduledKind::ReleaseButton)
                });
                self.schedule_tick(ScheduledTick {
                    due: self.tick + 20,
                    pos,
                    kind: ScheduledKind::ReleaseButton,
                });
                self.sleeping = false;
            }
            BlockType::Repeater => {
                if let Some(state) = self.components.get_mut(&pos) {
                    state.repeater_delay = state.repeater_delay % 4 + 1;
                }
            }
            BlockType::Comparator => {
                if let Some(state) = self.components.get_mut(&pos) {
                    state.comparator_mode = match state.comparator_mode {
                        ComparatorMode::Compare => ComparatorMode::Subtract,
                        ComparatorMode::Subtract => ComparatorMode::Compare,
                    };
                }
            }
            BlockType::NoteBlock => {
                let note = if let Some(state) = self.components.get_mut(&pos) {
                    state.note = (state.note + 1) % 25;
                    state.note
                } else {
                    0
                };
                update.actions.push(RedstoneAction::PlayNote { pos, note });
            }
            _ => return update,
        }
        self.mark_neighbors_dirty(manager, pos);
        self.reconcile_mutations(manager, &update.mutations);
        update
    }

    pub fn tick(&mut self, manager: &mut WorldColumns, occupants: &[BlockPos]) -> RedstoneUpdate {
        self.tick = self.tick.wrapping_add(1);

        // Sleep early-out before sync / plate HashSet work when residency and
        // plate occupancy are unchanged.
        if self.sleeping && self.dirty.is_empty() && self.scheduled.is_empty() {
            if self.known_load_generation == manager.load_generation() {
                fill_plate_occupants(
                    &mut self.plate_occupant_scratch,
                    &self.components,
                    manager,
                    occupants,
                );
                if self.plate_occupant_scratch == self.previous_plate_occupants {
                    return RedstoneUpdate::default();
                }
            }
        }

        self.sync_loaded_chunks(manager);
        let mut update = RedstoneUpdate::default();
        fill_plate_occupants(
            &mut self.plate_occupant_scratch,
            &self.components,
            manager,
            occupants,
        );
        let normalized_occupants = self.plate_occupant_scratch.clone();

        if self.sleeping
            && self.dirty.is_empty()
            && self.scheduled.is_empty()
            && normalized_occupants == self.previous_plate_occupants
        {
            return update;
        }

        self.refresh_container_revisions(manager);
        self.update_observers(manager, &mut update.block_entity_changes);

        self.due_positions_scratch.clear();
        self.process_scheduled(manager, &mut update);
        self.update_pressure_plates(manager, occupants, &mut update.mutations);

        let (converged, power_changed) = self.settle_power(manager);
        update.propagation_overflowed = !converged;
        self.apply_component_transitions(manager, &mut update, &power_changed);
        self.reconcile_mutations(manager, &update.mutations);

        if self.dirty.is_empty() && self.scheduled.is_empty() {
            self.sleeping = true;
        }
        self.previous_plate_occupants = normalized_occupants;

        update
    }

    pub(crate) fn refresh_container_revisions(&mut self, manager: &WorldColumns) {
        #[cfg(test)]
        {
            self.container_revision_scans += 1;
        }
        let mut comparators: Vec<BlockPos> = self.comparator_positions.iter().copied().collect();
        comparators.sort_unstable();
        let mut seen = HashSet::new();
        for pos in comparators {
            let Some(state) = self.components.get(&pos).copied() else {
                self.comparator_positions.remove(&pos);
                continue;
            };
            let rear = sub(pos, state.facing.delta());
            let revision = container_revision(manager, rear);
            seen.insert(pos);
            match self.container_revisions.insert(pos, revision) {
                Some(previous) if previous != revision => self.mark_neighbors_dirty(manager, pos),
                None => {}
                _ => {}
            }
        }
        self.container_revisions.retain(|pos, _| seen.contains(pos));
    }

    /// Compares each loaded observer's front block/state/entity revision to its
    /// persisted baseline.  Baselines are initialized without a pulse, while a
    /// real change schedules one bounded two-tick pulse.  An unloaded front is
    /// intentionally skipped so streaming cannot create a false edge.
    pub(crate) fn update_observers(
        &mut self,
        manager: &mut WorldColumns,
        block_entity_changes: &mut Vec<(BlockPos, crate::block_entity::BlockEntity)>,
    ) {
        let mut observers: Vec<BlockPos> = self
            .components
            .iter()
            .filter_map(|(&pos, _)| (get_block(manager, pos) == BlockType::Observer).then_some(pos))
            .collect();
        observers.sort_unstable();
        #[cfg(test)]
        {
            self.observer_scans += 1;
        }
        if observers.is_empty() {
            return;
        }
        let start = (self.tick as usize) % observers.len();
        let checks = observers.len().min(MAX_OBSERVER_CHECKS_PER_TICK);
        for offset in 0..checks {
            let pos = observers[(start + offset) % observers.len()];
            let facing = self
                .components
                .get(&pos)
                .map(|state| state.facing)
                .unwrap_or_default();
            let front = add(pos, facing.delta());
            if !manager.is_block_loaded(front.0, front.1, front.2) {
                continue;
            }
            let Some(mut observer) = manager
                .get_block_entity(pos.0, pos.1, pos.2)
                .cloned()
                .and_then(|entity| {
                    if let crate::block_entity::BlockEntity::Observer(observer) = entity {
                        Some(observer)
                    } else {
                        None
                    }
                })
            else {
                continue;
            };
            let observed_block = get_block(manager, front);
            let observed_state = manager.get_block_state(front.0, front.1, front.2);
            let observed_entity_revision = manager
                .get_block_entity(front.0, front.1, front.2)
                .map(crate::block_entity::BlockEntity::revision)
                .unwrap_or(0);
            let observed_entity_present = manager
                .get_block_entity(front.0, front.1, front.2)
                .is_some();
            let changed = observer.baseline_initialized
                && (observer.observed_block != observed_block
                    || observer.observed_state != observed_state
                    || observer.observed_entity_revision != observed_entity_revision
                    || observer.observed_entity_present != observed_entity_present);
            observer.observed_block = observed_block;
            observer.observed_state = observed_state;
            observer.observed_entity_revision = observed_entity_revision;
            observer.observed_entity_present = observed_entity_present;
            if !observer.baseline_initialized || changed {
                observer.baseline_initialized = true;
                if changed {
                    observer.pending_pulse = observer.pending_pulse.max(2);
                }
                observer.revision = observer.revision.wrapping_add(1);
                manager.set_block_entity(
                    pos.0,
                    pos.1,
                    pos.2,
                    Some(crate::block_entity::BlockEntity::Observer(observer.clone())),
                );
                manager.mark_block_entity_dirty(pos.0, pos.2);
                record_block_entity_change(
                    block_entity_changes,
                    pos,
                    crate::block_entity::BlockEntity::Observer(observer.clone()),
                );
                self.mark_dirty(pos);
            }
            if observer.pending_pulse > 0
                && !self.scheduled.iter().any(|scheduled| {
                    scheduled.pos == pos
                        && matches!(
                            scheduled.kind,
                            ScheduledKind::ObserverPulseOn | ScheduledKind::ObserverPulseOff
                        )
                })
            {
                self.schedule_tick(ScheduledTick {
                    due: self.tick + 1,
                    pos,
                    kind: ScheduledKind::ObserverPulseOn,
                });
            }
        }
    }

    pub(crate) fn sync_loaded_chunks(&mut self, manager: &WorldColumns) {
        if self.known_load_generation == manager.load_generation() {
            return;
        }
        self.sleeping = false;

        #[cfg(test)]
        {
            self.component_sync_scans += self.components.len() as u64;
        }
        self.known_chunks
            .retain(|chunk_pos| manager.chunks.contains_key(chunk_pos));
        let removed: Vec<BlockPos> = self
            .components
            .keys()
            .copied()
            .filter(|pos| {
                let cx = pos.0.div_euclid(CHUNK_WIDTH as i32);
                let cz = pos.2.div_euclid(CHUNK_DEPTH as i32);
                !manager.chunks.contains_key(&(cx, cz)) || !is_component(get_block(manager, *pos))
            })
            .collect();
        for pos in removed {
            self.components.remove(&pos);
            self.unindex_component(pos);
        }
        self.dirty.retain(|pos| self.components.contains_key(pos));

        for ((cx, cz), chunk) in manager.chunks.iter() {
            if !self.known_chunks.insert((cx, cz)) {
                continue;
            }
            let origin_x = cx * CHUNK_WIDTH as i32;
            let origin_z = cz * CHUNK_DEPTH as i32;
            for &encoded in chunk.redstone_positions() {
                let (x, y, z) = crate::world::Chunk::decode_torch_position(encoded);
                let pos = (origin_x + x as i32, y as i32, origin_z + z as i32);
                let block = chunk.get_block_local(x, y, z);
                use std::collections::hash_map::Entry;
                if let Entry::Vacant(e) = self.components.entry(pos) {
                    let mut component = ComponentState::new(block, Direction::North);
                    if block == BlockType::Observer {
                        if let Some(crate::block_entity::BlockEntity::Observer(observer)) =
                            chunk.get_block_entity(x as u8, y as i16, z as u8)
                        {
                            component.facing = observer.facing;
                            if observer.pending_pulse > 0 {
                                component.observer_pulse_until = self.tick + 2;
                            }
                        }
                    }
                    e.insert(component);
                    self.index_component(pos, block);
                    self.mark_dirty(pos);
                }
            }
        }
        self.known_load_generation = manager.load_generation();
    }

    pub(crate) fn reconcile_mutations(&mut self, manager: &WorldColumns, mutations: &[BlockMutation]) {
        for mutation in mutations {
            if is_component(mutation.new_block) {
                self.components
                    .entry(mutation.pos)
                    .or_insert_with(|| ComponentState::new(mutation.new_block, Direction::North));
                self.index_component(mutation.pos, mutation.new_block);
            } else {
                self.components.remove(&mutation.pos);
                self.unindex_component(mutation.pos);
                self.scheduled.retain(|scheduled| {
                    scheduled.pos != mutation.pos || scheduled.kind == ScheduledKind::Explode
                });
            }
            self.mark_neighbors_dirty(manager, mutation.pos);
        }
        let stale: Vec<BlockPos> = self
            .components
            .keys()
            .copied()
            .filter(|&pos| !is_component(get_block(manager, pos)))
            .collect();
        for pos in stale {
            self.components.remove(&pos);
            self.unindex_component(pos);
        }
    }

    pub(crate) fn process_scheduled(&mut self, manager: &mut WorldColumns, update: &mut RedstoneUpdate) {
        let (due, future): (Vec<_>, Vec<_>) = std::mem::take(&mut self.scheduled)
            .into_iter()
            .partition(|s| s.due <= self.tick);
        self.scheduled = future;
        self.due_positions_scratch.clear();
        for scheduled in &due {
            self.due_positions_scratch.push(scheduled.pos);
        }

        for scheduled in due {
            match scheduled.kind {
                ScheduledKind::ReleaseButton => {
                    if get_block(manager, scheduled.pos) == BlockType::StoneButton
                        && block_open_at(manager, scheduled.pos)
                    {
                        set_open_flag(
                            manager,
                            scheduled.pos,
                            BlockType::StoneButton,
                            false,
                            &mut update.mutations,
                        );
                        self.mark_neighbors_dirty(manager, scheduled.pos);
                    }
                }
                ScheduledKind::Repeater(powered) => {
                    let block = get_block(manager, scheduled.pos);
                    if matches!(block, BlockType::Repeater) {
                        set_open_flag(
                            manager,
                            scheduled.pos,
                            BlockType::Repeater,
                            powered,
                            &mut update.mutations,
                        );
                        self.mark_neighbors_dirty(manager, scheduled.pos);
                    }
                }
                ScheduledKind::Explode => update
                    .actions
                    .push(RedstoneAction::Explode { pos: scheduled.pos }),
                ScheduledKind::ObserverPulseOn => {
                    if get_block(manager, scheduled.pos) == BlockType::Observer {
                        update.observer_pulses = update.observer_pulses.saturating_add(1);
                        if let Some(state) = self.components.get_mut(&scheduled.pos) {
                            state.signal.power = 15;
                            state.signal.charge = ChargeKind::Weak;
                            state.observer_pulse_until = self.tick + 2;
                        }
                        let changed_entity =
                            if let Some(crate::block_entity::BlockEntity::Observer(observer)) =
                                manager.get_block_entity_mut(
                                    scheduled.pos.0,
                                    scheduled.pos.1,
                                    scheduled.pos.2,
                                )
                            {
                                observer.pending_pulse = observer.pending_pulse.saturating_sub(1);
                                observer.revision = observer.revision.wrapping_add(1);
                                Some(crate::block_entity::BlockEntity::Observer(observer.clone()))
                            } else {
                                None
                            };
                        if let Some(changed_entity) = changed_entity {
                            manager.mark_block_entity_dirty(scheduled.pos.0, scheduled.pos.2);
                            record_block_entity_change(
                                &mut update.block_entity_changes,
                                scheduled.pos,
                                changed_entity,
                            );
                        }
                        self.mark_neighbors_dirty(manager, scheduled.pos);
                        self.schedule_tick(ScheduledTick {
                            due: self.tick + 2,
                            pos: scheduled.pos,
                            kind: ScheduledKind::ObserverPulseOff,
                        });
                    }
                }
                ScheduledKind::ObserverPulseOff => {
                    if get_block(manager, scheduled.pos) == BlockType::Observer {
                        if let Some(state) = self.components.get_mut(&scheduled.pos) {
                            state.signal.power = 0;
                            state.signal.charge = ChargeKind::Unpowered;
                            state.observer_pulse_until = 0;
                        }
                        let changed_entity =
                            if let Some(crate::block_entity::BlockEntity::Observer(observer)) =
                                manager.get_block_entity_mut(
                                    scheduled.pos.0,
                                    scheduled.pos.1,
                                    scheduled.pos.2,
                                )
                            {
                                observer.pending_pulse = 0;
                                observer.revision = observer.revision.wrapping_add(1);
                                Some(crate::block_entity::BlockEntity::Observer(observer.clone()))
                            } else {
                                None
                            };
                        if let Some(changed_entity) = changed_entity {
                            manager.mark_block_entity_dirty(scheduled.pos.0, scheduled.pos.2);
                            record_block_entity_change(
                                &mut update.block_entity_changes,
                                scheduled.pos,
                                changed_entity,
                            );
                        }
                        self.mark_neighbors_dirty(manager, scheduled.pos);
                    }
                }
            }
        }
    }

    pub(crate) fn update_pressure_plates(
        &mut self,
        manager: &mut WorldColumns,
        occupants: &[BlockPos],
        mutations: &mut Vec<BlockMutation>,
    ) {
        #[cfg(test)]
        {
            self.pressure_plate_scans += 1;
        }
        let plates: Vec<BlockPos> = self
            .components
            .iter()
            .filter_map(|(&pos, _)| {
                matches!(
                    get_block(manager, pos),
                    BlockType::PressurePlate
                )
                .then_some(pos)
            })
            .collect();
        for pos in plates {
            let occupied = occupants.iter().any(|occupant| {
                occupant.0 == pos.0 && occupant.2 == pos.2 && occupant.1 == pos.1 + 1
            });
            let current_block = get_block(manager, pos);
            if current_block == BlockType::PressurePlate
                && block_open_at(manager, pos) != occupied
            {
                set_open_flag(manager, pos, BlockType::PressurePlate, occupied, mutations);
                self.mark_neighbors_dirty(manager, pos);
            }
        }
    }

    pub(crate) fn settle_power(&mut self, manager: &WorldColumns) -> (bool, HashSet<BlockPos>) {
        // Positions evaluated this settle — transition scheduling (repeaters)
        // needs them even when own power did not change.
        let mut evaluated = HashSet::new();
        if self.dirty.is_empty() {
            return (true, evaluated);
        }

        let mut current_dirty = std::mem::take(&mut self.dirty);
        let mut next_dirty = HashSet::new();
        let max_evaluations = (self.components.len() * MAX_PROPAGATION_PASSES).max(1024);
        let mut evaluations = 0;

        for _pass in 0..MAX_PROPAGATION_PASSES {
            if current_dirty.is_empty() {
                return (true, evaluated);
            }

            for pos in current_dirty.drain() {
                evaluations += 1;
                if evaluations > max_evaluations {
                    self.dirty.extend(next_dirty);
                    return (false, evaluated);
                }
                evaluated.insert(pos);

                let Some(state) = self.components.get(&pos).copied() else {
                    continue;
                };
                let block = get_block(manager, pos);
                let new_power = desired_power(manager, &self.components, pos, block, state);
                let new_charge = if new_power == 0 {
                    ChargeKind::Unpowered
                } else if is_strong_source(manager, pos, block) {
                    ChargeKind::Strong
                } else {
                    ChargeKind::Weak
                };

                if state.signal.power != new_power || state.signal.charge != new_charge {
                    if let Some(mut_state) = self.components.get_mut(&pos) {
                        mut_state.signal.power = new_power;
                        mut_state.signal.charge = new_charge;
                    }
                    next_dirty.insert(pos);
                    for offset in NEIGHBORS {
                        let n1 = add(pos, offset);
                        if self.components.contains_key(&n1) {
                            next_dirty.insert(n1);
                        }
                        if get_block(manager, n1).properties().is_solid {
                            for off2 in NEIGHBORS {
                                let n2 = add(n1, off2);
                                if self.components.contains_key(&n2) {
                                    next_dirty.insert(n2);
                                }
                            }
                        }
                    }
                }
            }

            std::mem::swap(&mut current_dirty, &mut next_dirty);
        }

        if !current_dirty.is_empty() {
            self.dirty.extend(current_dirty);
            return (false, evaluated);
        }

        (true, evaluated)
    }

    pub(crate) fn apply_component_transitions(
        &mut self,
        manager: &mut WorldColumns,
        update: &mut RedstoneUpdate,
        settle_evaluated: &HashSet<BlockPos>,
    ) {
        let mut positions: HashSet<BlockPos> = HashSet::new();
        for &pos in settle_evaluated {
            if self.transition_positions.contains(&pos) {
                positions.insert(pos);
            }
        }
        for &pos in &self.due_positions_scratch {
            if self.transition_positions.contains(&pos) {
                positions.insert(pos);
            }
        }
        let mut positions: Vec<BlockPos> = positions.into_iter().collect();
        positions.sort_unstable();
        for pos in positions {
            let block = get_block(manager, pos);
            let Some(mut state) = self.components.get(&pos).copied() else {
                continue;
            };

            match block {
                BlockType::RedstoneTorch
                | BlockType::Comparator
                | BlockType::RedstoneLamp => {
                    apply_powered_open_state(
                        manager,
                        pos,
                        block,
                        state.signal.power > 0,
                        &mut update.mutations,
                    );
                }
                BlockType::Repeater => {
                    let behind = sub(pos, state.facing.delta());
                    let input = signal_from_position(manager, &self.components, behind, pos, false);
                    let desired = input > 0;
                    let current = block_open_at(manager, pos);
                    let already_scheduled = self.scheduled.iter().any(|scheduled| {
                        scheduled.pos == pos && matches!(scheduled.kind, ScheduledKind::Repeater(_))
                    });
                    if desired != current && !already_scheduled {
                        self.schedule_tick(ScheduledTick {
                            due: self.tick + state.repeater_delay as u64,
                            pos,
                            kind: ScheduledKind::Repeater(desired),
                        });
                    }
                }
                BlockType::OakDoor | BlockType::OakTrapdoor => {
                    let is_open = state.signal.power > 0;
                    set_open_flag(manager, pos, block, is_open, &mut update.mutations);
                }
                BlockType::Piston | BlockType::StickyPiston => {
                    let powered = state.signal.power > 0;
                    let extended = block_open_at(manager, pos);
                    if powered && !state.last_powered && !extended {
                        self.extend_piston(
                            manager,
                            pos,
                            state.facing,
                            block,
                            &mut update.mutations,
                        );
                    } else if !powered && state.last_powered && extended {
                        self.retract_piston(
                            manager,
                            pos,
                            state.facing,
                            block,
                            &mut update.mutations,
                        );
                    }
                }
                BlockType::TNT if state.signal.power > 0 && !state.last_powered => {
                    set_block_record(manager, pos, BlockType::Air, &mut update.mutations);
                    self.schedule_tick(ScheduledTick {
                        due: self.tick + 80,
                        pos,
                        kind: ScheduledKind::Explode,
                    });
                }
                BlockType::Dispenser | BlockType::Dropper
                    if state.signal.power > 0 && !state.last_powered =>
                {
                    update.actions.push(RedstoneAction::Dispense {
                        pos,
                        facing: state.facing,
                        dropper: block == BlockType::Dropper,
                    });
                }
                BlockType::NoteBlock if state.signal.power > 0 && !state.last_powered => {
                    update.actions.push(RedstoneAction::PlayNote {
                        pos,
                        note: state.note,
                    });
                }
                _ => {}
            }

            state.last_powered = state.signal.power > 0;
            if let Some(current) = self.components.get_mut(&pos) {
                current.last_powered = state.last_powered;
            }
        }
    }

}
