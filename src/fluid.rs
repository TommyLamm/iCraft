use crate::chunk_manager::{mark_block_mesh_dependencies, ChunkManager};
use crate::world::{BlockType, CHUNK_DEPTH, CHUNK_WIDTH, FLUID_LEVEL_MASK};
use std::collections::{BTreeSet, HashSet};

type BlockPos = (i32, i32, i32);

const HORIZONTAL_DIRECTIONS: [(i32, i32, i32); 4] = [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)];

/// A deterministic fluid-cell mutation.  The raw byte is included even when
/// the block type is unchanged so level/falling/waterlogged transitions cannot
/// disappear between the authority and its network projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidMutation {
    pub position: BlockPos,
    pub block: BlockType,
    pub raw_fluid: u8,
}

/// Advances only fluid cells affected by a block change. Work is capped so a
/// large flow can span several frames without blocking rendering.
///
/// Returns the dirty chunk coordinates and the list of raw fluid mutations
/// applied this tick. The mutation list lets a multiplayer host broadcast the
/// exact cells that changed so connected clients render the same flow without
/// running the fluid simulation themselves.
pub fn tick_fluids(
    chunk_manager: &mut ChunkManager,
    is_lava: bool,
    max_updates: usize,
) -> (HashSet<(i32, i32)>, Vec<FluidMutation>) {
    tick_fluids_in_columns(chunk_manager, is_lava, max_updates, None)
}

