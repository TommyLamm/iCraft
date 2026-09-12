use crate::block_entity::{BlockEntity, SpawnerBlockEntity};
use crate::entity::EntityType;
use crate::loot::LootTableId;
use crate::structure::gen::helpers::{
    finish_start, hollow_box, place_loot_chest, push_block, push_block_state,
};
use crate::structure::types::*;
use crate::world::BlockType;

pub fn generate_stronghold(
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    seed: u32,
) -> StructureStart {
    let mut blocks = Vec::new();

    hollow_box(
        &mut blocks,
        origin_x,
        origin_y,
        origin_z,
        15,
        6,
        10,
        true,
        |_, _, _| BlockType::StoneBrick,
    );

    place_loot_chest(
        &mut blocks,
        origin_x + 2,
        origin_y + 1,
        origin_z + 2,
        LootTableId::StrongholdCorridor.as_str(),
        seed as u64 ^ 0x5354_524F,
        None,
    );

    let portal_room_x = origin_x + 2;
    let portal_room_y = origin_y;
    let portal_room_z = origin_z + 12;

    hollow_box(
        &mut blocks,
        portal_room_x,
        portal_room_y,
        portal_room_z,
        11,
        7,
        11,
        true,
        |_, _, _| BlockType::StoneBrick,
    );

    for dy in 1..4 {
        for dx in 5..8 {
            push_block(
                &mut blocks,
                origin_x + dx,
                origin_y + dy,
                origin_z + 10,
                BlockType::Air,
            );
            push_block(
                &mut blocks,
                origin_x + dx,
                origin_y + dy,
                origin_z + 11,
                BlockType::Air,
            );
        }
    }

    let p_center_x = portal_room_x + 5;
    let p_center_y = portal_room_y + 2;
    let p_center_z = portal_room_z + 5;

    for dx in -1..=1 {
        for dz in -1..=1 {
            push_block(
                &mut blocks,
                p_center_x + dx,
                p_center_y - 1,
                p_center_z + dz,
                BlockType::Lava,
            );
        }
    }

    let frame_offsets = [
        (-1, 0, -2),
        (0, 0, -2),
        (1, 0, -2),
        (2, 0, -1),
        (2, 0, 0),
        (2, 0, 1),
        (-1, 0, 2),
        (0, 0, 2),
        (1, 0, 2),
        (-2, 0, -1),
        (-2, 0, 0),
        (-2, 0, 1),
    ];

    for (i, &(dx, dy, dz)) in frame_offsets.iter().enumerate() {
        let pre_filled =
            (seed.wrapping_add((i * 17 + (dx as i32).unsigned_abs() as usize) as u32)) % 10 == 0;
        let mut frame_state = crate::world::BlockState::default();
        frame_state.is_open = pre_filled;
        push_block_state(
            &mut blocks,
            p_center_x + dx,
            p_center_y + dy,
            p_center_z + dz,
            BlockType::EndPortalFrame,
            frame_state.encode(),
        );
    }

    blocks.push(BlockPlacement {
        world_x: p_center_x,
        world_y: p_center_y,
        world_z: p_center_z - 3,
        block_type: BlockType::Spawner,
        block_state: 0,
        block_entity: Some(BlockEntity::Spawner(SpawnerBlockEntity {
            entity_type: EntityType::Zombie,
            spawn_delay: 150,
        })),
    });

    finish_start(
        StructureId::Stronghold,
        origin_x,
        origin_y,
        origin_z,
        origin_x,
        origin_y,
        origin_z,
        origin_x + 15,
        origin_y + 7,
        origin_z + 25,
        blocks,
    )
}
