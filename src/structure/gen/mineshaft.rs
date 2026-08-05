use crate::block_entity::{BlockEntity, ChestBlockEntity};
use crate::inventory::ContainerInventory;
use crate::loot::LootTableId;
use crate::structure::types::*;
use crate::world::BlockType;

pub fn generate_mineshaft(
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    seed: u32,
) -> StructureStart {
    let mut blocks = Vec::new();
    let corridor_length = 24;

    let min_x = origin_x;
    let min_y = origin_y;
    let min_z = origin_z;
    let max_x = origin_x + corridor_length - 1;
    let max_y = origin_y + 3;
    let max_z = origin_z + 4;

    for dx in 0..corridor_length {
        for dy in 0..3 {
            for dz in 0..3 {
                let wx = origin_x + dx;
                let wy = origin_y + dy;
                let wz = origin_z + dz;

                // Air corridor
                blocks.push(BlockPlacement {
                    world_x: wx,
                    world_y: wy,
                    world_z: wz,
                    block_type: BlockType::Air,
                    block_entity: None,
                });
            }
        }

        // Wood support frame every 5 blocks
        if dx % 5 == 0 {
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: origin_y,
                world_z: origin_z,
                block_type: BlockType::OakLog,
                block_entity: None,
            });
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: origin_y + 1,
                world_z: origin_z,
                block_type: BlockType::OakLog,
                block_entity: None,
            });
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: origin_y + 2,
                world_z: origin_z,
                block_type: BlockType::OakPlanks,
                block_entity: None,
            });
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: origin_y + 2,
                world_z: origin_z + 1,
                block_type: BlockType::OakPlanks,
                block_entity: None,
            });
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: origin_y + 2,
                world_z: origin_z + 2,
                block_type: BlockType::OakPlanks,
                block_entity: None,
            });
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: origin_y + 1,
                world_z: origin_z + 2,
                block_type: BlockType::OakLog,
                block_entity: None,
            });
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: origin_y,
                world_z: origin_z + 2,
                block_type: BlockType::OakLog,
                block_entity: None,
            });
        }
    }

    // Chest placement along corridor
    let cx = origin_x + 12;
    let cy = origin_y;
    let cz = origin_z + 1;
    let chest_entity = BlockEntity::Chest(ChestBlockEntity {
        custom_name: None,
        inventory: ContainerInventory::new(),
        loot_table: Some(LootTableId::Mineshaft.as_str().to_string()),
        loot_seed: Some(seed as u64 ^ 0x4D49_4E45),
    });
    blocks.push(BlockPlacement {
        world_x: cx,
        world_y: cy,
        world_z: cz,
        block_type: BlockType::Chest,
        block_entity: Some(chest_entity),
    });

    let bounding_box = BoundingBox::new(min_x, min_y, min_z, max_x, max_y, max_z);
    let piece = StructurePiece {
        bounding_box,
        blocks,
    };

    StructureStart {
        id: StructureId::Mineshaft,
        origin_x,
        origin_y,
        origin_z,
        bounding_box,
        pieces: vec![piece],
    }
}
