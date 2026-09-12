use crate::loot::LootTableId;
use crate::structure::gen::helpers::{finish_start, fill_box, place_loot_chest, push_block};
use crate::structure::types::*;
use crate::world::BlockType;

pub fn generate_mineshaft(
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    seed: u32,
) -> StructureStart {
    let corridor_length = 24;
    let mut blocks = Vec::new();

    for dx in 0..corridor_length {
        fill_box(
            &mut blocks,
            origin_x + dx,
            origin_y,
            origin_z,
            1,
            3,
            3,
            BlockType::Air,
        );

        if dx % 5 == 0 {
            push_block(
                &mut blocks,
                origin_x + dx,
                origin_y,
                origin_z,
                BlockType::OakLog,
            );
            push_block(
                &mut blocks,
                origin_x + dx,
                origin_y + 1,
                origin_z,
                BlockType::OakLog,
            );
            push_block(
                &mut blocks,
                origin_x + dx,
                origin_y + 2,
                origin_z,
                BlockType::OakPlanks,
            );
            push_block(
                &mut blocks,
                origin_x + dx,
                origin_y + 2,
                origin_z + 1,
                BlockType::OakPlanks,
            );
            push_block(
                &mut blocks,
                origin_x + dx,
                origin_y + 2,
                origin_z + 2,
                BlockType::OakPlanks,
            );
            push_block(
                &mut blocks,
                origin_x + dx,
                origin_y + 1,
                origin_z + 2,
                BlockType::OakLog,
            );
            push_block(
                &mut blocks,
                origin_x + dx,
                origin_y,
                origin_z + 2,
                BlockType::OakLog,
            );
        }
    }

    place_loot_chest(
        &mut blocks,
        origin_x + 12,
        origin_y,
        origin_z + 1,
        LootTableId::Mineshaft.as_str(),
        seed as u64 ^ 0x4D49_4E45,
        None,
    );

    finish_start(
        StructureId::Mineshaft,
        origin_x,
        origin_y,
        origin_z,
        origin_x,
        origin_y,
        origin_z,
        origin_x + corridor_length - 1,
        origin_y + 3,
        origin_z + 4,
        blocks,
    )
}
