use crate::chunk_manager::ChunkManager;
use crate::world::{BlockType, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub type BlockPos = (i32, i32, i32);

const NEIGHBORS: [BlockPos; 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];
const MAX_PROPAGATION_PASSES: usize = 64;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    North,
    South,
    West,
    East,
}

impl Direction {
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

    pub fn delta(self) -> BlockPos {
        match self {
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
        }
    }

    fn left(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::South => Self::East,
            Self::West => Self::South,
            Self::East => Self::North,
        }
    }

    fn right(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::South => Self::West,
            Self::West => Self::North,
            Self::East => Self::South,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparatorMode {
    Compare,
    Subtract,
}

#[derive(Debug, Clone, Copy)]
struct ComponentState {
    signal: RedstoneState,
    facing: Direction,
    repeater_delay: u8,
    comparator_mode: ComparatorMode,
    note: u8,
    last_powered: bool,
}

impl ComponentState {
    fn new(block: BlockType, facing: Direction) -> Self {
        let power = source_power(block);
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
    }
}

/// Direction encoding used by the redstone metadata sidecar. Independent of
/// the runtime `Direction` enum so the on-disk format stays stable even if the
/// enum is reordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedDirection {
    North,
    South,
    West,
    East,
}

impl From<Direction> for SavedDirection {
    fn from(direction: Direction) -> Self {
        match direction {
            Direction::North => SavedDirection::North,
            Direction::South => SavedDirection::South,
            Direction::West => SavedDirection::West,
            Direction::East => SavedDirection::East,
        }
    }
}

impl SavedDirection {
    fn into_direction(self) -> Direction {
        match self {
            SavedDirection::North => Direction::North,
            SavedDirection::South => Direction::South,
            SavedDirection::West => Direction::West,
            SavedDirection::East => Direction::East,
        }
    }
}

/// Comparator-mode encoding used by the redstone metadata sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedComparatorMode {
    Compare,
    Subtract,
}

impl From<ComparatorMode> for SavedComparatorMode {
    fn from(mode: ComparatorMode) -> Self {
        match mode {
            ComparatorMode::Compare => SavedComparatorMode::Compare,
            ComparatorMode::Subtract => SavedComparatorMode::Subtract,
        }
    }
}

impl SavedComparatorMode {
    fn into_comparator_mode(self) -> ComparatorMode {
        match self {
            SavedComparatorMode::Compare => ComparatorMode::Compare,
            SavedComparatorMode::Subtract => ComparatorMode::Subtract,
        }
    }
}

