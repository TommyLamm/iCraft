use super::*;

impl RedstoneSystem {
    pub(super) fn extend_piston(
        &self,
        manager: &mut WorldColumns,
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
        let sticky = matches!(block, BlockType::StickyPiston);
        let _ = sticky;
        set_open_flag(manager, pos, block, true, mutations);
    }

    pub(super) fn retract_piston(
        &self,
        manager: &mut WorldColumns,
        pos: BlockPos,
        facing: Direction,
        block: BlockType,
        mutations: &mut Vec<BlockMutation>,
    ) {
        let sticky = matches!(block, BlockType::StickyPiston);
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
        set_open_flag(manager, pos, block, false, mutations);
    }
}

pub(super) fn apply_powered_open_state(
    manager: &mut WorldColumns,
    pos: BlockPos,
    block: BlockType,
    powered: bool,
    mutations: &mut Vec<BlockMutation>,
) {
    let open = match block {
        BlockType::RedstoneTorch => !powered,
        _ => powered,
    };
    set_open_flag(manager, pos, block, open, mutations);
}

pub(super) fn block_open_at(manager: &WorldColumns, pos: BlockPos) -> bool {
    crate::world::BlockState::decode(manager.get_block_state(pos.0, pos.1, pos.2)).is_open
}

pub(super) fn set_open_flag(
    manager: &mut WorldColumns,
    pos: BlockPos,
    block: BlockType,
    is_open: bool,
    mutations: &mut Vec<BlockMutation>,
) {
    let mut bstate =
        crate::world::BlockState::decode(manager.get_block_state(pos.0, pos.1, pos.2));
    if get_block(manager, pos) == block && bstate.is_open == is_open {
        return;
    }
    bstate.is_open = is_open;
    set_block_record_with_state(manager, pos, block, bstate.encode(), mutations);
}

