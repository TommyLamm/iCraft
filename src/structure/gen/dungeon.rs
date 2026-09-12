use crate::block_entity::{BlockEntity, SpawnerBlockEntity};
use crate::entity::EntityType;
use crate::loot::LootTableId;
use crate::structure::gen::helpers::{finish_start, hollow_box, place_loot_chest};
use crate::structure::types::*;
use crate::world::BlockType;

pub fn generate_dungeon(origin_x: i32, origin_y: i32, origin_z: i32, seed: u32) -> StructureStart {
    let width = 7;
    let height = 5;
    let depth = 7;
    let mut blocks = Vec::new();

    hollow_box(
        &mut blocks,
        origin_x,
        origin_y,
        origin_z,
        width,
        height,
        depth,
        true,
        |dx, dy, dz| {
            let is_mossy = (seed.wrapping_add((dx * 31 + dy * 17 + dz) as u32)) % 4 == 0;
            if is_mossy {
                BlockType::MossyCobblestone
            } else {
                BlockType::Cobblestone
            }
        },
    );

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
        block_state: 0,
        block_entity: Some(BlockEntity::Spawner(SpawnerBlockEntity {
            entity_type: spawner_entity_type,
            spawn_delay: 200,
        })),
    });

    let chest_count = 1 + (seed % 2) as i32;
    let chest_positions = [
        (origin_x + 1, origin_y + 1, origin_z + 1),
        (origin_x + 5, origin_y + 1, origin_z + 5),
    ];
    for &(cx, cy, cz) in chest_positions.iter().take(chest_count as usize) {
        place_loot_chest(
            &mut blocks,
            cx,
            cy,
            cz,
            LootTableId::Dungeon.as_str(),
            (seed as u64) ^ (cx as u64).wrapping_mul(31).wrapping_add(cz as u64),
            None,
        );
    }

    finish_start(
        StructureId::Dungeon,
        origin_x,
        origin_y,
        origin_z,
        origin_x,
        origin_y,
        origin_z,
        origin_x + width - 1,
        origin_y + height - 1,
        origin_z + depth - 1,
        blocks,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_entity::BlockEntity;

    fn chest_loot_seeds(start: &StructureStart) -> Vec<u64> {
        start
            .pieces
            .iter()
            .flat_map(|piece| piece.blocks.iter())
            .filter_map(|placement| match placement.block_entity.as_ref() {
                Some(BlockEntity::Chest(chest)) => chest.loot_seed,
                _ => None,
            })
            .collect()
    }

    #[test]
    fn negative_coordinates_have_deterministic_non_panicking_loot_seeds() {
        let first = generate_dungeon(-511, 20, -1025, 42);
        let second = generate_dungeon(-511, 20, -1025, 42);

        let first_seeds = chest_loot_seeds(&first);
        assert!(!first_seeds.is_empty());
        assert_eq!(first_seeds, chest_loot_seeds(&second));
    }
}