/// Persistent redstone component metadata for a single block inside a chunk.
///
/// `local_x`/`local_y`/`local_z` are chunk-local coordinates (0..16, 0..256,
/// 0..16). Only components whose `ComponentState` differs from the runtime
/// default are serialized, so freshly-placed or never-interacted components
/// round-trip as an empty vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedstoneComponentMetadata {
    pub local_x: u8,
    pub local_y: u8,
    pub local_z: u8,
    pub facing: SavedDirection,
    pub repeater_delay: u8,
    pub comparator_mode: SavedComparatorMode,
    pub note: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScheduledKind {
    ReleaseButton,
    Repeater(bool),
    Explode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScheduledTick {
    due: u64,
    pos: BlockPos,
    kind: ScheduledKind,
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
    pub propagation_overflowed: bool,
}

#[derive(Default)]
pub struct RedstoneSystem {
    components: HashMap<BlockPos, ComponentState>,
    known_chunks: HashSet<(i32, i32)>,
    scheduled: Vec<ScheduledTick>,
    tick: u64,
    dirty: HashSet<BlockPos>,
    sleeping: bool,
}

#[allow(dead_code)]
impl RedstoneSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_sleeping(&self) -> bool {
        self.sleeping
    }

    pub fn current_tick(&self) -> u64 {
        self.tick
    }

    pub fn power_at(&self, pos: BlockPos) -> u8 {
        self.components
            .get(&pos)
            .map(|state| state.signal.power)
            .unwrap_or(0)
    }

    pub fn charge_at(&self, pos: BlockPos) -> ChargeKind {
        self.components
            .get(&pos)
            .map(|state| state.signal.charge)
            .unwrap_or(ChargeKind::Unpowered)
    }

    pub fn block_state_at(&self, manager: &ChunkManager, pos: BlockPos) -> RedstoneState {
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
        }
    }

    pub fn set_comparator_mode(&mut self, pos: BlockPos, mode: ComparatorMode) {
        if let Some(state) = self.components.get_mut(&pos) {
            state.comparator_mode = mode;
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
        _manager: &ChunkManager,
        cx: i32,
        cz: i32,
    ) -> Vec<RedstoneComponentMetadata> {
        let mut metadata = Vec::new();
        let origin_x = cx * CHUNK_WIDTH as i32;
        let origin_z = cz * CHUNK_DEPTH as i32;
        for (&pos, state) in &self.components {
            let local_x = pos.0 - origin_x;
            let local_z = pos.2 - origin_z;
            if local_x < 0
                || local_x >= CHUNK_WIDTH as i32
                || local_z < 0
                || local_z >= CHUNK_DEPTH as i32
            {
                continue;
            }
            if pos.1 < 0 || pos.1 >= CHUNK_HEIGHT as i32 {
                continue;
            }
            // `sync_loaded_chunks` evicts components whose block was replaced,
            // so any entry still present here corresponds to a live component.
            if !state.has_persistent_metadata() {
                continue;
            }
            metadata.push(RedstoneComponentMetadata {
                local_x: local_x as u8,
                local_y: pos.1 as u8,
                local_z: local_z as u8,
                facing: state.facing.into(),
                repeater_delay: state.repeater_delay,
                comparator_mode: state.comparator_mode.into(),
                note: state.note,
            });
        }
        metadata
    }

    /// Reapplies previously persisted component metadata after a chunk has been
    /// (re)loaded and `sync_loaded_chunks` has rebuilt default `ComponentState`
    /// entries. Entries whose block no longer matches a redstone component are
    /// ignored so stale metadata cannot resurrect facings on unrelated blocks.
    pub fn restore_chunk_metadata(
        &mut self,
        manager: &ChunkManager,
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
            state.facing = entry.facing.into_direction();
            state.repeater_delay = entry.repeater_delay.clamp(1, 4);
            state.comparator_mode = entry.comparator_mode.into_comparator_mode();
            state.note = entry.note.min(24);
            self.mark_dirty(pos);
        }
    }

    fn mark_dirty(&mut self, pos: BlockPos) {
        if self.components.contains_key(&pos) {
            self.dirty.insert(pos);
            self.sleeping = false;
        }
    }

    fn mark_neighbors_dirty(&mut self, manager: &ChunkManager, pos: BlockPos) {
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

    pub fn on_block_changed(&mut self, manager: &ChunkManager, pos: BlockPos, facing: Direction) {
        let block = get_block(manager, pos);
        if is_component(block) {
            self.components
                .entry(pos)
                .and_modify(|state| state.facing = facing)
                .or_insert_with(|| ComponentState::new(block, facing));
        } else {
            self.components.remove(&pos);
            self.scheduled.retain(|scheduled| {
                scheduled.pos != pos || scheduled.kind == ScheduledKind::Explode
            });
        }
        self.mark_neighbors_dirty(manager, pos);
    }

    pub fn interact(&mut self, manager: &mut ChunkManager, pos: BlockPos) -> RedstoneUpdate {
        self.sync_loaded_chunks(manager);
        let block = get_block(manager, pos);
        let mut update = RedstoneUpdate::default();
        match block {
            BlockType::Lever => {
                set_block_record(manager, pos, BlockType::LeverOn, &mut update.mutations);
            }
            BlockType::LeverOn => {
                set_block_record(manager, pos, BlockType::Lever, &mut update.mutations);
            }
            BlockType::StoneButton | BlockType::StoneButtonPressed => {
                set_block_record(
                    manager,
                    pos,
                    BlockType::StoneButtonPressed,
                    &mut update.mutations,
                );
                self.scheduled.retain(|scheduled| {
                    !(scheduled.pos == pos && scheduled.kind == ScheduledKind::ReleaseButton)
                });
                self.scheduled.push(ScheduledTick {
                    due: self.tick + 20,
                    pos,
                    kind: ScheduledKind::ReleaseButton,
                });
                self.sleeping = false;
            }
            BlockType::Repeater | BlockType::RepeaterPowered => {
                if let Some(state) = self.components.get_mut(&pos) {
                    state.repeater_delay = state.repeater_delay % 4 + 1;
                }
            }
            BlockType::Comparator | BlockType::ComparatorPowered => {
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

    pub fn tick(&mut self, manager: &mut ChunkManager, occupants: &[BlockPos]) -> RedstoneUpdate {
        self.tick = self.tick.wrapping_add(1);
        self.sync_loaded_chunks(manager);

        if self.sleeping
            && self.dirty.is_empty()
            && self.scheduled.is_empty()
            && occupants.is_empty()
        {
            return RedstoneUpdate::default();
        }

        let mut update = RedstoneUpdate::default();
        self.process_scheduled(manager, &mut update);
        self.update_pressure_plates(manager, occupants, &mut update.mutations);

        let converged = self.settle_power(manager);
        update.propagation_overflowed = !converged;
        self.apply_component_transitions(manager, &mut update);
        self.reconcile_mutations(manager, &update.mutations);

        if self.dirty.is_empty() && self.scheduled.is_empty() {
            self.sleeping = true;
        }

        update
    }

    fn sync_loaded_chunks(&mut self, manager: &ChunkManager) {
        self.known_chunks
            .retain(|chunk_pos| manager.chunks.contains_key(chunk_pos));
        self.components.retain(|pos, _| {
            let cx = pos.0.div_euclid(CHUNK_WIDTH as i32);
            let cz = pos.2.div_euclid(CHUNK_DEPTH as i32);
            manager.chunks.contains_key(&(cx, cz)) && is_component(get_block(manager, *pos))
        });
        self.dirty.retain(|pos| self.components.contains_key(pos));

        for (&(cx, cz), chunk) in &manager.chunks {
            if !self.known_chunks.insert((cx, cz)) {
                continue;
            }
            let origin_x = cx * CHUNK_WIDTH as i32;
            let origin_z = cz * CHUNK_DEPTH as i32;
            for &encoded in chunk.redstone_positions() {
                let (x, y, z) = crate::world::Chunk::decode_torch_position(encoded);
                let pos = (origin_x + x as i32, y as i32, origin_z + z as i32);
                let block = chunk.blocks[x][y][z];
                use std::collections::hash_map::Entry;
                if let Entry::Vacant(e) = self.components.entry(pos) {
                    e.insert(ComponentState::new(block, Direction::North));
                    self.mark_dirty(pos);
                }
            }
        }
    }

    fn reconcile_mutations(&mut self, manager: &ChunkManager, mutations: &[BlockMutation]) {
        for mutation in mutations {
            if is_component(mutation.new_block) {
                self.components
                    .entry(mutation.pos)
                    .or_insert_with(|| ComponentState::new(mutation.new_block, Direction::North));
            } else {
                self.components.remove(&mutation.pos);
                self.scheduled.retain(|scheduled| {
                    scheduled.pos != mutation.pos || scheduled.kind == ScheduledKind::Explode
                });
            }
            self.mark_neighbors_dirty(manager, mutation.pos);
        }
        self.components
            .retain(|pos, _| is_component(get_block(manager, *pos)));
    }

    fn process_scheduled(&mut self, manager: &mut ChunkManager, update: &mut RedstoneUpdate) {
        let (due, future): (Vec<_>, Vec<_>) = std::mem::take(&mut self.scheduled)
            .into_iter()
            .partition(|s| s.due <= self.tick);
        self.scheduled = future;

        for scheduled in due {
            match scheduled.kind {
                ScheduledKind::ReleaseButton => {
                    if get_block(manager, scheduled.pos) == BlockType::StoneButtonPressed {
                        set_block_record(
                            manager,
                            scheduled.pos,
                            BlockType::StoneButton,
                            &mut update.mutations,
                        );
                        self.mark_neighbors_dirty(manager, scheduled.pos);
                    }
                }
                ScheduledKind::Repeater(powered) => {
                    let block = get_block(manager, scheduled.pos);
                    if matches!(block, BlockType::Repeater | BlockType::RepeaterPowered) {
                        let target = if powered {
                            BlockType::RepeaterPowered
                        } else {
                            BlockType::Repeater
                        };
                        set_block_record(manager, scheduled.pos, target, &mut update.mutations);
                        self.mark_neighbors_dirty(manager, scheduled.pos);
                    }
                }
                ScheduledKind::Explode => update
                    .actions
                    .push(RedstoneAction::Explode { pos: scheduled.pos }),
            }
        }
    }

    fn update_pressure_plates(
        &mut self,
        manager: &mut ChunkManager,
        occupants: &[BlockPos],
        mutations: &mut Vec<BlockMutation>,
    ) {
        let plates: Vec<BlockPos> = self
            .components
            .iter()
            .filter_map(|(&pos, _)| {
                matches!(
                    get_block(manager, pos),
                    BlockType::PressurePlate | BlockType::PressurePlatePowered
                )
                .then_some(pos)
            })
            .collect();
        for pos in plates {
            let occupied = occupants.iter().any(|occupant| {
                occupant.0 == pos.0 && occupant.2 == pos.2 && occupant.1 == pos.1 + 1
            });
            let current_block = get_block(manager, pos);
            let target = if occupied {
                BlockType::PressurePlatePowered
            } else {
                BlockType::PressurePlate
            };
            if current_block != target {
                set_block_record(manager, pos, target, mutations);
                self.mark_neighbors_dirty(manager, pos);
            }
        }
    }

    fn settle_power(&mut self, manager: &ChunkManager) -> bool {
        if self.dirty.is_empty() {
            return true;
        }

        let mut current_dirty = std::mem::take(&mut self.dirty);
        let mut next_dirty = HashSet::new();
        let max_evaluations = (self.components.len() * MAX_PROPAGATION_PASSES).max(1024);
        let mut evaluations = 0;

        for _pass in 0..MAX_PROPAGATION_PASSES {
            if current_dirty.is_empty() {
                return true;
            }

            for pos in current_dirty.drain() {
                evaluations += 1;
                if evaluations > max_evaluations {
                    self.dirty.extend(next_dirty);
                    return false;
                }

                let Some(state) = self.components.get(&pos).copied() else {
                    continue;
                };
                let block = get_block(manager, pos);
                let new_power = desired_power(manager, &self.components, pos, block, state);
                let new_charge = if new_power == 0 {
                    ChargeKind::Unpowered
                } else if is_strong_source(block) {
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
            return false;
        }

        true
    }

    fn apply_component_transitions(
        &mut self,
        manager: &mut ChunkManager,
        update: &mut RedstoneUpdate,
    ) {
        let positions: Vec<BlockPos> = self.components.keys().copied().collect();
        for pos in positions {
            let block = get_block(manager, pos);
            let Some(mut state) = self.components.get(&pos).copied() else {
                continue;
            };

            match block {
                BlockType::RedstoneTorch | BlockType::RedstoneTorchOff => {
                    let target = if state.signal.power > 0 {
                        BlockType::RedstoneTorch
                    } else {
                        BlockType::RedstoneTorchOff
                    };
                    set_block_record(manager, pos, target, &mut update.mutations);
                }
                BlockType::Repeater | BlockType::RepeaterPowered => {
                    let behind = sub(pos, state.facing.delta());
                    let input = signal_from_position(manager, &self.components, behind, pos, false);
                    let desired = input > 0;
                    let current = block == BlockType::RepeaterPowered;
                    let already_scheduled = self.scheduled.iter().any(|scheduled| {
                        scheduled.pos == pos && matches!(scheduled.kind, ScheduledKind::Repeater(_))
                    });
                    if desired != current && !already_scheduled {
                        self.scheduled.push(ScheduledTick {
                            due: self.tick + state.repeater_delay as u64,
                            pos,
                            kind: ScheduledKind::Repeater(desired),
                        });
                    }
                }
                BlockType::Comparator | BlockType::ComparatorPowered => {
                    let target = if state.signal.power > 0 {
                        BlockType::ComparatorPowered
                    } else {
                        BlockType::Comparator
                    };
                    set_block_record(manager, pos, target, &mut update.mutations);
                }
                BlockType::RedstoneLamp | BlockType::RedstoneLampLit => {
                    let target = if state.signal.power > 0 {
                        BlockType::RedstoneLampLit
                    } else {
                        BlockType::RedstoneLamp
                    };
                    set_block_record(manager, pos, target, &mut update.mutations);
                }
                BlockType::OakDoor | BlockType::OakDoorOpen => {
                    let target = if state.signal.power > 0 {
                        BlockType::OakDoorOpen
                    } else {
                        BlockType::OakDoor
                    };
                    let is_open = state.signal.power > 0;
                    let cur_raw = manager.get_block_state(pos.0, pos.1, pos.2);
                    let mut bstate = crate::world::BlockState::decode(cur_raw);
                    if bstate.is_open != is_open {
                        bstate.is_open = is_open;
                        set_block_record_with_state(
                            manager,
                            pos,
                            target,
                            bstate.encode(),
                            &mut update.mutations,
                        );
                    } else {
                        set_block_record(manager, pos, target, &mut update.mutations);
                    }
                }
                BlockType::OakTrapdoor | BlockType::OakTrapdoorOpen => {
                    let target = if state.signal.power > 0 {
                        BlockType::OakTrapdoorOpen
                    } else {
                        BlockType::OakTrapdoor
                    };
                    let is_open = state.signal.power > 0;
                    let cur_raw = manager.get_block_state(pos.0, pos.1, pos.2);
                    let mut bstate = crate::world::BlockState::decode(cur_raw);
                    if bstate.is_open != is_open {
                        bstate.is_open = is_open;
                        set_block_record_with_state(
                            manager,
                            pos,
                            target,
                            bstate.encode(),
                            &mut update.mutations,
                        );
                    } else {
                        set_block_record(manager, pos, target, &mut update.mutations);
                    }
                }
                BlockType::Piston
                | BlockType::PistonExtended
                | BlockType::StickyPiston
                | BlockType::StickyPistonExtended => {
                    let powered = state.signal.power > 0;
                    if powered && !state.last_powered {
                        self.extend_piston(
                            manager,
                            pos,
                            state.facing,
                            block,
                            &mut update.mutations,
                        );
                    } else if !powered && state.last_powered {
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
                    self.scheduled.push(ScheduledTick {
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

    fn extend_piston(
        &self,
        manager: &mut ChunkManager,
        pos: BlockPos,
        facing: Direction,
        block: BlockType,
        mutations: &mut Vec<BlockMutation>,
    ) {
        let delta = facing.delta();
        let front = add(pos, delta);
        let destination = add(front, delta);
        let pushed = get_block(manager, front);
        if pushed != BlockType::Air {
            if !is_movable(pushed) || get_block(manager, destination) != BlockType::Air {
                return;
            }
            set_block_record(manager, destination, pushed, mutations);
            set_block_record(manager, front, BlockType::Air, mutations);
        }
        let target = if matches!(
            block,
            BlockType::StickyPiston | BlockType::StickyPistonExtended
        ) {
            BlockType::StickyPistonExtended
        } else {
            BlockType::PistonExtended
        };
        set_block_record(manager, pos, target, mutations);
    }

    fn retract_piston(
        &self,
        manager: &mut ChunkManager,
        pos: BlockPos,
        facing: Direction,
        block: BlockType,
        mutations: &mut Vec<BlockMutation>,
    ) {
        let sticky = matches!(
            block,
            BlockType::StickyPiston | BlockType::StickyPistonExtended
        );
        let delta = facing.delta();
        let front = add(pos, delta);
        if sticky && get_block(manager, front) == BlockType::Air {
            let pulled_from = add(front, delta);
            let pulled = get_block(manager, pulled_from);
            if is_movable(pulled) {
                set_block_record(manager, front, pulled, mutations);
                set_block_record(manager, pulled_from, BlockType::Air, mutations);
            }
        }
        let target = if sticky {
            BlockType::StickyPiston
        } else {
            BlockType::Piston
        };
        set_block_record(manager, pos, target, mutations);
    }
}

fn desired_power(
    manager: &ChunkManager,
    states: &HashMap<BlockPos, ComponentState>,
    pos: BlockPos,
    block: BlockType,
    state: ComponentState,
) -> u8 {
    match block {
        BlockType::LeverOn | BlockType::StoneButtonPressed | BlockType::PressurePlatePowered => 15,
        BlockType::Lever | BlockType::StoneButton | BlockType::PressurePlate => 0,
        BlockType::RedstoneTorch | BlockType::RedstoneTorchOff => {
            let support = add(pos, (0, -1, 0));
            if strong_power_into(manager, states, support) > 0 {
                0
            } else {
                15
            }
        }
        BlockType::RedstoneWire => incoming_power(manager, states, pos, true),
        BlockType::RepeaterPowered => 15,
        BlockType::Repeater => 0,
        BlockType::Comparator | BlockType::ComparatorPowered => {
            let rear = sub(pos, state.facing.delta());
            let rear_power = signal_from_position(manager, states, rear, pos, false);
            let left = add(pos, state.facing.left().delta());
            let right = add(pos, state.facing.right().delta());
            let side_power = signal_from_position(manager, states, left, pos, false)
                .max(signal_from_position(manager, states, right, pos, false));
            match state.comparator_mode {
                ComparatorMode::Compare => {
                    if rear_power >= side_power {
                        rear_power
                    } else {
                        0
                    }
                }
                ComparatorMode::Subtract => rear_power.saturating_sub(side_power),
            }
        }
        BlockType::RedstoneLamp
        | BlockType::RedstoneLampLit
        | BlockType::OakDoor
        | BlockType::OakDoorOpen
        | BlockType::OakTrapdoor
        | BlockType::OakTrapdoorOpen
        | BlockType::Piston
        | BlockType::PistonExtended
        | BlockType::StickyPiston
        | BlockType::StickyPistonExtended
        | BlockType::TNT
        | BlockType::Dispenser
        | BlockType::Dropper
        | BlockType::NoteBlock => incoming_power(manager, states, pos, false),
        _ => source_power(block),
    }
}

fn incoming_power(
    manager: &ChunkManager,
    states: &HashMap<BlockPos, ComponentState>,
    target: BlockPos,
    attenuate_wire: bool,
) -> u8 {
    NEIGHBORS
        .iter()
        .map(|offset| {
            let source = add(target, *offset);
            signal_from_position(manager, states, source, target, attenuate_wire)
        })
        .max()
        .unwrap_or(0)
}

fn signal_from_position(
    manager: &ChunkManager,
    states: &HashMap<BlockPos, ComponentState>,
    source: BlockPos,
    target: BlockPos,
    attenuate_wire: bool,
) -> u8 {
    let block = get_block(manager, source);
    if let Some(state) = states.get(&source) {
        let mut power = emitted_toward(source, target, block, *state);
        if attenuate_wire && block == BlockType::RedstoneWire {
            power = power.saturating_sub(1);
        }
        return power;
    }
    if block.properties().is_solid {
        return strong_power_into(manager, states, source);
    }
    0
}

fn emitted_toward(
    source: BlockPos,
    target: BlockPos,
    block: BlockType,
    state: ComponentState,
) -> u8 {
    match block {
        BlockType::RepeaterPowered | BlockType::ComparatorPowered => {
            (add(source, state.facing.delta()) == target)
                .then_some(state.signal.power)
                .unwrap_or(0)
        }
        BlockType::Repeater | BlockType::Comparator => 0,
        BlockType::RedstoneLamp
        | BlockType::RedstoneLampLit
        | BlockType::OakDoor
        | BlockType::OakDoorOpen
        | BlockType::OakTrapdoor
        | BlockType::OakTrapdoorOpen
        | BlockType::Piston
        | BlockType::PistonExtended
        | BlockType::StickyPiston
        | BlockType::StickyPistonExtended
        | BlockType::TNT
        | BlockType::Dispenser
        | BlockType::Dropper
        | BlockType::NoteBlock => 0,
        _ => state.signal.power,
    }
}

fn strong_power_into(
    manager: &ChunkManager,
    states: &HashMap<BlockPos, ComponentState>,
    target: BlockPos,
) -> u8 {
    NEIGHBORS
        .iter()
        .filter_map(|offset| {
            let source = add(target, *offset);
            let state = states.get(&source)?;
            let block = get_block(manager, source);
            is_strong_source(block).then_some(emitted_toward(source, target, block, *state))
        })
        .max()
        .unwrap_or(0)
}

fn source_power(block: BlockType) -> u8 {
    match block {
        BlockType::LeverOn
        | BlockType::StoneButtonPressed
        | BlockType::PressurePlatePowered
        | BlockType::RedstoneTorch
        | BlockType::RepeaterPowered => 15,
        BlockType::ComparatorPowered => 1,
        _ => 0,
    }
}

fn is_strong_source(block: BlockType) -> bool {
    matches!(
        block,
        BlockType::LeverOn
            | BlockType::StoneButtonPressed
            | BlockType::PressurePlatePowered
            | BlockType::RedstoneTorch
            | BlockType::RepeaterPowered
            | BlockType::ComparatorPowered
    )
}

pub(crate) fn is_component(block: BlockType) -> bool {
    matches!(
        block,
        BlockType::RedstoneWire
            | BlockType::RedstoneTorch
            | BlockType::RedstoneTorchOff
            | BlockType::Repeater
            | BlockType::RepeaterPowered
            | BlockType::Comparator
            | BlockType::ComparatorPowered
            | BlockType::StoneButton
            | BlockType::StoneButtonPressed
            | BlockType::Lever
            | BlockType::LeverOn
            | BlockType::PressurePlate
            | BlockType::PressurePlatePowered
            | BlockType::Piston
            | BlockType::PistonExtended
            | BlockType::StickyPiston
            | BlockType::StickyPistonExtended
            | BlockType::RedstoneLamp
            | BlockType::RedstoneLampLit
            | BlockType::OakDoor
            | BlockType::OakDoorOpen
            | BlockType::OakTrapdoor
            | BlockType::OakTrapdoorOpen
            | BlockType::TNT
            | BlockType::Dispenser
            | BlockType::Dropper
            | BlockType::NoteBlock
    )
}

fn is_movable(block: BlockType) -> bool {
    block != BlockType::Air
        && block != BlockType::Bedrock
        && !matches!(
            block,
            BlockType::Piston
                | BlockType::PistonExtended
                | BlockType::StickyPiston
                | BlockType::StickyPistonExtended
        )
}

fn get_block(manager: &ChunkManager, pos: BlockPos) -> BlockType {
    manager.get_block(pos.0, pos.1, pos.2)
}

fn set_block_record(
    manager: &mut ChunkManager,
    pos: BlockPos,
    block: BlockType,
    mutations: &mut Vec<BlockMutation>,
) {
    let old_block = get_block(manager, pos);
    if old_block == block {
        return;
    }
    manager.set_block(pos.0, pos.1, pos.2, block);
    if get_block(manager, pos) == block {
        mutations.push(BlockMutation {
            pos,
            old_block,
            new_block: block,
        });
    }
}

fn set_block_record_with_state(
    manager: &mut ChunkManager,
    pos: BlockPos,
    block: BlockType,
    state: u8,
    mutations: &mut Vec<BlockMutation>,
) {
    let old_block = get_block(manager, pos);
    let old_state = manager.get_block_state(pos.0, pos.1, pos.2);
    if old_block == block && old_state == state {
        return;
    }
    manager.set_block(pos.0, pos.1, pos.2, block);
    manager.set_block_state(pos.0, pos.1, pos.2, state);
    mutations.push(BlockMutation {
        pos,
        old_block,
        new_block: block,
    });
}

fn add(a: BlockPos, b: BlockPos) -> BlockPos {
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}

fn sub(a: BlockPos, b: BlockPos) -> BlockPos {
    (a.0 - b.0, a.1 - b.1, a.2 - b.2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;

    const Y: i32 = 200;

    fn manager() -> ChunkManager {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager
    }

    fn place(
        system: &mut RedstoneSystem,
        manager: &mut ChunkManager,
        x: i32,
        block: BlockType,
        facing: Direction,
    ) {
        manager.set_block(x, Y, 0, block);
        system.on_block_changed(manager, (x, Y, 0), facing);
    }

    #[test]
    fn dust_propagates_and_loses_one_level_per_block() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();
        place(
            &mut system,
            &mut manager,
            0,
            BlockType::Lever,
            Direction::East,
        );
        place(
            &mut system,
            &mut manager,
            1,
            BlockType::RedstoneWire,
            Direction::East,
        );
        place(
            &mut system,
            &mut manager,
            2,
            BlockType::RedstoneWire,
            Direction::East,
        );
        place(
            &mut system,
            &mut manager,
            3,
            BlockType::RedstoneLamp,
            Direction::East,
        );

        system.interact(&mut manager, (0, Y, 0));
        system.tick(&mut manager, &[]);

        assert_eq!(system.power_at((1, Y, 0)), 15);
        assert_eq!(system.power_at((2, Y, 0)), 14);
        assert_eq!(manager.get_block(3, Y, 0), BlockType::RedstoneLampLit);
    }

    #[test]
    fn repeater_applies_configured_tick_delay_and_restores_full_power() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();
        place(
            &mut system,
            &mut manager,
            0,
            BlockType::Lever,
            Direction::East,
        );
        place(
            &mut system,
            &mut manager,
            1,
            BlockType::Repeater,
            Direction::East,
        );
        place(
            &mut system,
            &mut manager,
            2,
            BlockType::RedstoneLamp,
            Direction::East,
        );
        system.set_repeater_delay((1, Y, 0), 4);
        system.interact(&mut manager, (0, Y, 0));

        for _ in 0..4 {
            system.tick(&mut manager, &[]);
            assert_eq!(manager.get_block(2, Y, 0), BlockType::RedstoneLamp);
        }
        system.tick(&mut manager, &[]);
        assert_eq!(system.power_at((1, Y, 0)), 15);
        assert_eq!(manager.get_block(2, Y, 0), BlockType::RedstoneLampLit);
    }

    #[test]
    fn piston_pushes_one_movable_block() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();
        place(
            &mut system,
            &mut manager,
            0,
            BlockType::Lever,
            Direction::East,
        );
        place(
            &mut system,
            &mut manager,
            1,
            BlockType::RedstoneWire,
            Direction::East,
        );
        place(
            &mut system,
            &mut manager,
            2,
            BlockType::Piston,
            Direction::East,
        );
        manager.set_block(3, Y, 0, BlockType::Stone);
        manager.set_block(4, Y, 0, BlockType::Air);

        system.interact(&mut manager, (0, Y, 0));
        system.tick(&mut manager, &[]);

        assert_eq!(manager.get_block(2, Y, 0), BlockType::PistonExtended);
        assert_eq!(manager.get_block(3, Y, 0), BlockType::Air);
        assert_eq!(manager.get_block(4, Y, 0), BlockType::Stone);
    }

    #[test]
    fn door_and_trapdoor_redstone_toggle_preserves_facing_and_updates_open_bit() {
        use crate::world::BlockState;

        let mut system = RedstoneSystem::new();
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));

        let initial_state = BlockState {
            facing: Direction::West,
            is_top: false,
            is_right_hinge: true,
            is_open: false,
        };

        place(
            &mut system,
            &mut manager,
            0,
            BlockType::Lever,
            Direction::South,
        );
        place(
            &mut system,
            &mut manager,
            1,
            BlockType::RedstoneWire,
            Direction::East,
        );

        manager.set_block(2, Y, 0, BlockType::OakDoor);
        manager.set_block_state(2, Y, 0, initial_state.encode());
        system.on_block_changed(&manager, (2, Y, 0), Direction::West);

        system.interact(&mut manager, (0, Y, 0));
        system.tick(&mut manager, &[]);

        assert_eq!(manager.get_block(2, Y, 0), BlockType::OakDoorOpen);
        let toggled_raw = manager.get_block_state(2, Y, 0);
        let toggled_state = BlockState::decode(toggled_raw);
        assert_eq!(toggled_state.facing, Direction::West);
        assert!(toggled_state.is_right_hinge);
        assert!(toggled_state.is_open);
    }

    #[test]
    fn pressure_plate_opens_and_closes_an_adjacent_door() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();
        place(
            &mut system,
            &mut manager,
            0,
            BlockType::PressurePlate,
            Direction::East,
        );
        place(
            &mut system,
            &mut manager,
            1,
            BlockType::OakDoor,
            Direction::East,
        );

        system.tick(&mut manager, &[(0, Y + 1, 0)]);
        assert_eq!(manager.get_block(0, Y, 0), BlockType::PressurePlatePowered);
        assert_eq!(manager.get_block(1, Y, 0), BlockType::OakDoorOpen);

        system.tick(&mut manager, &[]);
        assert_eq!(manager.get_block(0, Y, 0), BlockType::PressurePlate);
        assert_eq!(manager.get_block(1, Y, 0), BlockType::OakDoor);
    }

    #[test]
    fn comparator_subtract_mode_uses_side_input() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();
        place(
            &mut system,
            &mut manager,
            0,
            BlockType::LeverOn,
            Direction::East,
        );
        place(
            &mut system,
            &mut manager,
            1,
            BlockType::Comparator,
            Direction::East,
        );
        manager.set_block(1, Y, 1, BlockType::LeverOn);
        system.on_block_changed(&manager, (1, Y, 1), Direction::North);
        system.set_comparator_mode((1, Y, 0), ComparatorMode::Subtract);

        system.tick(&mut manager, &[]);
        assert_eq!(system.power_at((1, Y, 0)), 0);
        assert_eq!(manager.get_block(1, Y, 0), BlockType::Comparator);
    }

    #[test]
    fn direct_sources_strongly_charge_solid_blocks() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();
        place(
            &mut system,
            &mut manager,
            0,
            BlockType::Lever,
            Direction::East,
        );
        manager.set_block(1, Y, 0, BlockType::Stone);
        place(
            &mut system,
            &mut manager,
            2,
            BlockType::RedstoneWire,
            Direction::East,
        );
        system.interact(&mut manager, (0, Y, 0));
        system.tick(&mut manager, &[]);

        assert_eq!(
            system.block_state_at(&manager, (1, Y, 0)),
            RedstoneState {
                power: 15,
                charge: ChargeKind::Strong,
            }
        );
        assert_eq!(system.power_at((2, Y, 0)), 15);
    }

    #[test]
    fn powered_tnt_keeps_its_fuse_after_the_block_is_removed() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();
        place(
            &mut system,
            &mut manager,
            0,
            BlockType::Lever,
            Direction::East,
        );
        place(
            &mut system,
            &mut manager,
            1,
            BlockType::TNT,
            Direction::East,
        );
        system.interact(&mut manager, (0, Y, 0));

        let first = system.tick(&mut manager, &[]);
        assert_eq!(manager.get_block(1, Y, 0), BlockType::Air);
        assert!(first.actions.is_empty());
        for _ in 0..79 {
            assert!(system.tick(&mut manager, &[]).actions.is_empty());
        }
        let fired = system.tick(&mut manager, &[]);
        assert_eq!(
            fired.actions,
            vec![RedstoneAction::Explode { pos: (1, Y, 0) }]
        );
    }

    #[test]
    fn collect_and_restore_preserves_repeater_delay_comparator_mode_and_note() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();
        // Repeater with a non-default delay (4) and a non-North facing.
        place(
            &mut system,
            &mut manager,
            1,
            BlockType::Repeater,
            Direction::East,
        );
        system.set_repeater_delay((1, Y, 0), 4);
        // Comparator in Subtract mode.
        place(
            &mut system,
            &mut manager,
            2,
            BlockType::Comparator,
            Direction::South,
        );
        system.set_comparator_mode((2, Y, 0), ComparatorMode::Subtract);
        // NoteBlock tuned to pitch 12.
        place(
            &mut system,
            &mut manager,
            3,
            BlockType::NoteBlock,
            Direction::West,
        );
        for _ in 0..12 {
            system.interact(&mut manager, (3, Y, 0));
        }

        let metadata = system.collect_chunk_metadata(&manager, 0, 0);
        assert_eq!(metadata.len(), 3);

        // Simulate the unload+reload path: drop the in-memory component state
        // and let `sync_loaded_chunks` rebuild default entries from the blocks.
        let mut reloaded = RedstoneSystem::new();
        reloaded.tick(&mut manager, &[]);
        // Defaults restored by `sync_loaded_chunks` must differ from the saved
        // values before we apply the sidecar.
        assert_eq!(reloaded.repeater_delay((1, Y, 0)), Some(1));
        assert_eq!(
            reloaded.comparator_mode((2, Y, 0)),
            Some(ComparatorMode::Compare)
        );

        reloaded.restore_chunk_metadata(&manager, 0, 0, &metadata);
        assert_eq!(reloaded.repeater_delay((1, Y, 0)), Some(4));
        assert_eq!(
            reloaded.comparator_mode((2, Y, 0)),
            Some(ComparatorMode::Subtract)
        );
        assert_eq!(
            reloaded.components.get(&(3, Y, 0)).map(|s| s.note),
            Some(12)
        );
        assert_eq!(
            reloaded.components.get(&(1, Y, 0)).map(|s| s.facing),
            Some(Direction::East)
        );
        assert_eq!(
            reloaded.components.get(&(2, Y, 0)).map(|s| s.facing),
            Some(Direction::South)
        );
        assert_eq!(
            reloaded.components.get(&(3, Y, 0)).map(|s| s.facing),
            Some(Direction::West)
        );
    }

    #[test]
    fn collect_skips_components_with_default_metadata() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();
        // A freshly-placed Repeater with default delay 1 and North facing
        // carries no persistent metadata and must not appear in the sidecar.
        place(
            &mut system,
            &mut manager,
            1,
            BlockType::Repeater,
            Direction::North,
        );
        // A lever is a component but only carries the default state.
        place(
            &mut system,
            &mut manager,
            2,
            BlockType::Lever,
            Direction::North,
        );

        let metadata = system.collect_chunk_metadata(&manager, 0, 0);
        assert!(metadata.is_empty());
    }

    #[test]
    fn collect_only_emits_components_inside_the_target_chunk() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.chunks.insert((1, 0), Chunk::new(1, 0));
        let mut system = RedstoneSystem::new();
        // First component inside chunk (0, 0).
        manager.set_block(1, Y, 0, BlockType::Repeater);
        system.on_block_changed(&mut manager, (1, Y, 0), Direction::East);
        system.set_repeater_delay((1, Y, 0), 3);
        // Second component inside chunk (1, 0) (x = 16..31).
        manager.set_block(17, Y, 0, BlockType::Repeater);
        system.on_block_changed(&mut manager, (17, Y, 0), Direction::East);
        system.set_repeater_delay((17, Y, 0), 2);

        let metadata_chunk_0 = system.collect_chunk_metadata(&manager, 0, 0);
        assert_eq!(metadata_chunk_0.len(), 1);
        assert_eq!(metadata_chunk_0[0].local_x, 1);
        assert_eq!(metadata_chunk_0[0].repeater_delay, 3);

        let metadata_chunk_1 = system.collect_chunk_metadata(&manager, 1, 0);
        assert_eq!(metadata_chunk_1.len(), 1);
        assert_eq!(metadata_chunk_1[0].local_x, 1);
        assert_eq!(metadata_chunk_1[0].repeater_delay, 2);
    }

    #[test]
    fn restore_ignores_entries_whose_block_is_no_longer_a_component() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();
        manager.set_block(1, Y, 0, BlockType::Repeater);
        system.on_block_changed(&mut manager, (1, Y, 0), Direction::East);
        let metadata = system.collect_chunk_metadata(&manager, 0, 0);
        assert_eq!(metadata.len(), 1);

        // Simulate the block being replaced with Stone before reload. The
        // stale sidecar entry must not resurrect a facing on a non-component.
        manager.set_block(1, Y, 0, BlockType::Stone);
        let mut reloaded = RedstoneSystem::new();
        reloaded.tick(&mut manager, &[]);
        reloaded.restore_chunk_metadata(&manager, 0, 0, &metadata);
        assert!(reloaded.components.get(&(1, Y, 0)).is_none());
    }

    #[test]
    fn cross_chunk_redstone_line_propagation() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.chunks.insert((1, 0), Chunk::new(1, 0));
        let mut system = RedstoneSystem::new();

        // Place Lever at x=14 (chunk 0) and RedstoneWires across x=15 (chunk 0) to x=20 (chunk 1)
        manager.set_block(14, Y, 0, BlockType::Lever);
        system.on_block_changed(&manager, (14, Y, 0), Direction::North);

        for x in 15..=20 {
            manager.set_block(x, Y, 0, BlockType::RedstoneWire);
            system.on_block_changed(&manager, (x, Y, 0), Direction::North);
        }

        // Toggle Lever ON
        system.interact(&mut manager, (14, Y, 0));
        system.tick(&mut manager, &[]);

        assert_eq!(system.power_at((14, Y, 0)), 15);
        assert_eq!(system.power_at((15, Y, 0)), 15); // Wire adjacent to Lever gets full 15 power
        assert_eq!(system.power_at((16, Y, 0)), 14); // In chunk 1 (attenuated by 1)
        assert_eq!(system.power_at((17, Y, 0)), 13);
        assert_eq!(system.power_at((20, Y, 0)), 10);
    }

    #[test]
    fn sleeping_mechanism_behavior() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();

        place(
            &mut system,
            &mut manager,
            0,
            BlockType::Lever,
            Direction::North,
        );
        place(
            &mut system,
            &mut manager,
            1,
            BlockType::RedstoneWire,
            Direction::North,
        );

        // Initial tick settles power and enters sleeping state
        system.tick(&mut manager, &[]);
        assert!(system.is_sleeping());

        // Subsequent tick when idle stays sleeping
        let idle_update = system.tick(&mut manager, &[]);
        assert!(idle_update.mutations.is_empty());
        assert!(system.is_sleeping());

        // Interaction wakes system up
        system.interact(&mut manager, (0, Y, 0));
        assert!(!system.is_sleeping());

        // Tick settles again and returns to sleeping
        system.tick(&mut manager, &[]);
        assert!(system.is_sleeping());
        assert_eq!(system.power_at((0, Y, 0)), 15);
        assert_eq!(system.power_at((1, Y, 0)), 15);
    }

    fn reference_full_settle(
        components: &mut HashMap<BlockPos, ComponentState>,
        manager: &ChunkManager,
    ) -> bool {
        for _ in 0..MAX_PROPAGATION_PASSES {
            let snapshot = components.clone();
            let mut changed = false;
            for (&pos, state) in components.iter_mut() {
                let block = get_block(manager, pos);
                let new_power = desired_power(manager, &snapshot, pos, block, *state);
                let new_charge = if new_power == 0 {
                    ChargeKind::Unpowered
                } else if is_strong_source(block) {
                    ChargeKind::Strong
                } else {
                    ChargeKind::Weak
                };
                if state.signal.power != new_power || state.signal.charge != new_charge {
                    state.signal.power = new_power;
                    state.signal.charge = new_charge;
                    changed = true;
                }
            }
            if !changed {
                return true;
            }
        }
        false
    }

    #[test]
    fn differential_dirty_worklist_vs_full_settle_parity() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.chunks.insert((1, 0), Chunk::new(1, 0));
        let mut system = RedstoneSystem::new();

        // Build circuit spanning multiple chunks:
        // Lever at 0, wires 1..10, repeater at 11, wires 12..20
        manager.set_block(0, Y, 0, BlockType::Lever);
        system.on_block_changed(&manager, (0, Y, 0), Direction::East);

        for x in 1..=10 {
            manager.set_block(x, Y, 0, BlockType::RedstoneWire);
            system.on_block_changed(&manager, (x, Y, 0), Direction::North);
        }
        manager.set_block(11, Y, 0, BlockType::Repeater);
        system.on_block_changed(&manager, (11, Y, 0), Direction::East);

        for x in 12..=20 {
            manager.set_block(x, Y, 0, BlockType::RedstoneWire);
            system.on_block_changed(&manager, (x, Y, 0), Direction::North);
        }

        // Action 1: Toggle lever ON and tick
        system.interact(&mut manager, (0, Y, 0));
        system.tick(&mut manager, &[]);

        // Check parity against full settle reference
        let mut ref_components = system.components.clone();
        reference_full_settle(&mut ref_components, &manager);
        for (pos, state) in &system.components {
            let ref_state = ref_components.get(pos).unwrap();
            assert_eq!(
                state.signal.power, ref_state.signal.power,
                "Mismatch at pos {:?}",
                pos
            );
            assert_eq!(
                state.signal.charge, ref_state.signal.charge,
                "Mismatch at pos {:?}",
                pos
            );
        }

        // Action 2: Advance ticks for repeater propagation
        for _ in 0..5 {
            system.tick(&mut manager, &[]);
            let mut ref_comp = system.components.clone();
            reference_full_settle(&mut ref_comp, &manager);
            for (pos, state) in &system.components {
                let ref_state = ref_comp.get(pos).unwrap();
                assert_eq!(
                    state.signal.power, ref_state.signal.power,
                    "Mismatch post-repeater at pos {:?}",
                    pos
                );
                assert_eq!(
                    state.signal.charge, ref_state.signal.charge,
                    "Mismatch post-repeater at pos {:?}",
                    pos
                );
            }
        }
    }

    #[test]
    fn loop_budget_parity_test() {
        let mut manager = manager();
        let mut system = RedstoneSystem::new();

        // Create a feedback loop of wires
        let positions = [(0, Y, 0), (1, Y, 0), (1, Y, 1), (0, Y, 1)];
        for &pos in &positions {
            manager.set_block(pos.0, pos.1, pos.2, BlockType::RedstoneWire);
            system.on_block_changed(&manager, pos, Direction::North);
        }

        system.tick(&mut manager, &[]);
        assert!(!system.tick(&mut manager, &[]).propagation_overflowed);
    }
}
