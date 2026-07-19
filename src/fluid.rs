use crate::chunk_manager::ChunkManager;
use crate::world::{BlockType, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
use std::collections::HashSet;

type BlockPos = (i32, i32, i32);

const HORIZONTAL_DIRECTIONS: [(i32, i32, i32); 4] = [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)];

/// Advances only fluid cells affected by a block change. Work is capped so a
/// large flow can span several frames without blocking rendering.
pub fn tick_fluids(
    chunk_manager: &mut ChunkManager,
    is_lava: bool,
    max_updates: usize,
) -> HashSet<(i32, i32)> {
    let mut dirty_chunks = HashSet::new();
    let target_type = if is_lava {
        BlockType::Lava
    } else {
        BlockType::Water
    };
    let other_type = if is_lava {
        BlockType::Water
    } else {
        BlockType::Lava
    };

    for _ in 0..max_updates {
        let Some((wx, wy, wz)) = chunk_manager.pop_fluid_update(is_lava) else {
            break;
        };

        if wy < 0 || wy >= CHUNK_HEIGHT as i32 {
            continue;
        }
        let cx = wx.div_euclid(CHUNK_WIDTH as i32);
        let cz = wz.div_euclid(CHUNK_DEPTH as i32);
        if !chunk_manager.chunks.contains_key(&(cx, cz)) {
            continue;
        }

        if update_cell(
            chunk_manager,
            (wx, wy, wz),
            target_type,
            other_type,
            is_lava,
        ) {
            mark_mesh_boundaries_dirty(&mut dirty_chunks, wx, wz);
        }
    }

    dirty_chunks
}

fn update_cell(
    chunk_manager: &mut ChunkManager,
    pos: BlockPos,
    target_type: BlockType,
    other_type: BlockType,
    is_lava: bool,
) -> bool {
    let (wx, wy, wz) = pos;
    let current = chunk_manager.get_block(wx, wy, wz);

    // Level-zero, non-falling cells are permanent sources (terrain-generated
    // oceans and player-placed buckets). They require no periodic work.
    if current == target_type
        && chunk_manager.get_fluid_level(wx, wy, wz) == 0
        && !chunk_manager.get_fluid_falling(wx, wy, wz)
    {
        return false;
    }

    let desired = desired_flow(chunk_manager, pos, target_type, is_lava);

    if current == target_type {
        return match desired {
            Some((level, falling)) => {
                set_fluid_state(chunk_manager, pos, target_type, level, falling)
            }
            None => {
                chunk_manager.set_block(wx, wy, wz, BlockType::Air);
                chunk_manager.set_fluid_level(wx, wy, wz, 0);
                chunk_manager.set_fluid_falling(wx, wy, wz, false);
                true
            }
        };
    }

    let replaceable =
        current == BlockType::Air || current == other_type || current.properties().is_passable;
    if !replaceable {
        return false;
    }

    let Some((level, falling)) = desired else {
        return false;
    };

    if current == other_type {
        let other_is_source = chunk_manager.get_fluid_level(wx, wy, wz) == 0
            && !chunk_manager.get_fluid_falling(wx, wy, wz);
        let solid = if target_type == BlockType::Water {
            if other_is_source {
                BlockType::Obsidian
            } else {
                BlockType::Cobblestone
            }
        } else if other_is_source {
            BlockType::Stone
        } else {
            BlockType::Cobblestone
        };
        chunk_manager.set_block(wx, wy, wz, solid);
        chunk_manager.set_fluid_level(wx, wy, wz, 0);
        chunk_manager.set_fluid_falling(wx, wy, wz, false);
        return true;
    }

    set_fluid_state(chunk_manager, pos, target_type, level, falling)
}

