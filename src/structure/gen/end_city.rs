use crate::loot::LootTableId;
use crate::structure::gen::helpers::{
    finish_start, fill_box, hollow_box, place_loot_chest,
};
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

    fill_box(
        &mut blocks,
        origin_x,
        origin_y,
        origin_z,
        base_size,
        1,
        base_size,
        BlockType::EndStoneBrick,
    );

    hollow_box(
        &mut blocks,
        origin_x,
        origin_y + 1,
        origin_z,
        base_size,
        tower_height,
        base_size,
        false,
        |_, _, _| BlockType::Purpur,
    );

    let top_y = origin_y + tower_height + 1;
    fill_box(
        &mut blocks,
        origin_x - 1,
        top_y,
        origin_z - 1,
        base_size + 2,
        1,
        base_size + 2,
        BlockType::Purpur,
    );

    place_loot_chest(
        &mut blocks,
        origin_x + 3,
        top_y + 1,
        origin_z + 3,
        LootTableId::EndCity.as_str(),
        seed as u64 ^ 0x454E_4443,
        Some("End City Treasure".to_string()),
    );

    finish_start(
        StructureId::EndCity,
        origin_x,
        origin_y,
        origin_z,
        min_x,
        min_y,
        min_z,
        max_x,
        max_y,
        max_z,
        blocks,
    )
}
