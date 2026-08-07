use crate::block_entity::{BlockEntity, ChestBlockEntity, SpawnerBlockEntity};
use crate::entity::EntityType;
use crate::inventory::ContainerInventory;
use crate::loot::LootTableId;
use crate::structure::types::*;
use crate::world::BlockType;

pub fn generate_dungeon(origin_x: i32, origin_y: i32, origin_z: i32, seed: u32) -> StructureStart {
    let mut blocks = Vec::new();
    let width = 7;
    let height = 5;
    let depth = 7;

    let min_x = origin_x;
    let min_y = origin_y;
    let min_z = origin_z;
    let max_x = origin_x + width - 1;
    let max_y = origin_y + height - 1;
    let max_z = origin_z + depth - 1;

    for dx in 0..width {
        for dy in 0..height {
            for dz in 0..depth {
                let wx = origin_x + dx;
                let wy = origin_y + dy;
                let wz = origin_z + dz;

                let is_wall = dx == 0
                    || dx == width - 1
                    || dz == 0
                    || dz == depth - 1
                    || dy == 0
                    || dy == height - 1;
                if is_wall {
                    let is_mossy = (seed.wrapping_add((dx * 31 + dy * 17 + dz) as u32)) % 4 == 0;
                    let block = if is_mossy {
                        BlockType::MossyCobblestone
                    } else {
                        BlockType::Cobblestone
                    };
                    blocks.push(BlockPlacement {
                        world_x: wx,
                        world_y: wy,
                        world_z: wz,
                        block_type: block,
                        block_entity: None,
                    });
                } else {
                    blocks.push(BlockPlacement {
                        world_x: wx,
                        world_y: wy,
                        world_z: wz,
                        block_type: BlockType::Air,
                        block_entity: None,
                    });
                }
            }
        }
    }

    // Center spawner
    let spawner_entity_type = if seed % 2 == 0 {
        EntityType::Zombie
    } else {
        EntityType::Skeleton
    };
    blocks.push(BlockPlacement {
        world_x: origin_x + 3,
        world_y: origin_y + 1,
        world_z: origin_z + 3,
        block_type: BlockType::Spawner,
        block_entity: Some(BlockEntity::Spawner(SpawnerBlockEntity {
            entity_type: spawner_entity_type,
            spawn_delay: 200,
        })),
    });

    // 1-2 Chests with Dungeon loot table
    let chest_count = 1 + (seed % 2) as i32;
    let chest_positions = [
        (origin_x + 1, origin_y + 1, origin_z + 1),
        (origin_x + 5, origin_y + 1, origin_z + 5),
    ];
    for &(cx, cy, cz) in chest_positions.iter().take(chest_count as usize) {
        let chest_entity = BlockEntity::Chest(ChestBlockEntity {
            custom_name: None,
            inventory: ContainerInventory::new(),
            loot_table: Some(LootTableId::Dungeon.as_str().to_string()),
            loot_seed: Some(seed as u64 ^ (cx as u64 * 31 + cz as u64)),
            revision: 0,
        });
        blocks.push(BlockPlacement {
            world_x: cx,
            world_y: cy,
            world_z: cz,
            block_type: BlockType::Chest,
            block_entity: Some(chest_entity),
        });
    }

    let bounding_box = BoundingBox::new(min_x, min_y, min_z, max_x, max_y, max_z);
    let piece = StructurePiece {
        bounding_box,
        blocks,
    };

    StructureStart {
        id: StructureId::Dungeon,
        origin_x,
        origin_y,
        origin_z,
        bounding_box,
        pieces: vec![piece],
    }
}
