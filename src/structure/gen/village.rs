use crate::loot::LootTableId;
use crate::structure::gen::helpers::{
    finish_start, hollow_box, place_loot_chest, push_block,
};
use crate::structure::types::*;
use crate::world::BlockType;

pub fn generate_village(origin_x: i32, origin_y: i32, origin_z: i32, seed: u32) -> StructureStart {
    let house_width = 7;
    let house_height = 5;
    let house_depth = 7;
    let mut blocks = Vec::new();

    for p in 0..18 {
        push_block(
            &mut blocks,
            origin_x + p,
            origin_y,
            origin_z + 8,
            BlockType::DirtPath,
        );
        push_block(
            &mut blocks,
            origin_x + 8,
            origin_y,
            origin_z + p,
            BlockType::DirtPath,
        );
    }

    hollow_box(
        &mut blocks,
        origin_x,
        origin_y + 1,
        origin_z,
        house_width,
        house_height,
        house_depth,
        true,
        |dx, dy, dz| {
            let is_roof = dy == house_height - 1;
            if is_roof {
                BlockType::OakPlanks
            } else if (dx == 0 || dx == house_width - 1) && (dz == 0 || dz == house_depth - 1) {
                BlockType::OakLog
            } else {
                BlockType::Cobblestone
            }
        },
    );

    push_block(
        &mut blocks,
        origin_x + 3,
        origin_y + 1,
        origin_z,
        BlockType::OakDoor,
    );
    push_block(
        &mut blocks,
        origin_x + 3,
        origin_y + 2,
        origin_z,
        BlockType::OakDoor,
    );
    push_block(
        &mut blocks,
        origin_x + 1,
        origin_y + 1,
        origin_z + 5,
        BlockType::Bed,
    );

    place_loot_chest(
        &mut blocks,
        origin_x + 5,
        origin_y + 1,
        origin_z + 5,
        LootTableId::Village.as_str(),
        seed as u64 ^ 0x5649_4C4C,
        None,
    );

    finish_start(
        StructureId::Village,
        origin_x,
        origin_y,
        origin_z,
        origin_x,
        origin_y,
        origin_z,
        origin_x + 18,
        origin_y + house_height,
        origin_z + 18,
        blocks,
    )
}
