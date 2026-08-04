use crate::chunk_manager::ChunkManager;
use crate::world::{BlockType, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
use crate::world_mutation::{BlockMutationRequest, MutationCause};

/// Statistics for random tick sampling per frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RandomTickStats {
    pub sampled_sections: u32,
    pub total_ticks: u32,
    pub mutations_generated: u32,
    pub backlog_sections: u32,
}

/// Deterministic SplitMix64 pseudo-RNG helper.
pub fn deterministic_rng(seed: u64, salt: u64) -> u64 {
    let mut x = seed.wrapping_add(salt);
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^= x >> 31;
    x
}

/// Checks if a water block is within 4 blocks horizontally (x, z offset <= 4)
/// and within -1..=1 vertically of the farmland block.
pub fn is_water_nearby<F>(pos: (i32, i32, i32), mut get_block: F) -> bool
where
    F: FnMut(i32, i32, i32) -> Option<BlockType>,
{
    let (fx, fy, fz) = pos;
    for dx in -4..=4 {
        for dz in -4..=4 {
            for dy in -1..=1 {
                let target_y = fy + dy;
                if target_y >= 0 && target_y < CHUNK_HEIGHT as i32 {
                    if let Some(BlockType::Water) = get_block(fx + dx, target_y, fz + dz) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Evaluates a random tick event on a single voxel position.
pub fn evaluate_random_tick_at<F>(
    pos: (i32, i32, i32),
    block: BlockType,
    state: u8,
    rng_val: u64,
    mut get_block: F,
) -> Option<BlockMutationRequest>
where
    F: FnMut(i32, i32, i32) -> Option<BlockType>,
{
    let (x, y, z) = pos;
    match block {
        BlockType::Farmland => {
            let moisture = state & 0b111;
            let block_above = get_block(x, y + 1, z).unwrap_or(BlockType::Air);
            if block_above.properties().is_solid {
                // Covered by solid block -> decay to Dirt
                return Some(BlockMutationRequest {
                    pos,
                    new_block: BlockType::Dirt,
                    new_state: 0,
                    new_entity: None,
                    cause: MutationCause::System,
                });
            }

            let water_near = is_water_nearby(pos, &mut get_block);
            if water_near {
                if moisture < 7 {
                    return Some(BlockMutationRequest {
                        pos,
                        new_block: BlockType::Farmland,
                        new_state: moisture + 1,
                        new_entity: None,
                        cause: MutationCause::System,
                    });
                }
            } else {
                if moisture > 0 {
                    return Some(BlockMutationRequest {
                        pos,
                        new_block: BlockType::Farmland,
                        new_state: moisture - 1,
                        new_entity: None,
                        cause: MutationCause::System,
                    });
                } else if block_above == BlockType::Air {
                    // Dry farmland with no crop -> decay to Dirt
                    return Some(BlockMutationRequest {
                        pos,
                        new_block: BlockType::Dirt,
                        new_state: 0,
                        new_entity: None,
                        cause: MutationCause::System,
                    });
                }
            }
            None
        }
        BlockType::WheatCrop | BlockType::CarrotCrop | BlockType::PotatoCrop => {
            let age = state & 0b111;
            let block_below = get_block(x, y - 1, z).unwrap_or(BlockType::Air);
            if block_below != BlockType::Farmland {
                // Unsupported crop -> revert to Air (harvesting drop handled on break)
                return Some(BlockMutationRequest {
                    pos,
                    new_block: BlockType::Air,
                    new_state: 0,
                    new_entity: None,
                    cause: MutationCause::System,
                });
            }

            if age < 7 {
                // Check farmland below moisture
                let farmland_state = 7u8; // default to hydrated if loaded state unknown
                let is_hydrated = (farmland_state & 0b111) > 0;
                let growth_chance = if is_hydrated { 3 } else { 7 };
                if (rng_val % growth_chance) == 0 {
                    return Some(BlockMutationRequest {
                        pos,
                        new_block: block,
                        new_state: age + 1,
                        new_entity: None,
                        cause: MutationCause::System,
                    });
                }
            }
            None
        }
        _ => None,
    }
}

/// Samples random ticks across loaded chunks in the ChunkManager.
pub fn sample_random_ticks(
    chunk_manager: &ChunkManager,
    world_seed: u64,
    game_tick: u64,
    dimension: u8,
    max_sections_per_tick: usize,
) -> (Vec<BlockMutationRequest>, RandomTickStats) {
    let mut requests = Vec::new();
    let mut stats = RandomTickStats::default();

    let mut eligible_sections = Vec::new();
    for (&(cx, cz), chunk) in &chunk_manager.chunks {
        for (sec_idx, section) in chunk.sections.iter().enumerate() {
            if section.random_tick_count() > 0 {
                eligible_sections.push((cx, cz, sec_idx as i16));
            }
        }
    }

    // Sort deterministically to avoid HashMap order non-determinism
    eligible_sections.sort_unstable();

    let total_eligible = eligible_sections.len();
    let process_count = total_eligible.min(max_sections_per_tick);
    stats.backlog_sections = (total_eligible - process_count) as u32;

    for (i, &(cx, cz, sec_y)) in eligible_sections.iter().take(process_count).enumerate() {
        stats.sampled_sections += 1;
        let sec_salt = deterministic_rng(
            world_seed,
            game_tick
                ^ (cx as u64)
                ^ ((cz as u64) << 16)
                ^ ((sec_y as u64) << 32)
                ^ ((dimension as u64) << 48)
                ^ (i as u64),
        );

        // Vanilla Minecraft default randomTickSpeed is 3 ticks per section
        for tick_idx in 0..3 {
            stats.total_ticks += 1;
            let rng_val = deterministic_rng(sec_salt, tick_idx as u64);
            let lx = ((rng_val) & 0xF) as i32;
            let ly = ((rng_val >> 4) & 0xF) as i32;
            let lz = ((rng_val >> 8) & 0xF) as i32;

            let world_x = cx * (CHUNK_WIDTH as i32) + lx;
            let world_y = (sec_y as i32) * 16 + ly;
            let world_z = cz * (CHUNK_DEPTH as i32) + lz;

            let block = chunk_manager.get_block(world_x, world_y, world_z);
            let state = chunk_manager.get_block_state(world_x, world_y, world_z);

            if let Some(req) = evaluate_random_tick_at(
                (world_x, world_y, world_z),
                block,
                state,
                rng_val,
                |x, y, z| Some(chunk_manager.get_block(x, y, z)),
            ) {
                requests.push(req);
                stats.mutations_generated += 1;
            }
        }
    }

    (requests, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_rng_reproducibility() {
        let r1 = deterministic_rng(12345, 99);
        let r2 = deterministic_rng(12345, 99);
        assert_eq!(r1, r2);
        assert_ne!(r1, deterministic_rng(12345, 100));
    }

    #[test]
    fn test_water_nearby_detection() {
        let mut map = std::collections::HashMap::new();
        map.insert((10, 64, 10), BlockType::Farmland);
        map.insert((13, 64, 12), BlockType::Water);

        let found = is_water_nearby((10, 64, 10), |x, y, z| map.get(&(x, y, z)).copied());
        assert!(found);

        let not_found = is_water_nearby((10, 64, 10), |x, y, z| {
            if (x, y, z) == (18, 64, 10) {
                Some(BlockType::Water)
            } else {
                None
            }
        });
        assert!(!not_found);
    }

    #[test]
    fn test_farmland_hydration_mutation() {
        let mut map = std::collections::HashMap::new();
        map.insert((0, 64, 0), BlockType::Farmland);
        map.insert((2, 64, 0), BlockType::Water);

        let req = evaluate_random_tick_at((0, 64, 0), BlockType::Farmland, 0, 123, |x, y, z| {
            map.get(&(x, y, z)).copied()
        });

        assert!(req.is_some());
        let r = req.unwrap();
        assert_eq!(r.new_block, BlockType::Farmland);
        assert_eq!(r.new_state, 1);
    }

    #[test]
    fn test_farmland_degradation_when_covered() {
        let mut map = std::collections::HashMap::new();
        map.insert((0, 64, 0), BlockType::Farmland);
        map.insert((0, 65, 0), BlockType::Stone);

        let req = evaluate_random_tick_at((0, 64, 0), BlockType::Farmland, 7, 123, |x, y, z| {
            map.get(&(x, y, z)).copied()
        });

        assert!(req.is_some());
        let r = req.unwrap();
        assert_eq!(r.new_block, BlockType::Dirt);
        assert_eq!(r.new_state, 0);
    }
}
