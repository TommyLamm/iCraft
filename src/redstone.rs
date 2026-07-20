use crate::chunk_manager::ChunkManager;
use crate::world::{BlockType, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
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
}

#[allow(dead_code)]
impl RedstoneSystem {
    pub fn new() -> Self {
        Self::default()
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
    }

    pub fn interact(&mut self, manager: &mut ChunkManager, pos: BlockPos) -> RedstoneUpdate {
        self.sync_loaded_chunks(manager);
        let block = get_block(manager, pos);
        let mut update = RedstoneUpdate::default();
        match block {
            BlockType::Lever => {
                set_block_record(manager, pos, BlockType::LeverOn, &mut update.mutations)
            }
            BlockType::LeverOn => {
                set_block_record(manager, pos, BlockType::Lever, &mut update.mutations)
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
        self.reconcile_mutations(manager, &update.mutations);
        update
    }

    pub fn tick(&mut self, manager: &mut ChunkManager, occupants: &[BlockPos]) -> RedstoneUpdate {
        self.tick = self.tick.wrapping_add(1);
        self.sync_loaded_chunks(manager);
        let mut update = RedstoneUpdate::default();
        self.process_scheduled(manager, &mut update);
        self.update_pressure_plates(manager, occupants, &mut update.mutations);

        let converged = self.settle_power(manager);
        update.propagation_overflowed = !converged;
        self.apply_component_transitions(manager, &mut update);
        self.reconcile_mutations(manager, &update.mutations);
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

        for (&(cx, cz), chunk) in &manager.chunks {
            if !self.known_chunks.insert((cx, cz)) {
                continue;
            }
            for x in 0..CHUNK_WIDTH {
                for y in 0..CHUNK_HEIGHT {
                    for z in 0..CHUNK_DEPTH {
                        let block = chunk.blocks[x][y][z];
                        if is_component(block) {
                            let pos = (
                                cx * CHUNK_WIDTH as i32 + x as i32,
                                y as i32,
                                cz * CHUNK_DEPTH as i32 + z as i32,
                            );
                            self.components
                                .entry(pos)
                                .or_insert_with(|| ComponentState::new(block, Direction::North));
                        }
                    }
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
        }
        self.components
            .retain(|pos, _| is_component(get_block(manager, *pos)));
    }

    fn process_scheduled(&mut self, manager: &mut ChunkManager, update: &mut RedstoneUpdate) {
        let mut future = Vec::with_capacity(self.scheduled.len());
        for scheduled in self.scheduled.drain(..) {
            if scheduled.due > self.tick {
                future.push(scheduled);
                continue;
            }
            match scheduled.kind {
                ScheduledKind::ReleaseButton => {
                    if get_block(manager, scheduled.pos) == BlockType::StoneButtonPressed {
                        set_block_record(
                            manager,
                            scheduled.pos,
                            BlockType::StoneButton,
                            &mut update.mutations,
                        );
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
                    }
                }
                ScheduledKind::Explode => update
                    .actions
                    .push(RedstoneAction::Explode { pos: scheduled.pos }),
            }
        }
        self.scheduled = future;
    }

    fn update_pressure_plates(
        &self,
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
            let target = if occupied {
                BlockType::PressurePlatePowered
            } else {
                BlockType::PressurePlate
            };
            set_block_record(manager, pos, target, mutations);
        }
    }

    fn settle_power(&mut self, manager: &ChunkManager) -> bool {
        for _ in 0..MAX_PROPAGATION_PASSES {
            let snapshot = self.components.clone();
            let mut changed = false;
            for (&pos, state) in &mut self.components {
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
                    set_block_record(manager, pos, target, &mut update.mutations);
                }
                BlockType::OakTrapdoor | BlockType::OakTrapdoorOpen => {
                    let target = if state.signal.power > 0 {
                        BlockType::OakTrapdoorOpen
                    } else {
                        BlockType::OakTrapdoor
                    };
                    set_block_record(manager, pos, target, &mut update.mutations);
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

fn is_component(block: BlockType) -> bool {
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
}