pub fn tick_fluids_in_columns(
    chunk_manager: &mut ChunkManager,
    is_lava: bool,
    max_updates: usize,
    columns: Option<&BTreeSet<(i32, i32)>>,
) -> (HashSet<(i32, i32)>, Vec<FluidMutation>) {
    let mut dirty_chunks = HashSet::new();
    let mut mutations: Vec<FluidMutation> = Vec::new();
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

        let height = chunk_manager.dimension.height();
        if !height.contains_y(wy) {
            continue;
        }
        let cx = wx.div_euclid(CHUNK_WIDTH as i32);
        let cz = wz.div_euclid(CHUNK_DEPTH as i32);
        if !chunk_manager.chunks.contains_key(&(cx, cz)) {
            continue;
        }
        if columns.is_some_and(|allowed| !allowed.contains(&(cx, cz))) {
            continue;
        }

        let before = (
            chunk_manager.get_block(wx, wy, wz),
            chunk_manager.get_fluid_raw(wx, wy, wz),
        );
        if update_cell(
            chunk_manager,
            (wx, wy, wz),
            target_type,
            other_type,
            is_lava,
        ) {
            mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
            let after = (
                chunk_manager.get_block(wx, wy, wz),
                chunk_manager.get_fluid_raw(wx, wy, wz),
            );
            if after != before {
                mutations.push(FluidMutation {
                    position: (wx, wy, wz),
                    block: after.0,
                    raw_fluid: after.1,
                });
            }
        }
    }

    (dirty_chunks, mutations)
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

    // A waterlogged slab is a solid host and remains in place while acting as
    // a water source for neighboring cells. It is never replaced by ordinary
    // flow (and lava cannot clear it).
    if !is_lava && chunk_manager.is_waterlogged(wx, wy, wz) {
        return false;
    }

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
        (current == BlockType::Air || current == other_type || current.properties().is_passable)
            && !chunk_manager.is_waterlogged(wx, wy, wz);
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
    if chunk_manager.dimension.height().contains_y(wy + 1)
        && is_water_source_at(chunk_manager, (wx, wy + 1, wz), target_type, is_lava)
    {
        return Some((0, true));
    }

    // Two adjacent source blocks above a supporting block create an infinite
    // water source. Lava intentionally does not use this rule.
    if !is_lava && is_supported(chunk_manager, wx, wy, wz, target_type) {
        let source_count = HORIZONTAL_DIRECTIONS
            .iter()
            .filter(|&&(dx, _, dz)| {
                is_water_source_at(
                    chunk_manager,
                    (wx + dx, wy, wz + dz),
                    BlockType::Water,
                    false,
                )
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
        if !is_water_source_at(chunk_manager, (nx, wy, nz), target_type, is_lava) {
            continue;
        }

        let neighbor_level = if !is_lava && chunk_manager.is_waterlogged(nx, wy, nz) {
            0
        } else {
            chunk_manager.get_fluid_level(nx, wy, nz)
        };
        let neighbor_falling = if !is_lava && chunk_manager.is_waterlogged(nx, wy, nz) {
            false
        } else {
            chunk_manager.get_fluid_falling(nx, wy, nz)
        };
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

fn is_water_source_at(
    chunk_manager: &ChunkManager,
    pos: BlockPos,
    target_type: BlockType,
    is_lava: bool,
) -> bool {
    let (wx, wy, wz) = pos;
    if !is_lava && target_type == BlockType::Water && chunk_manager.is_waterlogged(wx, wy, wz) {
        return true;
    }
    chunk_manager.get_block(wx, wy, wz) == target_type
        && (chunk_manager.get_fluid_level(wx, wy, wz) & FLUID_LEVEL_MASK) == 0
        && !chunk_manager.get_fluid_falling(wx, wy, wz)
}

fn is_supported(
    chunk_manager: &ChunkManager,
    wx: i32,
    wy: i32,
    wz: i32,
    fluid_type: BlockType,
) -> bool {
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
    let mut dirty = HashSet::new();
    chunk_manager.check_and_break_unsupported_above(wx, wy, wz, &mut dirty, |_, _| {});
    true
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
        assert!(tick_fluids(&mut manager, false, 64).0.is_empty());
        assert_eq!(manager.pending_fluid_updates(false), 0);
    }

    #[test]
    fn placed_source_flows_without_scanning_the_world() {
        let mut manager = ChunkManager::new(1);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        let source = (8, 120, 8);
        manager.set_block(source.0, source.1, source.2, BlockType::Water);

        let (dirty, mutations) = tick_fluids(&mut manager, false, 128);

        assert!(!dirty.is_empty());
        assert_eq!(manager.get_block(8, 119, 8), BlockType::Water);
        assert!(manager.get_fluid_falling(8, 119, 8));
        // A flowing cell should report its new block so a host can broadcast it.
        assert!(mutations.iter().any(|mutation| {
            mutation.position == (8, 119, 8) && mutation.block == BlockType::Water
        }));
    }

    #[test]
    fn raw_fluid_change_is_reported_when_block_stays_water() {
        let mut manager = ChunkManager::new(1);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        let source = (8, 120, 8);
        let flowing = (8, 119, 8);
        manager.set_block(source.0, source.1, source.2, BlockType::Water);
        manager.set_block(flowing.0, flowing.1, flowing.2, BlockType::Water);
        manager.set_fluid_raw(
            flowing.0,
            flowing.1,
            flowing.2,
            FLUID_LEVEL_MASK | crate::world::FLUID_FALLING_BIT,
        );

        let before = manager.get_fluid_raw(flowing.0, flowing.1, flowing.2);
        let (_, mutations) = tick_fluids(&mut manager, false, 64);
        assert!(mutations.iter().any(|mutation| {
            mutation.position == flowing
                && mutation.block == BlockType::Water
                && mutation.raw_fluid != before
        }));
    }

    #[test]
    fn waterlogged_slab_source_flows_into_adjacent_air() {
        let mut manager = ChunkManager::new(1);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        let slab = (8, 120, 8);
        let below = (8, 119, 8);
        manager.set_block(slab.0, slab.1, slab.2, BlockType::OakSlab);
        assert!(manager.set_waterlogged(slab.0, slab.1, slab.2, true));

        let (_, mutations) = tick_fluids(&mut manager, false, 128);
        assert_eq!(
            manager.get_block(below.0, below.1, below.2),
            BlockType::Water
        );
        assert!(mutations
            .iter()
            .any(|mutation| { mutation.position == below && mutation.block == BlockType::Water }));
        assert!(manager.is_waterlogged(slab.0, slab.1, slab.2));
    }

    #[test]
    fn waterlogged_slab_flows_across_chunk_boundary() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.chunks.insert((1, 0), Chunk::new(1, 0));
        let slab = (15, 80, 8);
        let across = (16, 80, 8);
        manager.set_block(slab.0, slab.1, slab.2, BlockType::OakSlab);
        manager.set_block(slab.0, slab.1 - 1, slab.2, BlockType::Stone);
        manager.set_block(across.0, across.1 - 1, across.2, BlockType::Stone);
        manager.set_waterlogged(slab.0, slab.1, slab.2, true);

        let mut dirty = HashSet::new();
        let mut mutations = Vec::new();
        for _ in 0..8 {
            let (next_dirty, next_mutations) = tick_fluids(&mut manager, false, 128);
            dirty.extend(next_dirty);
            mutations.extend(next_mutations);
            if manager.pending_fluid_updates(false) == 0 {
                break;
            }
        }

        assert_eq!(
            manager.get_block(across.0, across.1, across.2),
            BlockType::Water
        );
        assert_eq!(manager.get_fluid_level(across.0, across.1, across.2), 1);
        assert!(dirty.contains(&(1, 0)));
        assert!(mutations
            .iter()
            .any(|mutation| { mutation.position == across && mutation.block == BlockType::Water }));
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

    #[test]
    fn y_zero_is_not_automatic_support_for_infinite_source() {
        let mut manager = ChunkManager::new(1);
        manager.chunks.insert((0, 0), Chunk::empty(0, 0));
        manager.set_block(8, 0, 8, BlockType::Water);
        manager.set_block(10, 0, 8, BlockType::Water);
        manager.set_block(9, 0, 8, BlockType::Water);
        manager.set_fluid_level(9, 0, 8, 1);
        manager.set_fluid_falling(9, 0, 8, false);

        tick_fluids(&mut manager, false, 64);

        assert_eq!(manager.get_block(9, 0, 8), BlockType::Water);
        assert_ne!(
            manager.get_fluid_level(9, 0, 8),
            0,
            "Y=0 over air must not form an infinite source"
        );
    }
}
