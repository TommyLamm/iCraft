use crate::block_entity::BlockEntity;
use crate::chunk_manager::{mark_block_mesh_dependencies, ChunkManager};
use crate::world::BlockType;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationCause {
    PlayerPlace { player_id: Option<u64> },
    PlayerBreak { player_id: Option<u64> },
    Redstone,
    Explosion,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockMutationRequest {
    pub pos: (i32, i32, i32),
    pub new_block: BlockType,
    pub new_state: u8,
    pub new_entity: Option<BlockEntity>,
    pub cause: MutationCause,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleMutationResult {
    pub pos: (i32, i32, i32),
    pub old_block: BlockType,
    pub old_state: u8,
    pub old_entity: Option<BlockEntity>,
    pub new_block: BlockType,
    pub new_state: u8,
    pub new_entity: Option<BlockEntity>,
    pub chunk_pos: (i32, i32),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockMutationOutcome {
    pub mutations: Vec<SingleMutationResult>,
    pub dirty_chunks: HashSet<(i32, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationError {
    EmptyBatch,
    OutOfBounds { pos: (i32, i32, i32) },
    ChunkNotLoaded { chunk_pos: (i32, i32) },
    EntityMismatch { pos: (i32, i32, i32) },
}

impl std::fmt::Display for MutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MutationError::EmptyBatch => write!(f, "empty mutation batch"),
            MutationError::OutOfBounds { pos } => {
                write!(f, "mutation position {pos:?} out of bounds")
            }
            MutationError::ChunkNotLoaded { chunk_pos } => {
                write!(f, "target chunk {chunk_pos:?} not loaded")
            }
            MutationError::EntityMismatch { pos } => write!(f, "block entity mismatch at {pos:?}"),
        }
    }
}

impl std::error::Error for MutationError {}

/// Atomically validates a batch of block mutation requests.
/// Verification succeeds if all target chunks exist, positions are within world bounds,
/// and block entity types match the target block types.
pub fn validate_batch(
    chunk_manager: &ChunkManager,
    requests: &[BlockMutationRequest],
) -> Result<(), MutationError> {
    if requests.is_empty() {
        return Err(MutationError::EmptyBatch);
    }

    for req in requests {
        let (x, y, z) = req.pos;
        let height = chunk_manager.dimension.height();
        if !height.contains_y(y) {
            return Err(MutationError::OutOfBounds { pos: req.pos });
        }

        let (cx, cz) = (
            x.div_euclid(crate::world::CHUNK_WIDTH as i32),
            z.div_euclid(crate::world::CHUNK_DEPTH as i32),
        );

        if !chunk_manager.chunks.contains_key(&(cx, cz)) {
            return Err(MutationError::ChunkNotLoaded {
                chunk_pos: (cx, cz),
            });
        }

        let effective_entity = req
            .new_entity
            .clone()
            .or_else(|| crate::block_entity::default_stub_for_block(req.new_block));

        if let Some(ref entity) = effective_entity {
            if !entity.matches_block_type(req.new_block) {
                return Err(MutationError::EntityMismatch { pos: req.pos });
            }
        }
    }

    Ok(())
}

/// Atomically applies a batch of block mutation requests.
/// Performs pre-validation first. If validation fails, no changes are committed.
/// Otherwise, updates block types, states, block entities, lighting, heightmaps,
/// mesh invalidation dependencies, support cascades, and dirty chunk flags across all affected chunks.
pub fn apply_batch(
    chunk_manager: &mut ChunkManager,
    requests: Vec<BlockMutationRequest>,
) -> Result<BlockMutationOutcome, MutationError> {
    validate_batch(chunk_manager, &requests)?;

    let mut outcome = BlockMutationOutcome::default();

    for req in requests {
        let (x, y, z) = req.pos;
        let (cx, cz) = (
            x.div_euclid(crate::world::CHUNK_WIDTH as i32),
            z.div_euclid(crate::world::CHUNK_DEPTH as i32),
        );
        let (bx, by, bz) = (
            x.rem_euclid(crate::world::CHUNK_WIDTH as i32) as u8,
            y as i16,
            z.rem_euclid(crate::world::CHUNK_DEPTH as i32) as u8,
        );

        let old_block = chunk_manager.get_block(x, y, z);
        let old_state = chunk_manager.get_block_state(x, y, z);
        let old_entity = chunk_manager
            .chunks
            .get(&(cx, cz))
            .and_then(|c| c.get_block_entity(bx, by, bz).cloned());

        let effective_entity = req
            .new_entity
            .clone()
            .or_else(|| crate::block_entity::default_stub_for_block(req.new_block));

        // Apply block & state
        chunk_manager.set_block(x, y, z, req.new_block);
        chunk_manager.set_block_state(x, y, z, req.new_state);

        // Apply entity
        if let Some(chunk) = chunk_manager.chunks.get_mut(&(cx, cz)) {
            if let Some(new_entity) = &effective_entity {
                let _ = chunk.insert_block_entity(bx, by, bz, new_entity.clone());
            } else if old_entity.is_some() && old_block != req.new_block {
                chunk.remove_block_entity(bx, by, bz);
            }
        }

        let new_entity = chunk_manager
            .chunks
            .get(&(cx, cz))
            .and_then(|c| c.get_block_entity(bx, by, bz).cloned());

        // Lighting updates
        let old_props = old_block.properties();
        let new_props = req.new_block.properties();

        if old_props.is_opaque() != new_props.is_opaque() {
            if new_props.is_opaque() {
                crate::lighting::update_sky_light_after_placed(
                    chunk_manager,
                    x,
                    y,
                    z,
                    &mut outcome.dirty_chunks,
                );
            } else {
                crate::lighting::update_sky_light_after_removed(
                    chunk_manager,
                    x,
                    y,
                    z,
                    &mut outcome.dirty_chunks,
                );
            }
        }
        // #region agent log
        {
            use crate::world::RenderType;
            let old_opaque = old_props.render_type == RenderType::Opaque;
            let new_opaque = new_props.render_type == RenderType::Opaque;
            if old_props.is_solid == new_props.is_solid && old_opaque != new_opaque {
                use std::io::Write;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("debug-879839.log")
                    .and_then(|mut f| {
                        writeln!(
                            f,
                            "{{\"sessionId\":\"879839\",\"hypothesisId\":\"B\",\"location\":\"world_mutation.rs:apply_batch\",\"message\":\"opacity changed with same is_solid\",\"data\":{{\"pos\":[{},{},{}],\"old_solid\":{},\"new_solid\":{},\"old_opaque\":{},\"new_opaque\":{}}},\"timestamp\":{}}}",
                            x, y, z, old_props.is_solid, new_props.is_solid, old_opaque, new_opaque, ts
                        )
                    });
            }
        }
        // #endregion

        if old_props.light_emission != new_props.light_emission {
            crate::lighting::update_block_light_after_removed(
                chunk_manager,
                x,
                y,
                z,
                old_props.light_emission,
                &mut outcome.dirty_chunks,
            );
            if new_props.light_emission > 0 {
                crate::lighting::update_block_light_after_placed(
                    chunk_manager,
                    x,
                    y,
                    z,
                    new_props.light_emission,
                    &mut outcome.dirty_chunks,
                );
            }
        }

        // Support cascade check
        chunk_manager.check_and_break_unsupported_above(
            x,
            y,
            z,
            &mut outcome.dirty_chunks,
            |pos, broken_block| {
                let (bcx, bcz) = (
                    pos.0.div_euclid(crate::world::CHUNK_WIDTH as i32),
                    pos.2.div_euclid(crate::world::CHUNK_DEPTH as i32),
                );
                outcome.mutations.push(SingleMutationResult {
                    pos,
                    old_block: broken_block,
                    old_state: 0,
                    old_entity: None,
                    new_block: BlockType::Air,
                    new_state: 0,
                    new_entity: None,
                    chunk_pos: (bcx, bcz),
                });
            },
        );

        mark_block_mesh_dependencies(&mut outcome.dirty_chunks, x, z);
        outcome.dirty_chunks.insert((cx, cz));

        outcome.mutations.push(SingleMutationResult {
            pos: req.pos,
            old_block,
            old_state,
            old_entity,
            new_block: req.new_block,
            new_state: req.new_state,
            new_entity,
            chunk_pos: (cx, cz),
        });
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_entity::ChestBlockEntity;
    use crate::world::Chunk;

    #[test]
    fn test_batch_mutation_all_or_nothing() {
        let mut manager = ChunkManager::new(8);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        // Note: Chunk (1, 0) is deliberately NOT added

        let initial_block = manager.get_block(0, 64, 0);

        let valid_req = BlockMutationRequest {
            pos: (0, 64, 0),
            new_block: BlockType::Stone,
            new_state: 0,
            new_entity: None,
            cause: MutationCause::System,
        };

        let invalid_req = BlockMutationRequest {
            pos: (16, 64, 0), // Falls into unloaded Chunk (1, 0)
            new_block: BlockType::Stone,
            new_state: 0,
            new_entity: None,
            cause: MutationCause::System,
        };

        // Batch with one invalid request must fail completely
        let res = apply_batch(&mut manager, vec![valid_req, invalid_req]);
        assert!(matches!(
            res,
            Err(MutationError::ChunkNotLoaded { chunk_pos: (1, 0) })
        ));

        // Verify that (0, 64, 0) was NOT mutated!
        assert_eq!(manager.get_block(0, 64, 0), initial_block);
    }

    #[test]
    fn test_batch_mutation_with_block_entity() {
        let mut manager = ChunkManager::new(8);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));

        let chest_stub = BlockEntity::Chest(ChestBlockEntity {
            inventory: crate::inventory::ContainerInventory::new(),
            custom_name: None,
            loot_table: None,
            loot_seed: None,
            revision: 0,
        });
        let req = BlockMutationRequest {
            pos: (2, 64, 2),
            new_block: BlockType::Chest,
            new_state: 1,
            new_entity: Some(chest_stub.clone()),
            cause: MutationCause::PlayerPlace { player_id: Some(1) },
        };

        let res = apply_batch(&mut manager, vec![req]);
        assert!(res.is_ok());
        let outcome = res.unwrap();
        assert_eq!(outcome.mutations.len(), 1);
        assert_eq!(outcome.mutations[0].new_block, BlockType::Chest);
        assert_eq!(outcome.mutations[0].new_entity, Some(chest_stub.clone()));

        let chunk = manager.chunks.get(&(0, 0)).unwrap();
        assert_eq!(chunk.get_block_entity(2, 64, 2), Some(&chest_stub));
    }

    #[test]
    fn debug_stone_to_glass_skips_sky_update() {
        let mut manager = ChunkManager::new(8);
        let mut chunk = Chunk::empty(0, 0);
        let height = crate::dimension::WorldHeight::OVERWORLD;
        for x in 0..crate::world::CHUNK_WIDTH {
            for z in 0..crate::world::CHUNK_DEPTH {
                for wy in height.min_y()..height.max_y_exclusive() {
                    chunk.set_block_local(x, wy, z, BlockType::Air);
                    chunk.set_sky_light(x, wy, z, 15);
                    chunk.set_block_light(x, wy, z, 0);
                }
            }
        }
        for x in 0..crate::world::CHUNK_WIDTH {
            for z in 0..crate::world::CHUNK_DEPTH {
                chunk.set_block_local(x, 80, z, BlockType::Stone);
            }
        }
        manager.chunks.insert((0, 0), chunk);
        let mut dirty = std::collections::HashSet::new();
        for x in 0..crate::world::CHUNK_WIDTH {
            for z in 0..crate::world::CHUNK_DEPTH {
                crate::lighting::update_sky_light_after_placed(
                    &mut manager,
                    x as i32,
                    80,
                    z as i32,
                    &mut dirty,
                );
            }
        }

        let before = manager.get_sky_light(8, 79, 8);
        let req = BlockMutationRequest {
            pos: (8, 80, 8),
            new_block: BlockType::Glass,
            new_state: 0,
            new_entity: None,
            cause: MutationCause::System,
        };
        let outcome = apply_batch(&mut manager, vec![req]).unwrap();
        let after = manager.get_sky_light(8, 79, 8);
        // #region agent log
        {
            use std::io::Write;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("debug-879839.log")
                .and_then(|mut f| {
                    writeln!(
                        f,
                        "{{\"sessionId\":\"879839\",\"hypothesisId\":\"B\",\"location\":\"world_mutation.rs:debug_stone_to_glass\",\"message\":\"sky below glass after swap\",\"data\":{{\"before\":{},\"after\":{},\"dirty\":{}}},\"timestamp\":{}}}",
                        before, after, outcome.dirty_chunks.len(), ts
                    )
                });
        }
        // #endregion
        let _ = (before, after, outcome);
        assert_eq!(before, 0);
        assert_eq!(after, 15);
    }
}