fn desired_flow(
    chunk_manager: &ChunkManager,
    (wx, wy, wz): BlockPos,
    target_type: BlockType,
    is_lava: bool,
) -> Option<(u8, bool)> {
    if wy + 1 < CHUNK_HEIGHT as i32 && chunk_manager.get_block(wx, wy + 1, wz) == target_type {
        return Some((0, true));
    }

    // Two adjacent source blocks above a supporting block create an infinite
    // water source. Lava intentionally does not use this rule.
    if !is_lava && is_supported(chunk_manager, wx, wy, wz, target_type) {
        let source_count = HORIZONTAL_DIRECTIONS
            .iter()
            .filter(|&&(dx, _, dz)| {
                chunk_manager.get_block(wx + dx, wy, wz + dz) == BlockType::Water
                    && chunk_manager.get_fluid_level(wx + dx, wy, wz + dz) == 0
                    && !chunk_manager.get_fluid_falling(wx + dx, wy, wz + dz)
            })
            .count();
        if source_count >= 2 {
            return Some((0, false));
        }
    }

    let mut best_level = None;
    for (dx, _, dz) in HORIZONTAL_DIRECTIONS {
        let nx = wx + dx;
        let nz = wz + dz;
        if chunk_manager.get_block(nx, wy, nz) != target_type {
            continue;
        }

        let neighbor_level = chunk_manager.get_fluid_level(nx, wy, nz);
        let neighbor_falling = chunk_manager.get_fluid_falling(nx, wy, nz);
        if neighbor_level >= 7 {
            continue;
        }

        // A falling column spreads sideways only after it reaches a surface.
        if neighbor_falling && !is_supported(chunk_manager, nx, wy, nz, target_type) {
            continue;
        }

        best_level = Some(best_level.map_or(neighbor_level, |best: u8| best.min(neighbor_level)));
    }

    best_level.map(|level| (level + 1, false))
}

fn is_supported(
    chunk_manager: &ChunkManager,
    wx: i32,
    wy: i32,
    wz: i32,
    fluid_type: BlockType,
) -> bool {
    if wy == 0 {
        return true;
    }
    let below = chunk_manager.get_block(wx, wy - 1, wz);
    below != BlockType::Air && below != fluid_type && !below.properties().is_passable
}

fn set_fluid_state(
    chunk_manager: &mut ChunkManager,
    (wx, wy, wz): BlockPos,
    fluid_type: BlockType,
    level: u8,
    falling: bool,
) -> bool {
    let changed = chunk_manager.get_block(wx, wy, wz) != fluid_type
        || chunk_manager.get_fluid_level(wx, wy, wz) != level
        || chunk_manager.get_fluid_falling(wx, wy, wz) != falling;
    if !changed {
        return false;
    }

    chunk_manager.set_block(wx, wy, wz, fluid_type);
    chunk_manager.set_fluid_level(wx, wy, wz, level);
    chunk_manager.set_fluid_falling(wx, wy, wz, falling);
    true
}

fn mark_mesh_boundaries_dirty(dirty: &mut HashSet<(i32, i32)>, wx: i32, wz: i32) {
    let cx = wx.div_euclid(CHUNK_WIDTH as i32);
    let cz = wz.div_euclid(CHUNK_DEPTH as i32);
    let lx = wx.rem_euclid(CHUNK_WIDTH as i32);
    let lz = wz.rem_euclid(CHUNK_DEPTH as i32);
    dirty.insert((cx, cz));
    if lx == 0 {
        dirty.insert((cx - 1, cz));
    }
    if lx == CHUNK_WIDTH as i32 - 1 {
        dirty.insert((cx + 1, cz));
    }
    if lz == 0 {
        dirty.insert((cx, cz - 1));
    }
    if lz == CHUNK_DEPTH as i32 - 1 {
        dirty.insert((cx, cz + 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;

    #[test]
    fn generated_chunks_do_not_schedule_the_static_ocean() {
        let mut manager = ChunkManager::new(1);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));

        assert_eq!(manager.pending_fluid_updates(false), 0);
        assert!(tick_fluids(&mut manager, false, 64).is_empty());
        assert_eq!(manager.pending_fluid_updates(false), 0);
    }

    #[test]
    fn placed_source_flows_without_scanning_the_world() {
        let mut manager = ChunkManager::new(1);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        let source = (8, 120, 8);
        manager.set_block(source.0, source.1, source.2, BlockType::Water);

        let dirty = tick_fluids(&mut manager, false, 128);

        assert!(!dirty.is_empty());
        assert_eq!(manager.get_block(8, 119, 8), BlockType::Water);
        assert!(manager.get_fluid_falling(8, 119, 8));
    }

    #[test]
    fn removing_a_source_drains_its_incremental_flow() {
        let mut manager = ChunkManager::new(1);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        let source = (8, 120, 8);
        manager.set_block(source.0, source.1, source.2, BlockType::Water);

        for _ in 0..32 {
            tick_fluids(&mut manager, false, 256);
            if manager.pending_fluid_updates(false) == 0 {
                break;
            }
        }
        assert_eq!(manager.get_block(8, 119, 8), BlockType::Water);

        manager.set_block(source.0, source.1, source.2, BlockType::Air);
        for _ in 0..2048 {
            tick_fluids(&mut manager, false, 256);
            if manager.pending_fluid_updates(false) == 0 {
                break;
            }
        }

        assert_eq!(manager.pending_fluid_updates(false), 0);
        assert_eq!(manager.get_block(8, 119, 8), BlockType::Air);
    }
}
