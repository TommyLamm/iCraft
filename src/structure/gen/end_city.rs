use crate::block_entity::{BlockEntity, ChestBlockEntity};
use crate::inventory::ContainerInventory;
use crate::loot::LootTableId;
use crate::structure::types::*;
use crate::world::BlockType;

pub fn generate_end_city(origin_x: i32, origin_y: i32, origin_z: i32, seed: u32) -> StructureStart {
    let mut blocks = Vec::new();
    let tower_height = 16;
    let base_size = 7;

    let min_x = origin_x - 3;
    let min_y = origin_y;
    let min_z = origin_z - 3;
    let max_x = origin_x + base_size + 3;
    let max_y = origin_y + tower_height + 5;
    let max_z = origin_z + base_size + 3;

    // Tower base platform (EndStoneBrick / Purpur)
    for dx in 0..base_size {
        for dz in 0..base_size {
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: origin_y,
                world_z: origin_z + dz,
                block_type: BlockType::EndStoneBrick,
                block_entity: None,
            });
        }
    }

    // Tower walls (Purpur)
    for dy in 1..=tower_height {
        for dx in 0..base_size {
            for dz in 0..base_size {
                let is_wall = dx == 0 || dx == base_size - 1 || dz == 0 || dz == base_size - 1;
                let block = if is_wall {
                    BlockType::Purpur
                } else {
                    BlockType::Air
                };
                blocks.push(BlockPlacement {
                    world_x: origin_x + dx,
                    world_y: origin_y + dy,
                    world_z: origin_z + dz,
                    block_type: block,
                    block_entity: None,
                });
            }
        }
    }

    // Top room (Ship / Treasury room)
    let top_y = origin_y + tower_height + 1;
    for dx in -1..=(base_size as i32) {
        for dz in -1..=(base_size as i32) {
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: top_y,
                world_z: origin_z + dz,
                block_type: BlockType::Purpur,
                block_entity: None,
            });
        }
    }

    // End City Treasure Chest containing Elytra loot table
    let chest_x = origin_x + 3;
    let chest_y = top_y + 1;
    let chest_z = origin_z + 3;

    let chest_entity = BlockEntity::Chest(ChestBlockEntity {
        custom_name: Some("End City Treasure".to_string()),
        inventory: ContainerInventory::new(),
        loot_table: Some(LootTableId::EndCity.as_str().to_string()),
        loot_seed: Some(seed as u64 ^ 0x454E_4443),
    });
    blocks.push(BlockPlacement {
        world_x: chest_x,
        world_y: chest_y,
        world_z: chest_z,
        block_type: BlockType::Chest,
        block_entity: Some(chest_entity),
    });

    let bounding_box = BoundingBox::new(min_x, min_y, min_z, max_x, max_y, max_z);
    let piece = StructurePiece {
        bounding_box,
        blocks,
    };

    StructureStart {
        id: StructureId::EndCity,
        origin_x,
        origin_y,
        origin_z,
        bounding_box,
        pieces: vec![piece],
    }
}
