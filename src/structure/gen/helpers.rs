//! Shared structure-piece builders. Sorted placement fingerprints must stay
//! byte-identical (see `gen/mod.rs` fingerprint tests).

use crate::block_entity::{BlockEntity, ChestBlockEntity};
use crate::inventory::ContainerInventory;
use crate::structure::types::{
    BlockPlacement, BoundingBox, StructureId, StructurePiece, StructureStart,
};
use crate::world::BlockType;

#[inline]
pub fn push_block(
    blocks: &mut Vec<BlockPlacement>,
    x: i32,
    y: i32,
    z: i32,
    block_type: BlockType,
) {
    blocks.push(BlockPlacement {
        world_x: x,
        world_y: y,
        world_z: z,
        block_type,
        block_state: 0,
        block_entity: None,
    });
}

#[inline]
pub fn push_block_state(
    blocks: &mut Vec<BlockPlacement>,
    x: i32,
    y: i32,
    z: i32,
    block_type: BlockType,
    block_state: u8,
) {
    blocks.push(BlockPlacement {
        world_x: x,
        world_y: y,
        world_z: z,
        block_type,
        block_state,
        block_entity: None,
    });
}

/// Inclusive fill. Loop order: `dx` → `dy` → `dz`.
pub fn fill_box(
    blocks: &mut Vec<BlockPlacement>,
    ox: i32,
    oy: i32,
    oz: i32,
    sx: i32,
    sy: i32,
    sz: i32,
    block: BlockType,
) {
    for dx in 0..sx {
        for dy in 0..sy {
            for dz in 0..sz {
                push_block(blocks, ox + dx, oy + dy, oz + dz, block);
            }
        }
    }
}

/// Hollow room: `wall_block(dx,dy,dz)` on shell, Air inside.
/// `closed=false` → vertical XZ shell only (no floor/ceiling faces).
pub fn hollow_box<F>(
    blocks: &mut Vec<BlockPlacement>,
    ox: i32,
    oy: i32,
    oz: i32,
    sx: i32,
    sy: i32,
    sz: i32,
    closed: bool,
    mut wall_block: F,
) where
    F: FnMut(i32, i32, i32) -> BlockType,
{
    for dx in 0..sx {
        for dy in 0..sy {
            for dz in 0..sz {
                let on_xz = dx == 0 || dx == sx - 1 || dz == 0 || dz == sz - 1;
                let is_wall = if closed {
                    on_xz || dy == 0 || dy == sy - 1
                } else {
                    on_xz
                };
                let block = if is_wall {
                    wall_block(dx, dy, dz)
                } else {
                    BlockType::Air
                };
                push_block(blocks, ox + dx, oy + dy, oz + dz, block);
            }
        }
    }
}

pub fn place_loot_chest(
    blocks: &mut Vec<BlockPlacement>,
    x: i32,
    y: i32,
    z: i32,
    loot_table: &str,
    loot_seed: u64,
    custom_name: Option<String>,
) {
    blocks.push(BlockPlacement {
        world_x: x,
        world_y: y,
        world_z: z,
        block_type: BlockType::Chest,
        block_state: 0,
        block_entity: Some(BlockEntity::Chest(ChestBlockEntity {
            custom_name,
            inventory: ContainerInventory::new(),
            loot_table: Some(loot_table.to_string()),
            loot_seed: Some(loot_seed),
            revision: 0,
        })),
    });
}

pub fn finish_start(
    id: StructureId,
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    min_x: i32,
    min_y: i32,
    min_z: i32,
    max_x: i32,
    max_y: i32,
    max_z: i32,
    blocks: Vec<BlockPlacement>,
) -> StructureStart {
    let bounding_box = BoundingBox::new(min_x, min_y, min_z, max_x, max_y, max_z);
    StructureStart {
        id,
        origin_x,
        origin_y,
        origin_z,
        bounding_box,
        pieces: vec![StructurePiece {
            bounding_box,
            blocks,
        }],
    }
}
