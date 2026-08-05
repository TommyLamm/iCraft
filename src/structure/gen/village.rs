use crate::block_entity::{BlockEntity, ChestBlockEntity};
use crate::inventory::ContainerInventory;
use crate::loot::LootTableId;
use crate::structure::types::*;
use crate::world::BlockType;

pub fn generate_village(origin_x: i32, origin_y: i32, origin_z: i32, seed: u32) -> StructureStart {
    let mut blocks = Vec::new();
    let house_width = 7;
    let house_height = 5;
    let house_depth = 7;

    let min_x = origin_x;
    let min_y = origin_y;
    let min_z = origin_z;
    let max_x = origin_x + 18;
    let max_y = origin_y + house_height;
    let max_z = origin_z + 18;

    // Dirt paths
    for p in 0..18 {
        blocks.push(BlockPlacement {
            world_x: origin_x + p,
            world_y: origin_y,
            world_z: origin_z + 8,
            block_type: BlockType::DirtPath,
            block_entity: None,
        });
        blocks.push(BlockPlacement {
            world_x: origin_x + 8,
            world_y: origin_y,
            world_z: origin_z + p,
            block_type: BlockType::DirtPath,
            block_entity: None,
        });
    }

    // Village House 1
    for dx in 0..house_width {
        for dy in 0..house_height {
            for dz in 0..house_depth {
                let wx = origin_x + dx;
                let wy = origin_y + dy + 1;
                let wz = origin_z + dz;

                let is_wall = dx == 0
                    || dx == house_width - 1
                    || dz == 0
                    || dz == house_depth - 1
                    || dy == 0
                    || dy == house_height - 1;
                let is_roof = dy == house_height - 1;
                let block = if is_roof {
                    BlockType::OakPlanks
                } else if is_wall {
                    if (dx == 0 || dx == house_width - 1) && (dz == 0 || dz == house_depth - 1) {
                        BlockType::OakLog
                    } else {
                        BlockType::Cobblestone
                    }
                } else {
                    BlockType::Air
                };

                blocks.push(BlockPlacement {
                    world_x: wx,
                    world_y: wy,
                    world_z: wz,
                    block_type: block,
                    block_entity: None,
                });
            }
        }
    }

    // Doorway
    blocks.push(BlockPlacement {
        world_x: origin_x + 3,
        world_y: origin_y + 1,
        world_z: origin_z,
        block_type: BlockType::OakDoor,
        block_entity: None,
    });
    blocks.push(BlockPlacement {
        world_x: origin_x + 3,
        world_y: origin_y + 2,
        world_z: origin_z,
        block_type: BlockType::OakDoor,
        block_entity: None,
    });

    // Bed inside house
    blocks.push(BlockPlacement {
        world_x: origin_x + 1,
        world_y: origin_y + 1,
        world_z: origin_z + 5,
        block_type: BlockType::Bed,
        block_entity: None,
    });

    // Village Chest
    let chest_entity = BlockEntity::Chest(ChestBlockEntity {
        custom_name: None,
        inventory: ContainerInventory::new(),
        loot_table: Some(LootTableId::Village.as_str().to_string()),
        loot_seed: Some(seed as u64 ^ 0x5649_4C4C),
    });
    blocks.push(BlockPlacement {
        world_x: origin_x + 5,
        world_y: origin_y + 1,
        world_z: origin_z + 5,
        block_type: BlockType::Chest,
        block_entity: Some(chest_entity),
    });

    let bounding_box = BoundingBox::new(min_x, min_y, min_z, max_x, max_y, max_z);
    let piece = StructurePiece {
        bounding_box,
        blocks,
    };

    StructureStart {
        id: StructureId::Village,
        origin_x,
        origin_y,
        origin_z,
        bounding_box,
        pieces: vec![piece],
    }
}
