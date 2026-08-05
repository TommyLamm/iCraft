use crate::block_entity::{BlockEntity, ChestBlockEntity, SpawnerBlockEntity};
use crate::entity::EntityType;
use crate::inventory::ContainerInventory;
use crate::loot::LootTableId;
use crate::structure::types::*;
use crate::world::BlockType;

pub fn generate_stronghold(
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    seed: u32,
) -> StructureStart {
    let mut blocks = Vec::new();

    // Piece 1: Corridors & Entrance Room (StoneBrick)
    let min_x = origin_x;
    let min_y = origin_y;
    let min_z = origin_z;
    let max_x = origin_x + 15;
    let max_y = origin_y + 7;
    let max_z = origin_z + 25;

    // Room 1: Main Corridor / Entrance
    for dx in 0..15 {
        for dy in 0..6 {
            for dz in 0..10 {
                let wx = origin_x + dx;
                let wy = origin_y + dy;
                let wz = origin_z + dz;

                let is_wall = dx == 0 || dx == 14 || dz == 0 || dz == 9 || dy == 0 || dy == 5;
                let block = if is_wall {
                    BlockType::StoneBrick
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

    // Corridor chest
    let chest_entity1 = BlockEntity::Chest(ChestBlockEntity {
        custom_name: None,
        inventory: ContainerInventory::new(),
        loot_table: Some(LootTableId::StrongholdCorridor.as_str().to_string()),
        loot_seed: Some(seed as u64 ^ 0x5354_524F),
    });
    blocks.push(BlockPlacement {
        world_x: origin_x + 2,
        world_y: origin_y + 1,
        world_z: origin_z + 2,
        block_type: BlockType::Chest,
        block_entity: Some(chest_entity1),
    });

    // Room 2: Portal Room
    let portal_room_x = origin_x + 2;
    let portal_room_y = origin_y;
    let portal_room_z = origin_z + 12;

    for dx in 0..11 {
        for dy in 0..7 {
            for dz in 0..11 {
                let wx = portal_room_x + dx;
                let wy = portal_room_y + dy;
                let wz = portal_room_z + dz;

                let is_wall = dx == 0 || dx == 10 || dz == 0 || dz == 10 || dy == 0 || dy == 6;
                let block = if is_wall {
                    BlockType::StoneBrick
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

    // Doorway connecting corridor to portal room
    for dy in 1..4 {
        for dx in 5..8 {
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: origin_y + dy,
                world_z: origin_z + 10,
                block_type: BlockType::Air,
                block_entity: None,
            });
            blocks.push(BlockPlacement {
                world_x: origin_x + dx,
                world_y: origin_y + dy,
                world_z: origin_z + 11,
                block_type: BlockType::Air,
                block_entity: None,
            });
        }
    }

    // Portal Room details: Lava pool under portal frame
    let p_center_x = portal_room_x + 5;
    let p_center_y = portal_room_y + 2;
    let p_center_z = portal_room_z + 5;

    for dx in -1..=1 {
        for dz in -1..=1 {
            blocks.push(BlockPlacement {
                world_x: p_center_x + dx,
                world_y: p_center_y - 1,
                world_z: p_center_z + dz,
                block_type: BlockType::Lava,
                block_entity: None,
            });
        }
    }

    // End Portal Frames (12 blocks around 3x3 ring)
    let frame_offsets = [
        (-1, 0, -2),
        (0, 0, -2),
        (1, 0, -2), // North
        (2, 0, -1),
        (2, 0, 0),
        (2, 0, 1), // East
        (-1, 0, 2),
        (0, 0, 2),
        (1, 0, 2), // South
        (-2, 0, -1),
        (-2, 0, 0),
        (-2, 0, 1), // West
    ];

    for (i, &(dx, dy, dz)) in frame_offsets.iter().enumerate() {
        let frame_x = p_center_x + dx;
        let frame_y = p_center_y + dy;
        let frame_z = p_center_z + dz;

        // ~10% chance to be pre-filled with Eye of Ender
        let pre_filled = (seed.wrapping_add((i * 17 + dx.abs() as usize) as u32)) % 10 == 0;
        let block_type = if pre_filled {
            BlockType::EndPortalFrameFilled
        } else {
            BlockType::EndPortalFrame
        };

        blocks.push(BlockPlacement {
            world_x: frame_x,
            world_y: frame_y,
            world_z: frame_z,
            block_type,
            block_entity: None,
        });
    }

    // Spawner in Portal Room (Zombie / Silverfish)
    blocks.push(BlockPlacement {
        world_x: p_center_x,
        world_y: p_center_y,
        world_z: p_center_z - 3,
        block_type: BlockType::Spawner,
        block_entity: Some(BlockEntity::Spawner(SpawnerBlockEntity {
            entity_type: EntityType::Zombie,
            spawn_delay: 150,
        })),
    });

    let bounding_box = BoundingBox::new(min_x, min_y, min_z, max_x, max_y, max_z);
    let piece = StructurePiece {
        bounding_box,
        blocks,
    };

    StructureStart {
        id: StructureId::Stronghold,
        origin_x,
        origin_y,
        origin_z,
        bounding_box,
        pieces: vec![piece],
    }
}
