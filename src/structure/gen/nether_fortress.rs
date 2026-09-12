use crate::block_entity::{BlockEntity, SpawnerBlockEntity};
use crate::entity::EntityType;
use crate::loot::LootTableId;
use crate::structure::gen::helpers::{finish_start, fill_box, place_loot_chest, push_block};
use crate::structure::types::*;
use crate::world::BlockType;

pub fn generate_nether_fortress(
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    seed: u32,
) -> StructureStart {
    let length = 20;
    let mut blocks = Vec::new();

    for dx in 0..length {
        fill_box(
            &mut blocks,
            origin_x + dx,
            origin_y,
            origin_z,
            1,
            1,
            5,
            BlockType::NetherBrick,
        );
        if dx % 5 == 0 {
            for dy in 1..4 {
                push_block(
                    &mut blocks,
                    origin_x + dx,
                    origin_y + dy,
                    origin_z,
                    BlockType::NetherBrick,
                );
                push_block(
                    &mut blocks,
                    origin_x + dx,
                    origin_y + dy,
                    origin_z + 4,
                    BlockType::NetherBrick,
                );
            }
        }
    }

    let farm_x = origin_x + 8;
    let farm_z = origin_z + 6;
    fill_box(
        &mut blocks,
        farm_x,
        origin_y,
        farm_z,
        4,
        1,
        4,
        BlockType::SoulSand,
    );
    fill_box(
        &mut blocks,
        farm_x,
        origin_y + 1,
        farm_z,
        4,
        1,
        4,
        BlockType::NetherWartCrop,
    );

    blocks.push(BlockPlacement {
        world_x: origin_x + 16,
        world_y: origin_y + 1,
        world_z: origin_z + 2,
        block_type: BlockType::Spawner,
        block_state: 0,
        block_entity: Some(BlockEntity::Spawner(SpawnerBlockEntity {
            entity_type: EntityType::Blaze,
            spawn_delay: 160,
        })),
    });

    place_loot_chest(
        &mut blocks,
        origin_x + 3,
        origin_y + 1,
        origin_z + 2,
        LootTableId::NetherBridge.as_str(),
        seed as u64 ^ 0x464F_5254,
        None,
    );

    finish_start(
        StructureId::NetherFortress,
        origin_x,
        origin_y,
        origin_z,
        origin_x,
        origin_y,
        origin_z,
        origin_x + length - 1,
        origin_y + 8,
        origin_z + 10,
        blocks,
    )
}
