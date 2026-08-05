use crate::block_entity::{BlockEntity, ChestBlockEntity, SpawnerBlockEntity};
use crate::entity::EntityType;
use crate::inventory::ContainerInventory;
use crate::loot::LootTableId;
use crate::structure::types::*;
use crate::world::BlockType;

pub fn generate_nether_fortress(
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    seed: u32,
) -> StructureStart {
    let mut blocks = Vec::new();
    let length = 20;

    let min_x = origin_x;
    let min_y = origin_y;
    let min_z = origin_z;
    let max_x = origin_x + length - 1;
    let max_y = origin_y + 8;
    let max_z = origin_z + 10;

    // Bridge / Corridor built from NetherBrick
    for dx in 0..length {
        // Floor
        for dz in 0..5 {
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: origin_y,
                world_z: origin_z + dz,
                block_type: BlockType::NetherBrick,
                block_entity: None,
            });
        }
        // Pillars & Walls
        if dx % 5 == 0 {
            for dy in 1..4 {
                blocks.push(BlockPlacement {
                    world_x: origin_x + dx,
                    world_y: origin_y + dy,
                    world_z: origin_z,
                    block_type: BlockType::NetherBrick,
                    block_entity: None,
                });
                blocks.push(BlockPlacement {
                    world_x: origin_x + dx,
                    world_y: origin_y + dy,
                    world_z: origin_z + 4,
                    block_type: BlockType::NetherBrick,
                    block_entity: None,
                });
            }
        }
    }

    // Nether Wart Farm room
    let farm_x = origin_x + 8;
    let farm_z = origin_z + 6;
    for dx in 0..4 {
        for dz in 0..4 {
            blocks.push(BlockPlacement {
                world_x: farm_x + dx,
                world_y: origin_y,
                world_z: farm_z + dz,
                block_type: BlockType::SoulSand,
                block_entity: None,
            });
            blocks.push(BlockPlacement {
                world_x: farm_x + dx,
                world_y: origin_y + 1,
                world_z: farm_z + dz,
                block_type: BlockType::NetherWartCrop,
                block_entity: None,
            });
        }
    }

    // Blaze Spawner platform
    let spawner_x = origin_x + 16;
    let spawner_z = origin_z + 2;
    blocks.push(BlockPlacement {
        world_x: spawner_x,
        world_y: origin_y + 1,
        world_z: spawner_z,
        block_type: BlockType::Spawner,
        block_entity: Some(BlockEntity::Spawner(SpawnerBlockEntity {
            entity_type: EntityType::Blaze,
            spawn_delay: 160,
        })),
    });

    // Fortress Chest
    let chest_entity = BlockEntity::Chest(ChestBlockEntity {
        custom_name: None,
        inventory: ContainerInventory::new(),
        loot_table: Some(LootTableId::NetherBridge.as_str().to_string()),
        loot_seed: Some(seed as u64 ^ 0x464F_5254),
    });
    blocks.push(BlockPlacement {
        world_x: origin_x + 3,
        world_y: origin_y + 1,
        world_z: origin_z + 2,
        block_type: BlockType::Chest,
        block_entity: Some(chest_entity),
    });

    let bounding_box = BoundingBox::new(min_x, min_y, min_z, max_x, max_y, max_z);
    let piece = StructurePiece {
        bounding_box,
        blocks,
    };

    StructureStart {
        id: StructureId::NetherFortress,
        origin_x,
        origin_y,
        origin_z,
        bounding_box,
        pieces: vec![piece],
    }
}
