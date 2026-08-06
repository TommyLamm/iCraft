use crate::chunk_manager::ChunkManager;
use crate::inventory::ItemStack;
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
        BlockType::Grass => {
            let block_above = get_block(x, y + 1, z).unwrap_or(BlockType::Air);
            if block_above.properties().is_opaque() {
                return Some(BlockMutationRequest {
                    pos,
                    new_block: BlockType::Dirt,
                    new_state: 0,
                    new_entity: None,
                    cause: MutationCause::System,
                });
            }
            // Grass spread to adjacent dirt block
            if rng_val % 4 == 0 {
                let dx = ((rng_val >> 12) % 3) as i32 - 1;
                let dz = ((rng_val >> 14) % 3) as i32 - 1;
                let dy = ((rng_val >> 16) % 5) as i32 - 3;
                let target_pos = (x + dx, y + dy, z + dz);
                if get_block(target_pos.0, target_pos.1, target_pos.2) == Some(BlockType::Dirt) {
                    let target_above = get_block(target_pos.0, target_pos.1 + 1, target_pos.2)
                        .unwrap_or(BlockType::Air);
                    if !target_above.properties().is_opaque() {
                        return Some(BlockMutationRequest {
                            pos: target_pos,
                            new_block: BlockType::Grass,
                            new_state: 0,
                            new_entity: None,
                            cause: MutationCause::System,
                        });
                    }
                }
            }
            None
        }
        BlockType::OakLeaves | BlockType::BirchLeaves | BlockType::SpruceLeaves => {
            // Leaf decay check: search within Manhattan distance 4 for log
            let mut connected = false;
            'search: for dx in -4..=4i32 {
                for dy in -4..=4i32 {
                    for dz in -4..=4i32 {
                        if dx.abs() + dy.abs() + dz.abs() <= 4 {
                            if let Some(b) = get_block(x + dx, y + dy, z + dz) {
                                if matches!(
                                    b,
                                    BlockType::OakLog | BlockType::BirchLog | BlockType::SpruceLog
                                ) {
                                    connected = true;
                                    break 'search;
                                }
                            }
                        }
                    }
                }
            }
            if !connected {
                Some(BlockMutationRequest {
                    pos,
                    new_block: BlockType::Air,
                    new_state: 0,
                    new_entity: None,
                    cause: MutationCause::System,
                })
            } else {
                None
            }
        }
        BlockType::OakSapling | BlockType::BirchSapling | BlockType::SpruceSapling => {
            if rng_val % 7 == 0 {
                let block_above = get_block(x, y + 1, z).unwrap_or(BlockType::Air);
                if block_above == BlockType::Air {
                    let log_type = match block {
                        BlockType::BirchSapling => BlockType::BirchLog,
                        BlockType::SpruceSapling => BlockType::SpruceLog,
                        _ => BlockType::OakLog,
                    };
                    return Some(BlockMutationRequest {
                        pos,
                        new_block: log_type,
                        new_state: 0,
                        new_entity: None,
                        cause: MutationCause::System,
                    });
                }
            }
            None
        }
        BlockType::Cactus => {
            let mut height = 1;
            let mut check_y = y - 1;
            while get_block(x, check_y, z) == Some(BlockType::Cactus) {
                height += 1;
                check_y -= 1;
            }
            if height < 3 && get_block(x, y + 1, z) == Some(BlockType::Air) && rng_val % 3 == 0 {
                Some(BlockMutationRequest {
                    pos: (x, y + 1, z),
                    new_block: BlockType::Cactus,
                    new_state: 0,
                    new_entity: None,
                    cause: MutationCause::System,
                })
            } else {
                None
            }
        }
        BlockType::SugarCane => {
            let mut height = 1;
            let mut check_y = y - 1;
            while get_block(x, check_y, z) == Some(BlockType::SugarCane) {
                height += 1;
                check_y -= 1;
            }
            if height < 3 && get_block(x, y + 1, z) == Some(BlockType::Air) && rng_val % 3 == 0 {
                Some(BlockMutationRequest {
                    pos: (x, y + 1, z),
                    new_block: BlockType::SugarCane,
                    new_state: 0,
                    new_entity: None,
                    cause: MutationCause::System,
                })
            } else {
                None
            }
        }
        BlockType::Ice => {
            if rng_val % 4 == 0 {
                Some(BlockMutationRequest {
                    pos,
                    new_block: BlockType::Water,
                    new_state: 0,
                    new_entity: None,
                    cause: MutationCause::System,
                })
            } else {
                None
            }
        }
        BlockType::Snow => {
            let block_above = get_block(x, y + 1, z).unwrap_or(BlockType::Air);
            if block_above.properties().is_opaque() {
                Some(BlockMutationRequest {
                    pos,
                    new_block: BlockType::Air,
                    new_state: 0,
                    new_entity: None,
                    cause: MutationCause::System,
                })
            } else {
                None
            }
        }
        BlockType::Fire => {
            if rng_val % 3 == 0 {
                Some(BlockMutationRequest {
                    pos,
                    new_block: BlockType::Air,
                    new_state: 0,
                    new_entity: None,
                    cause: MutationCause::System,
                })
            } else {
                None
            }
        }
        BlockType::Sand | BlockType::Gravel => {
            let below = get_block(x, y - 1, z).unwrap_or(BlockType::Air);
            if below == BlockType::Air {
                Some(BlockMutationRequest {
                    pos,
                    new_block: BlockType::Air,
                    new_state: 0,
                    new_entity: None,
                    cause: MutationCause::System,
                })
            } else {
                None
            }
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
        for (sec_idx, section_opt) in chunk.sections.iter().enumerate() {
            let Some(section) = section_opt else {
                continue;
            };
            if section.random_tick_count() > 0 {
                let sec_y = chunk.section_y_at_index(sec_idx);
                eligible_sections.push((cx, cz, sec_y));
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

/// Upper bound applied even when a caller supplies a larger budget.  This is
/// deliberately small enough that a long hopper chain cannot monopolize the
/// host tick or create an unbounded cascade in one frame.
pub const MAX_HOPPER_TRANSFERS_PER_TICK: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HopperTickResult {
    pub transfers: usize,
    /// Number of loaded source/target container capability checks attempted.
    /// This is separate from `transfers` so a full or sided-incompatible
    /// destination is visible in host telemetry without implying a mutation.
    pub container_checks: usize,
    pub changed_positions: Vec<(i32, i32, i32)>,
    pub budget_exhausted: bool,
}

/// Compatibility wrapper used by focused world-tick tests and callers that do
/// not own an entity manager.  It still uses the same atomic transfer path.
pub fn tick_hoppers(chunk_manager: &mut ChunkManager, max_transfers_per_tick: usize) -> usize {
    tick_hoppers_with_entities(chunk_manager, None, max_transfers_per_tick).transfers
}

/// Ticks active hoppers across loaded chunks.  A transfer is planned against
/// cloned source/target entities and committed only after both sided-capability
/// checks succeed, so a failed destination never consumes the source slot.
/// Dropped item entities are considered after container pulls and are removed
/// or decremented only after the hopper accepts one complete metadata-bearing
/// stack item.
pub fn tick_hoppers_with_entities(
    chunk_manager: &mut ChunkManager,
    mut entity_manager: Option<&mut crate::entity::EntityManager>,
    max_transfers_per_tick: usize,
) -> HopperTickResult {
    use crate::block_entity::BlockEntity;
    use crate::redstone::Direction;

    let budget = max_transfers_per_tick.min(MAX_HOPPER_TRANSFERS_PER_TICK);
    let mut result = HopperTickResult::default();
    let mut hoppers = Vec::new();
    for (&(cx, cz), chunk) in &chunk_manager.chunks {
        for (pos, entity) in &chunk.block_entities {
            if let BlockEntity::Hopper(h) = entity {
                hoppers.push((
                    cx * CHUNK_WIDTH as i32 + pos.0 as i32,
                    pos.1 as i32,
                    cz * CHUNK_DEPTH as i32 + pos.2 as i32,
                    h.facing,
                    h.transfer_cooldown,
                    h.is_powered,
                ));
            }
        }
    }
    hoppers.sort_unstable_by_key(|&(x, y, z, _, _, _)| (x, y, z));

    for (x, y, z, facing, cooldown, is_powered) in hoppers {
        if result.transfers >= budget || is_powered {
            continue;
        }
        if cooldown > 0 {
            if let Some(BlockEntity::Hopper(h)) = chunk_manager.get_block_entity_mut(x, y, z) {
                h.transfer_cooldown = h.transfer_cooldown.saturating_sub(1);
                // Cooldown is authoritative runtime/save state, but it is not a
                // container slot mutation.  Keep the chunk dirty for persistence
                // without waking comparators or broadcasting an update every tick.
                chunk_manager.mark_block_entity_dirty(x, z);
            }
            continue;
        }

        let mut transferred = false;
        let delta = facing.delta();
        let target_pos = (x + delta.0, y + delta.1, z + delta.2);
        if chunk_manager.is_block_loaded(target_pos.0, target_pos.1, target_pos.2) {
            let source = chunk_manager.get_block_entity(x, y, z).cloned();
            let target = chunk_manager
                .get_block_entity(target_pos.0, target_pos.1, target_pos.2)
                .cloned();
            if let (Some(source), Some(target)) = (source, target) {
                result.container_checks = result.container_checks.saturating_add(1);
                if let Some((source_after, target_after)) =
                    transfer_one(&source, Some(facing), &target, Some(facing.opposite()))
                {
                    chunk_manager.set_block_entity(x, y, z, Some(source_after));
                    chunk_manager.set_block_entity(
                        target_pos.0,
                        target_pos.1,
                        target_pos.2,
                        Some(target_after),
                    );
                    result.changed_positions.push((x, y, z));
                    result.changed_positions.push(target_pos);
                    transferred = true;
                }
            }
        }

        if !transferred {
            let above_pos = (x, y + 1, z);
            if chunk_manager.is_block_loaded(above_pos.0, above_pos.1, above_pos.2) {
                let source = chunk_manager
                    .get_block_entity(above_pos.0, above_pos.1, above_pos.2)
                    .cloned();
                let target = chunk_manager.get_block_entity(x, y, z).cloned();
                if let (Some(source), Some(target)) = (source, target) {
                    result.container_checks = result.container_checks.saturating_add(1);
                    if let Some((source_after, target_after)) =
                        transfer_one(&source, Some(Direction::Down), &target, Some(Direction::Up))
                    {
                        chunk_manager.set_block_entity(
                            above_pos.0,
                            above_pos.1,
                            above_pos.2,
                            Some(source_after),
                        );
                        chunk_manager.set_block_entity(x, y, z, Some(target_after));
                        result.changed_positions.push(above_pos);
                        result.changed_positions.push((x, y, z));
                        transferred = true;
                    }
                }
            }
        }

        if !transferred {
            // Dropped items are deterministic by entity id.  Only the small
            // pickup volume immediately above the hopper is considered.
            let candidate = entity_manager.as_deref().and_then(|entities| {
                entities
                    .entities
                    .iter()
                    .filter(|entity| {
                        entity.entity_type == crate::entity::EntityType::DroppedItem
                            && entity.pickup_cooldown <= 0.0
                            && entity.position.x >= x as f32 - 0.5
                            && entity.position.x <= x as f32 + 1.5
                            && entity.position.z >= z as f32 - 0.5
                            && entity.position.z <= z as f32 + 1.5
                            && entity.position.y >= y as f32 + 0.75
                            && entity.position.y <= y as f32 + 2.5
                    })
                    .min_by_key(|entity| entity.id)
                    .map(|entity| {
                        let stack = entity.dropped_stack.unwrap_or_else(|| {
                            ItemStack::new(
                                entity.dropped_item.unwrap_or(crate::inventory::Item::Air),
                                entity.dropped_count.max(1),
                            )
                        });
                        (entity.id, stack)
                    })
            });
            if let Some((entity_id, stack)) = candidate {
                if let Some(mut hopper) = chunk_manager.get_block_entity(x, y, z).cloned() {
                    let one = ItemStack { count: 1, ..stack };
                    if hopper.try_insert_item(Some(Direction::Up), one) {
                        chunk_manager.set_block_entity(x, y, z, Some(hopper));
                        if let Some(entities) = entity_manager.as_deref_mut() {
                            if stack.count <= 1 {
                                // Removal happens after the successful
                                // insertion, so the entity cannot vanish on a
                                // rejected/full hopper.
                                let _ = entities.remove_by_id(entity_id);
                            } else if let Some(entity) = entities.get_by_id_mut(entity_id) {
                                let remaining = ItemStack {
                                    count: stack.count - 1,
                                    ..stack
                                };
                                entity.dropped_stack = Some(remaining);
                                entity.dropped_item = Some(remaining.item);
                                entity.dropped_count = remaining.count;
                            }
                        }
                        result.changed_positions.push((x, y, z));
                        transferred = true;
                    }
                }
            }
        }

        if transferred {
            result.transfers += 1;
            if let Some(BlockEntity::Hopper(h)) = chunk_manager.get_block_entity_mut(x, y, z) {
                h.transfer_cooldown = 8;
                h.revision = h.revision.wrapping_add(1);
                chunk_manager.mark_block_entity_dirty(x, z);
            }
        }
    }
    result.changed_positions.sort_unstable();
    result.changed_positions.dedup();
    result.budget_exhausted = budget > 0 && result.transfers >= budget;
    result
}

fn transfer_one(
    source: &crate::block_entity::BlockEntity,
    source_side: Option<crate::redstone::Direction>,
    target: &crate::block_entity::BlockEntity,
    target_side: Option<crate::redstone::Direction>,
) -> Option<(
    crate::block_entity::BlockEntity,
    crate::block_entity::BlockEntity,
)> {
    let slot = (0..source.slot_count()).find(|&slot| {
        source.can_extract_item(slot, source_side) && source.get_stack(slot).is_some()
    })?;
    let stack = *source.get_stack(slot)?;
    let one = ItemStack { count: 1, ..stack };
    let mut source_after = source.clone();
    let mut target_after = target.clone();
    if !target_after.try_insert_item(target_side, one) {
        return None;
    }
    source_after.set_stack(
        slot,
        (stack.count > 1).then_some(ItemStack {
            count: stack.count - 1,
            ..stack
        }),
    );
    Some((source_after, target_after))
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

    #[test]
    fn test_leaf_decay_without_log() {
        let map: std::collections::HashMap<(i32, i32, i32), BlockType> =
            std::collections::HashMap::new();
        let req = evaluate_random_tick_at((0, 64, 0), BlockType::OakLeaves, 0, 123, |x, y, z| {
            map.get(&(x, y, z)).copied()
        });
        assert!(req.is_some());
        let r = req.unwrap();
        assert_eq!(r.new_block, BlockType::Air);
    }

    #[test]
    fn test_leaf_preservation_with_log() {
        let mut map = std::collections::HashMap::new();
        map.insert((0, 63, 0), BlockType::OakLog);
        let req = evaluate_random_tick_at((0, 64, 0), BlockType::OakLeaves, 0, 123, |x, y, z| {
            map.get(&(x, y, z)).copied()
        });
        assert!(req.is_none());
    }

    #[test]
    fn test_grass_decay_when_covered() {
        let mut map = std::collections::HashMap::new();
        map.insert((0, 64, 0), BlockType::Grass);
        map.insert((0, 65, 0), BlockType::Stone);
        let req = evaluate_random_tick_at((0, 64, 0), BlockType::Grass, 0, 123, |x, y, z| {
            map.get(&(x, y, z)).copied()
        });
        assert!(req.is_some());
        let r = req.unwrap();
        assert_eq!(r.new_block, BlockType::Dirt);
    }

    #[test]
    fn test_cactus_growth() {
        let mut map = std::collections::HashMap::new();
        map.insert((0, 64, 0), BlockType::Cactus);
        map.insert((0, 65, 0), BlockType::Air);
        let req = evaluate_random_tick_at((0, 64, 0), BlockType::Cactus, 0, 3, |x, y, z| {
            map.get(&(x, y, z)).copied()
        });
        assert!(req.is_some());
        let r = req.unwrap();
        assert_eq!(r.pos, (0, 65, 0));
        assert_eq!(r.new_block, BlockType::Cactus);
    }

    #[test]
    fn test_falling_sand() {
        let map: std::collections::HashMap<(i32, i32, i32), BlockType> =
            std::collections::HashMap::new();
        let req = evaluate_random_tick_at((0, 64, 0), BlockType::Sand, 0, 123, |x, y, z| {
            map.get(&(x, y, z)).copied()
        });
        assert!(req.is_some());
        let r = req.unwrap();
        assert_eq!(r.pos, (0, 64, 0));
        assert_eq!(r.new_block, BlockType::Air);
    }

    #[test]
    fn test_hopper_transfer_chain_smelting() {
        use crate::block_entity::{
            BlockEntity, ChestBlockEntity, FurnaceBlockEntity, HopperBlockEntity,
        };
        use crate::inventory::{Item, ItemStack};
        use crate::redstone::Direction;

        let mut manager = ChunkManager::new(8);
        manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));

        // 1. Top Chest at (0, 66, 0) containing 5 Raw Iron
        manager.set_block(0, 66, 0, BlockType::Chest);
        let mut top_chest = ChestBlockEntity::new();
        top_chest.set_stack(0, Some(ItemStack::new(Item::IronOre, 5)));
        manager.set_block_entity(0, 66, 0, Some(BlockEntity::Chest(top_chest)));

        // 2. Hopper at (0, 65, 0) facing Down
        manager.set_block(0, 65, 0, BlockType::Hopper);
        let top_hopper = HopperBlockEntity::with_facing(Direction::Down);
        manager.set_block_entity(0, 65, 0, Some(BlockEntity::Hopper(top_hopper)));

        // 3. Furnace at (0, 64, 0)
        manager.set_block(0, 64, 0, BlockType::Furnace);
        let mut furnace = FurnaceBlockEntity::new();
        furnace.set_stack(1, Some(ItemStack::new(Item::Coal, 1))); // Fuel
        manager.set_block_entity(0, 64, 0, Some(BlockEntity::Furnace(furnace)));

        // 4. Bottom Hopper at (0, 63, 0) facing Down
        manager.set_block(0, 63, 0, BlockType::Hopper);
        let bot_hopper = HopperBlockEntity::with_facing(Direction::Down);
        manager.set_block_entity(0, 63, 0, Some(BlockEntity::Hopper(bot_hopper)));

        // 5. Bottom Chest at (0, 62, 0)
        manager.set_block(0, 62, 0, BlockType::Chest);
        let bot_chest = ChestBlockEntity::new();
        manager.set_block_entity(0, 62, 0, Some(BlockEntity::Chest(bot_chest)));

        // Execute hopper ticks
        // First tick: top hopper pulls 1 IronOre from top chest
        tick_hoppers(&mut manager, 64);
        let top_h_be = manager.get_block_entity(0, 65, 0).unwrap();
        assert_eq!(
            top_h_be.get_stack(0),
            Some(&ItemStack::new(Item::IronOre, 1))
        );

        // Reset cooldown on top hopper for test tick acceleration
        if let Some(BlockEntity::Hopper(ref mut h)) = manager.get_block_entity_mut(0, 65, 0) {
            h.transfer_cooldown = 0;
        }

        // Second tick: top hopper pushes 1 IronOre into Furnace slot 0
        tick_hoppers(&mut manager, 64);
        let furn_be = manager.get_block_entity(0, 64, 0).unwrap();
        assert_eq!(
            furn_be.get_stack(0),
            Some(&ItemStack::new(Item::IronOre, 1))
        );
    }

    #[test]
    fn hopper_full_chain_smelts_and_conserves_items() {
        use crate::block_entity::{
            BlockEntity, ChestBlockEntity, FurnaceBlockEntity, HopperBlockEntity,
        };
        use crate::inventory::{Item, ItemStack};
        use crate::recipes::RecipeManager;
        use crate::redstone::Direction;

        let mut manager = ChunkManager::new(8);
        manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));
        manager.set_block(0, 66, 0, BlockType::Chest);
        let mut input = ChestBlockEntity::new();
        input.set_stack(0, Some(ItemStack::new(Item::IronOre, 5)));
        manager.set_block_entity(0, 66, 0, Some(BlockEntity::Chest(input)));

        manager.set_block(0, 65, 0, BlockType::Hopper);
        manager.set_block_entity(
            0,
            65,
            0,
            Some(BlockEntity::Hopper(HopperBlockEntity::with_facing(
                Direction::Down,
            ))),
        );

        manager.set_block(0, 64, 0, BlockType::Furnace);
        let mut furnace = FurnaceBlockEntity::new();
        furnace.set_stack(1, Some(ItemStack::new(Item::Coal, 1)));
        manager.set_block_entity(0, 64, 0, Some(BlockEntity::Furnace(furnace)));

        manager.set_block(0, 63, 0, BlockType::Hopper);
        manager.set_block_entity(
            0,
            63,
            0,
            Some(BlockEntity::Hopper(HopperBlockEntity::with_facing(
                Direction::Down,
            ))),
        );
        manager.set_block(0, 62, 0, BlockType::Chest);
        manager.set_block_entity(0, 62, 0, Some(BlockEntity::Chest(ChestBlockEntity::new())));

        let recipes = RecipeManager::new();
        for _ in 0..1800 {
            let _ = tick_hoppers(&mut manager, MAX_HOPPER_TRANSFERS_PER_TICK);
            if let Some(BlockEntity::Furnace(furnace)) = manager.get_block_entity_mut(0, 64, 0) {
                let _ = furnace.tick(&recipes);
            }
        }

        let output = manager.get_block_entity(0, 62, 0).unwrap();
        assert_eq!(
            output.get_stack(0),
            Some(&ItemStack::new(Item::IronIngot, 5)),
            "all five input items must reach the output chest after smelting"
        );
        assert!(manager
            .get_block_entity(0, 66, 0)
            .unwrap()
            .get_stack(0)
            .is_none());
        assert!(manager
            .get_block_entity(0, 64, 0)
            .unwrap()
            .get_stack(1)
            .is_none());
    }

    #[test]
    fn hopper_failed_destination_is_atomic_and_preserves_metadata() {
        use crate::block_entity::{BlockEntity, ChestBlockEntity, HopperBlockEntity};
        use crate::enchantment::Enchantment;
        use crate::inventory::{Item, ItemStack};
        use crate::redstone::Direction;

        let mut manager = ChunkManager::new(2);
        manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));
        manager.set_block(0, 64, 0, BlockType::Hopper);
        let mut hopper = HopperBlockEntity::with_facing(Direction::East);
        let mut payload = ItemStack::new(Item::DiamondPickaxe, 2);
        payload.durability = 42;
        payload
            .enchantments
            .add_or_upgrade(Enchantment::Efficiency(4));
        hopper.slots[0] = Some(payload);
        manager.set_block_entity(0, 64, 0, Some(BlockEntity::Hopper(hopper)));

        manager.set_block(1, 64, 0, BlockType::Chest);
        let mut full = ChestBlockEntity::new();
        for slot in 0..27 {
            full.set_stack(slot, Some(ItemStack::new(Item::Stone, 64)));
        }
        manager.set_block_entity(1, 64, 0, Some(BlockEntity::Chest(full)));

        let result = tick_hoppers(&mut manager, MAX_HOPPER_TRANSFERS_PER_TICK);
        assert_eq!(result, 0);
        assert_eq!(
            manager.get_block_entity(0, 64, 0).unwrap().get_stack(0),
            Some(&payload)
        );
        assert_eq!(
            manager.get_block_entity(1, 64, 0).unwrap().get_stack(0),
            Some(&ItemStack::new(Item::Stone, 64))
        );
    }

    #[test]
    fn hopper_does_not_consume_across_unloaded_chunk_boundary() {
        use crate::block_entity::{BlockEntity, ChestBlockEntity, HopperBlockEntity};
        use crate::inventory::{Item, ItemStack};
        use crate::redstone::Direction;

        let mut manager = ChunkManager::new(2);
        manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));
        manager.set_block(15, 64, 0, BlockType::Hopper);
        let mut hopper = HopperBlockEntity::with_facing(Direction::East);
        hopper.slots[0] = Some(ItemStack::new(Item::Diamond, 2));
        manager.set_block_entity(15, 64, 0, Some(BlockEntity::Hopper(hopper)));

        // x=16 belongs to an unloaded chunk.  The source remains untouched.
        assert_eq!(tick_hoppers(&mut manager, MAX_HOPPER_TRANSFERS_PER_TICK), 0);
        assert_eq!(
            manager
                .get_block_entity(15, 64, 0)
                .and_then(|entity| entity.get_stack(0)),
            Some(&ItemStack::new(Item::Diamond, 2))
        );

        manager
            .chunks
            .insert((1, 0), crate::world::Chunk::new(1, 0));
        manager.set_block(16, 64, 0, BlockType::Chest);
        manager.set_block_entity(16, 64, 0, Some(BlockEntity::Chest(ChestBlockEntity::new())));
        assert_eq!(tick_hoppers(&mut manager, MAX_HOPPER_TRANSFERS_PER_TICK), 1);
        assert_eq!(
            manager
                .get_block_entity(16, 64, 0)
                .and_then(|entity| entity.get_stack(0)),
            Some(&ItemStack::new(Item::Diamond, 1))
        );
    }

    #[test]
    fn hopper_cycle_obeys_transfer_budget_and_sorted_order() {
        use crate::block_entity::{BlockEntity, HopperBlockEntity};
        use crate::inventory::{Item, ItemStack};
        use crate::redstone::Direction;

        let mut manager = ChunkManager::new(2);
        manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));
        manager.set_block(0, 64, 0, BlockType::Hopper);
        manager.set_block(1, 64, 0, BlockType::Hopper);
        let mut left = HopperBlockEntity::with_facing(Direction::East);
        left.slots[0] = Some(ItemStack::new(Item::Stone, 1));
        manager.set_block_entity(0, 64, 0, Some(BlockEntity::Hopper(left)));
        manager.set_block_entity(
            1,
            64,
            0,
            Some(BlockEntity::Hopper(HopperBlockEntity::with_facing(
                Direction::West,
            ))),
        );

        let result = tick_hoppers(&mut manager, 1);
        assert_eq!(result, 1);
        assert!(manager
            .get_block_entity(0, 64, 0)
            .unwrap()
            .get_stack(0)
            .is_none());
        assert_eq!(
            manager.get_block_entity(1, 64, 0).unwrap().get_stack(0),
            Some(&ItemStack::new(Item::Stone, 1))
        );
    }

    #[test]
    fn powered_hopper_is_disabled_without_consuming_source() {
        use crate::block_entity::{BlockEntity, ChestBlockEntity, HopperBlockEntity};
        use crate::inventory::{Item, ItemStack};
        use crate::redstone::Direction;

        let mut manager = ChunkManager::new(2);
        manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));
        manager.set_block(0, 64, 0, BlockType::Hopper);
        let mut hopper = HopperBlockEntity::with_facing(Direction::East);
        hopper.is_powered = true;
        hopper.slots[0] = Some(ItemStack::new(Item::Stone, 1));
        manager.set_block_entity(0, 64, 0, Some(BlockEntity::Hopper(hopper)));
        manager.set_block(1, 64, 0, BlockType::Chest);
        manager.set_block_entity(1, 64, 0, Some(BlockEntity::Chest(ChestBlockEntity::new())));

        assert_eq!(tick_hoppers(&mut manager, MAX_HOPPER_TRANSFERS_PER_TICK), 0);
        assert_eq!(
            manager.get_block_entity(0, 64, 0).unwrap().get_stack(0),
            Some(&ItemStack::new(Item::Stone, 1))
        );
    }

    #[test]
    fn hopper_pulls_one_dropped_item_with_full_stack_metadata() {
        use crate::block_entity::{BlockEntity, HopperBlockEntity};
        use crate::enchantment::ItemName;
        use crate::entity::{EntityManager, EntityType};
        use crate::inventory::{Item, ItemStack};
        use crate::redstone::Direction;

        let mut manager = ChunkManager::new(2);
        manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));
        manager.set_block(0, 64, 0, BlockType::Hopper);
        manager.set_block_entity(
            0,
            64,
            0,
            Some(BlockEntity::Hopper(HopperBlockEntity::with_facing(
                Direction::Down,
            ))),
        );

        let mut entities = EntityManager::new();
        let id = entities.spawn(EntityType::DroppedItem, glam::Vec3::new(0.5, 65.5, 0.5));
        let mut stack = ItemStack::new(Item::SplashPotion, 3);
        let mut name = ItemName::default();
        name.set("drop payload");
        stack.custom_name = name;
        if let Some(entity) = entities.get_by_id_mut(id) {
            entity.dropped_stack = Some(stack);
            entity.dropped_item = Some(stack.item);
            entity.dropped_count = stack.count;
        }

        let result = tick_hoppers_with_entities(&mut manager, Some(&mut entities), 1);
        assert_eq!(result.transfers, 1);
        assert_eq!(
            manager.get_block_entity(0, 64, 0).unwrap().get_stack(0),
            Some(&ItemStack { count: 1, ..stack })
        );
        let remaining = entities.get_by_id(id).unwrap();
        assert_eq!(
            remaining.dropped_stack,
            Some(ItemStack { count: 2, ..stack })
        );
    }
}
