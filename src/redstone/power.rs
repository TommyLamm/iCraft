use super::*;

pub(super) fn desired_power(
    manager: &WorldColumns,
    states: &HashMap<BlockPos, ComponentState>,
    pos: BlockPos,
    block: BlockType,
    state: ComponentState,
) -> u8 {
    let open = block_open_at(manager, pos);
    match block {
        BlockType::Lever | BlockType::StoneButton | BlockType::PressurePlate => {
            if open {
                15
            } else {
                0
            }
        }
        BlockType::RedstoneTorch => {
            let support = add(pos, (0, -1, 0));
            if strong_power_into(manager, states, support) > 0 {
                0
            } else {
                15
            }
        }
        BlockType::RedstoneWire => incoming_power(manager, states, pos, true),
        BlockType::Repeater => {
            if open {
                15
            } else {
                0
            }
        }
        BlockType::Comparator => {
            let rear = sub(pos, state.facing.delta());
            let mut rear_power = signal_from_position(manager, states, rear, pos, false);
            let container_signal =
                crate::block_entity::calculate_container_comparator_signal(manager, rear);
            if container_signal > 0 {
                rear_power = rear_power.max(container_signal);
            } else if get_block(manager, rear).properties().is_solid {
                let rear_behind = sub(rear, state.facing.delta());
                let behind_signal = crate::block_entity::calculate_container_comparator_signal(
                    manager,
                    rear_behind,
                );
                rear_power = rear_power.max(behind_signal);
            }
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
        BlockType::Observer => state.signal.power,
        BlockType::RedstoneLamp
        | BlockType::OakDoor
        | BlockType::OakTrapdoor
        | BlockType::Piston
        | BlockType::StickyPiston
        | BlockType::TNT
        | BlockType::Dispenser
        | BlockType::Dropper
        | BlockType::NoteBlock => incoming_power(manager, states, pos, false),
        _ => source_power(manager, pos, block),
    }
}

pub(super) fn incoming_power(
    manager: &WorldColumns,
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

pub(super) fn signal_from_position(
    manager: &WorldColumns,
    states: &HashMap<BlockPos, ComponentState>,
    source: BlockPos,
    target: BlockPos,
    attenuate_wire: bool,
) -> u8 {
    let block = get_block(manager, source);
    if let Some(state) = states.get(&source) {
        let mut power = emitted_toward(
            source,
            target,
            block,
            *state,
            block_open_at(manager, source),
        );
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

pub(super) fn emitted_toward(
    source: BlockPos,
    target: BlockPos,
    block: BlockType,
    state: ComponentState,
    open: bool,
) -> u8 {
    match block {
        BlockType::Repeater | BlockType::Comparator => {
            if open && add(source, state.facing.delta()) == target {
                state.signal.power
            } else {
                0
            }
        }
        BlockType::Observer => (add(source, state.facing.opposite().delta()) == target)
            .then_some(state.signal.power)
            .unwrap_or(0),
        BlockType::RedstoneLamp
        | BlockType::OakDoor
        | BlockType::OakTrapdoor
        | BlockType::Piston
        | BlockType::StickyPiston
        | BlockType::TNT
        | BlockType::Dispenser
        | BlockType::Dropper
        | BlockType::NoteBlock => 0,
        _ => state.signal.power,
    }
}

pub(super) fn strong_power_into(
    manager: &WorldColumns,
    states: &HashMap<BlockPos, ComponentState>,
    target: BlockPos,
) -> u8 {
    NEIGHBORS
        .iter()
        .filter_map(|offset| {
            let source = add(target, *offset);
            let state = states.get(&source)?;
            let block = get_block(manager, source);
            let open = block_open_at(manager, source);
            is_strong_source(manager, source, block)
                .then_some(emitted_toward(source, target, block, *state, open))
        })
        .max()
        .unwrap_or(0)
}

pub(super) fn source_power(manager: &WorldColumns, pos: BlockPos, block: BlockType) -> u8 {
    let open = block_open_at(manager, pos);
    match block {
        BlockType::RedstoneTorch if !open => 15,
        BlockType::Comparator if open => 1,
        BlockType::Lever | BlockType::StoneButton | BlockType::PressurePlate | BlockType::Repeater
            if open =>
        {
            15
        }
        _ => 0,
    }
}

pub(super) fn append_len(bytes: &mut Vec<u8>, len: usize) {
    bytes.extend_from_slice(&(len as u64).to_le_bytes());
}

pub(super) fn append_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn append_pos(bytes: &mut Vec<u8>, pos: BlockPos) {
    append_i32(bytes, pos.0);
    append_i32(bytes, pos.1);
    append_i32(bytes, pos.2);
}

pub(super) fn scheduled_kind_key(kind: ScheduledKind) -> u8 {
    match kind {
        ScheduledKind::ReleaseButton => 0,
        ScheduledKind::Repeater(false) => 1,
        ScheduledKind::Repeater(true) => 2,
        ScheduledKind::Explode => 3,
        ScheduledKind::ObserverPulseOn => 4,
        ScheduledKind::ObserverPulseOff => 5,
    }
}

pub(super) fn container_revision(manager: &WorldColumns, pos: BlockPos) -> u64 {
    let own = manager
        .get_block_entity(pos.0, pos.1, pos.2)
        .map(crate::block_entity::BlockEntity::revision)
        .unwrap_or(0);
    let partner = crate::block_entity::double_chest_partner(manager, pos)
        .and_then(|partner| manager.get_block_entity(partner.0, partner.1, partner.2))
        .map(crate::block_entity::BlockEntity::revision)
        .unwrap_or(0);
    own.rotate_left(17) ^ partner.rotate_right(11)
}

pub(super) fn record_block_entity_change(
    changes: &mut Vec<(BlockPos, crate::block_entity::BlockEntity)>,
    pos: BlockPos,
    entity: crate::block_entity::BlockEntity,
) {
    if let Some((_, existing)) = changes
        .iter_mut()
        .find(|(existing_pos, _)| *existing_pos == pos)
    {
        *existing = entity;
        return;
    }
    if changes.len() < MAX_REDSTONE_ENTITY_CHANGES_PER_UPDATE {
        changes.push((pos, entity));
    }
}

pub(super) fn encode_charge(charge: ChargeKind) -> u8 {
    match charge {
        ChargeKind::Unpowered => 0,
        ChargeKind::Weak => 1,
        ChargeKind::Strong => 2,
    }
}

pub(super) fn encode_direction(direction: Direction) -> u8 {
    match direction {
        Direction::North => 0,
        Direction::South => 1,
        Direction::West => 2,
        Direction::East => 3,
        Direction::Up => 4,
        Direction::Down => 5,
    }
}

pub(super) fn encode_comparator_mode(mode: ComparatorMode) -> u8 {
    match mode {
        ComparatorMode::Compare => 0,
        ComparatorMode::Subtract => 1,
    }
}

pub(super) fn fnv1a(data: &[u8]) -> u64 {
    crate::rng::fnv1a(data)
}

pub(super) fn is_strong_source(manager: &WorldColumns, pos: BlockPos, block: BlockType) -> bool {
    let open = block_open_at(manager, pos);
    match block {
        BlockType::Lever | BlockType::StoneButton | BlockType::PressurePlate | BlockType::Repeater => {
            open
        }
        BlockType::RedstoneTorch => !open,
        BlockType::Comparator => open,
        _ => false,
    }
}

pub fn is_component(block: BlockType) -> bool {
    matches!(
        block,
        BlockType::RedstoneWire
            | BlockType::RedstoneTorch
            | BlockType::Repeater
            | BlockType::Comparator
            | BlockType::StoneButton
            | BlockType::Lever
            | BlockType::PressurePlate
            | BlockType::Piston
            | BlockType::StickyPiston
            | BlockType::RedstoneLamp
            | BlockType::OakDoor
            | BlockType::OakTrapdoor
            | BlockType::TNT
            | BlockType::Dispenser
            | BlockType::Dropper
            | BlockType::Observer
            | BlockType::NoteBlock
    )
}

pub(super) fn is_movable(block: BlockType) -> bool {
    block != BlockType::Air
        && block != BlockType::Bedrock
        && !matches!(
            block,
            BlockType::Piston
                | BlockType::StickyPiston
        )
}

pub(super) fn get_block(manager: &WorldColumns, pos: BlockPos) -> BlockType {
    manager.get_block(pos.0, pos.1, pos.2)
}

pub(super) fn set_block_record(
    manager: &mut WorldColumns,
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

pub(super) fn set_block_record_with_state(
    manager: &mut WorldColumns,
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

pub(super) fn add(a: BlockPos, b: BlockPos) -> BlockPos {
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}

pub(super) fn sub(a: BlockPos, b: BlockPos) -> BlockPos {
    (a.0 - b.0, a.1 - b.1, a.2 - b.2)
}

pub(super) fn fill_plate_occupants(
    scratch: &mut HashSet<BlockPos>,
    components: &HashMap<BlockPos, ComponentState>,
    manager: &WorldColumns,
    occupants: &[BlockPos],
) {
    scratch.clear();
    for &(x, y, z) in occupants {
        let pos = (x, y - 1, z);
        if components.contains_key(&pos)
            && matches!(
                get_block(manager, pos),
                BlockType::PressurePlate
            )
        {
            scratch.insert(pos);
        }
    }
}

pub(super) fn is_comparator_block(block: BlockType) -> bool {
    matches!(block, BlockType::Comparator)
}

pub(super) fn is_transition_capable(block: BlockType) -> bool {
    matches!(
        block,
        BlockType::RedstoneTorch
            | BlockType::Comparator
            | BlockType::RedstoneLamp
            | BlockType::Repeater
            | BlockType::OakDoor
            | BlockType::OakTrapdoor
            | BlockType::Piston
            | BlockType::StickyPiston
            | BlockType::TNT
            | BlockType::Dispenser
            | BlockType::Dropper
            | BlockType::NoteBlock
    )
}
