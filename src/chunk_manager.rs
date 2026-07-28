use crate::world::{BlockSupportStatus, BlockType, Chunk, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
use std::collections::{HashMap, HashSet, VecDeque};

type BlockPos = (i32, i32, i32);

/// Adds every chunk whose mesh can depend on a block at the supplied world position.
/// AO corner samples make a diagonal chunk dependent on blocks at chunk corners.
pub(crate) fn mark_block_mesh_dependencies(dirty: &mut HashSet<(i32, i32)>, wx: i32, wz: i32) {
    let cx = wx.div_euclid(CHUNK_WIDTH as i32);
    let cz = wz.div_euclid(CHUNK_DEPTH as i32);
    let lx = wx.rem_euclid(CHUNK_WIDTH as i32);
    let lz = wz.rem_euclid(CHUNK_DEPTH as i32);

    let x_neighbor = if lx == 0 {
        Some(cx - 1)
    } else if lx == CHUNK_WIDTH as i32 - 1 {
        Some(cx + 1)
    } else {
        None
    };

    let z_neighbor = if lz == 0 {
        Some(cz - 1)
    } else if lz == CHUNK_DEPTH as i32 - 1 {
        Some(cz + 1)
    } else {
        None
    };

    dirty.insert((cx, cz));
    if let Some(affected_cx) = x_neighbor {
        dirty.insert((affected_cx, cz));
    }
    if let Some(affected_cz) = z_neighbor {
        dirty.insert((cx, affected_cz));
    }
    if let (Some(affected_cx), Some(affected_cz)) = (x_neighbor, z_neighbor) {
        dirty.insert((affected_cx, affected_cz));
    }
}

pub(crate) fn surrounding_chunk_coords(cx: i32, cz: i32) -> [(i32, i32); 8] {
    [
        (cx - 1, cz - 1),
        (cx, cz - 1),
        (cx + 1, cz - 1),
        (cx - 1, cz),
        (cx + 1, cz),
        (cx - 1, cz + 1),
        (cx, cz + 1),
        (cx + 1, cz + 1),
    ]
}

struct FluidUpdateQueue {
    queue: VecDeque<BlockPos>,
    queued: HashSet<BlockPos>,
}

impl FluidUpdateQueue {
    fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            queued: HashSet::new(),
        }
    }

    fn push(&mut self, pos: BlockPos) {
        if self.queued.insert(pos) {
            self.queue.push_back(pos);
        }
    }

    fn pop(&mut self) -> Option<BlockPos> {
        let pos = self.queue.pop_front()?;
        self.queued.remove(&pos);
        Some(pos)
    }
}

pub struct ChunkManager {
    pub chunks: HashMap<(i32, i32), Chunk>,
    pub render_distance: i32,
    pub dimension: crate::dimension::Dimension,
    pub dirty_chunks: crate::save::DirtyChunkSet,
    water_updates: FluidUpdateQueue,
    lava_updates: FluidUpdateQueue,
}

impl ChunkManager {
    #[cfg(test)]
    pub fn new(render_distance: i32) -> Self {
        Self::new_in_dimension(render_distance, crate::dimension::Dimension::Overworld)
    }

    pub fn new_in_dimension(render_distance: i32, dimension: crate::dimension::Dimension) -> Self {
        Self {
            chunks: HashMap::new(),
            render_distance,
            dimension,
            dirty_chunks: crate::save::DirtyChunkSet::new(),
            water_updates: FluidUpdateQueue::new(),
            lava_updates: FluidUpdateQueue::new(),
        }
    }

    pub fn mark_dirty(&mut self, cx: i32, cz: i32) {
        self.dirty_chunks.mark_dirty(cx, cz);
    }

    fn schedule_fluid_neighbors(&mut self, wx: i32, wy: i32, wz: i32) {
        const OFFSETS: [(i32, i32, i32); 7] = [
            (0, 0, 0),
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ];

        for (dx, dy, dz) in OFFSETS {
            let pos = (wx + dx, wy + dy, wz + dz);
            if pos.1 >= 0 && pos.1 < CHUNK_HEIGHT as i32 {
                self.water_updates.push(pos);
                self.lava_updates.push(pos);
            }
        }
    }

    pub fn pop_fluid_update(&mut self, is_lava: bool) -> Option<BlockPos> {
        if is_lava {
            self.lava_updates.pop()
        } else {
            self.water_updates.pop()
        }
    }

    #[cfg(test)]
    pub fn pending_fluid_updates(&self, is_lava: bool) -> usize {
        if is_lava {
            self.lava_updates.queue.len()
        } else {
            self.water_updates.queue.len()
        }
    }

    pub fn world_to_local(
        &self,
        wx: i32,
        wy: i32,
        wz: i32,
    ) -> Option<((i32, i32), (usize, usize, usize))> {
        if wy < 0 || wy >= CHUNK_HEIGHT as i32 {
            return None;
        }
        let cx = wx.div_euclid(CHUNK_WIDTH as i32);
        let cz = wz.div_euclid(CHUNK_DEPTH as i32);
        let bx = wx.rem_euclid(CHUNK_WIDTH as i32) as usize;
        let bz = wz.rem_euclid(CHUNK_DEPTH as i32) as usize;
        let by = wy as usize;
        Some(((cx, cz), (bx, by, bz)))
    }

    pub fn get_block(&self, wx: i32, wy: i32, wz: i32) -> BlockType {
        self.get_loaded_block(wx, wy, wz).unwrap_or(BlockType::Air)
    }

    /// Returns `None` when the coordinate is outside world height or its chunk
    /// is not loaded. Support rules use this instead of treating missing chunk
    /// data as air.
    pub fn get_loaded_block(&self, wx: i32, wy: i32, wz: i32) -> Option<BlockType> {
        let ((cx, cz), (bx, by, bz)) = self.world_to_local(wx, wy, wz)?;
        let chunk = self.chunks.get(&(cx, cz))?;
        Some(chunk.get_block_local(bx, by, bz))
    }

    pub fn block_support_status(
        &self,
        block: BlockType,
        wx: i32,
        wy: i32,
        wz: i32,
    ) -> BlockSupportStatus {
        block.support_status_at((wx, wy, wz), |x, y, z| self.get_loaded_block(x, y, z))
    }

    pub fn can_place_block_with_support(
        &self,
        block: BlockType,
        wx: i32,
        wy: i32,
        wz: i32,
    ) -> bool {
        self.get_loaded_block(wx, wy, wz).is_some()
            && self.block_support_status(block, wx, wy, wz) == BlockSupportStatus::Supported
    }

    pub fn get_block_state(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.get_block_state(bx as i32, by as i32, bz as i32);
            }
        }
        0
    }

    pub fn set_block_state(&mut self, wx: i32, wy: i32, wz: i32, state: u8) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                if chunk.get_block_state(bx as i32, by as i32, bz as i32) != state {
                    chunk.set_block_state(bx as i32, by as i32, bz as i32, state);
                    self.dirty_chunks.mark_dirty(cx, cz);
                }
            }
        }
    }

    pub fn set_block(&mut self, wx: i32, wy: i32, wz: i32, block: BlockType) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                if chunk.get_block_local(bx, by, bz) == block {
                    return;
                }
                chunk.set_block_local(bx, by, bz, block);
                chunk.set_block_state(bx as i32, by as i32, bz as i32, 0);
                if block != BlockType::Water && block != BlockType::Lava {
                    chunk.set_fluid_level(bx, by, bz, 0);
                }
                chunk.update_heightmap(bx, bz);
                self.schedule_fluid_neighbors(wx, wy, wz);
                self.dirty_chunks.mark_dirty(cx, cz);
            }
        }
    }

    pub fn get_sky_light(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.get_sky_light(bx, by, bz);
            }
        }
        if wy >= CHUNK_HEIGHT as i32 {
            return if self.dimension.has_sky_light() {
                15
            } else {
                0
            };
        }
        0
    }

    pub fn set_sky_light(&mut self, wx: i32, wy: i32, wz: i32, val: u8) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                if chunk.get_sky_light(bx, by, bz) != val {
                    chunk.set_sky_light(bx, by, bz, val);
                    self.dirty_chunks.mark_dirty(cx, cz);
                }
            }
        }
    }

    pub fn get_block_light(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.get_block_light(bx, by, bz);
            }
        }
        0
    }

    pub fn set_block_light(&mut self, wx: i32, wy: i32, wz: i32, val: u8) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                if chunk.get_block_light(bx, by, bz) != val {
                    chunk.set_block_light(bx, by, bz, val);
                    self.dirty_chunks.mark_dirty(cx, cz);
                }
            }
        }
    }

    pub fn get_fluid_level(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.get_fluid_level(bx, by, bz) & 0x07;
            }
        }
        0
    }

    pub fn set_fluid_level(&mut self, wx: i32, wy: i32, wz: i32, level: u8) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                let current = chunk.get_fluid_level(bx, by, bz);
                let updated = (current & 0xF8) | (level & 0x07);
                if current != updated {
                    chunk.set_fluid_level(bx, by, bz, updated);
                    self.schedule_fluid_neighbors(wx, wy, wz);
                    self.dirty_chunks.mark_dirty(cx, cz);
                }
            }
        }
    }

    pub fn get_fluid_falling(&self, wx: i32, wy: i32, wz: i32) -> bool {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return (chunk.get_fluid_level(bx, by, bz) & 0x08) != 0;
            }
        }
        false
    }

    pub fn set_fluid_falling(&mut self, wx: i32, wy: i32, wz: i32, falling: bool) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                let current = chunk.get_fluid_level(bx, by, bz);
                let updated = if falling {
                    current | 0x08
                } else {
                    current & !0x08
                };
                if current != updated {
                    chunk.set_fluid_level(bx, by, bz, updated);
                    self.schedule_fluid_neighbors(wx, wy, wz);
                    self.dirty_chunks.mark_dirty(cx, cz);
                }
            }
        }
    }

    pub fn check_and_break_unsupported_above<F>(
        &mut self,
        wx: i32,
        wy: i32,
        wz: i32,
        dirty_chunks: &mut std::collections::HashSet<(i32, i32)>,
        mut on_break: F,
    ) where
        F: FnMut((i32, i32, i32), BlockType),
    {
        self.break_unsupported_from_candidates(
            support_candidates_affected_by_change((wx, wy, wz)),
            dirty_chunks,
            &mut on_break,
        );
    }

    /// Revalidates context-dependent plants in a newly loaded chunk and along
    /// the cardinal borders of already-loaded neighbors. This resolves blocks
    /// previously preserved as `Unknown` without loading any additional chunks.
    pub fn check_and_break_unsupported_for_loaded_chunk<F>(
        &mut self,
        cx: i32,
        cz: i32,
        dirty_chunks: &mut std::collections::HashSet<(i32, i32)>,
        mut on_break: F,
    ) where
        F: FnMut((i32, i32, i32), BlockType),
    {
        if !self.chunks.contains_key(&(cx, cz)) {
            return;
        }

        let min_x = cx * CHUNK_WIDTH as i32;
        let min_z = cz * CHUNK_DEPTH as i32;
        let mut candidates = Vec::new();

        for x in min_x..min_x + CHUNK_WIDTH as i32 {
            for z in min_z..min_z + CHUNK_DEPTH as i32 {
                for y in 1..CHUNK_HEIGHT as i32 {
                    if matches!(
                        self.get_loaded_block(x, y, z),
                        Some(BlockType::SugarCane | BlockType::Cactus)
                    ) {
                        candidates.push((x, y, z));
                    }
                }
            }
        }

        for z in min_z..min_z + CHUNK_DEPTH as i32 {
            for x in [min_x - 1, min_x + CHUNK_WIDTH as i32] {
                for y in 1..CHUNK_HEIGHT as i32 {
                    if matches!(
                        self.get_loaded_block(x, y, z),
                        Some(BlockType::SugarCane | BlockType::Cactus)
                    ) {
                        candidates.push((x, y, z));
                    }
                }
            }
        }
        for x in min_x..min_x + CHUNK_WIDTH as i32 {
            for z in [min_z - 1, min_z + CHUNK_DEPTH as i32] {
                for y in 1..CHUNK_HEIGHT as i32 {
                    if matches!(
                        self.get_loaded_block(x, y, z),
                        Some(BlockType::SugarCane | BlockType::Cactus)
                    ) {
                        candidates.push((x, y, z));
                    }
                }
            }
        }

        self.break_unsupported_from_candidates(candidates, dirty_chunks, &mut on_break);
    }

    fn break_unsupported_from_candidates<I, F>(
        &mut self,
        candidates: I,
        dirty_chunks: &mut std::collections::HashSet<(i32, i32)>,
        on_break: &mut F,
    ) where
        I: IntoIterator<Item = BlockPos>,
        F: FnMut((i32, i32, i32), BlockType),
    {
        let mut queue = VecDeque::new();
        let mut queued = HashSet::new();
        for position in candidates {
            if queued.insert(position) {
                queue.push_back(position);
            }
        }

        while let Some((x, y, z)) = queue.pop_front() {
            queued.remove(&(x, y, z));
            let Some(block) = self.get_loaded_block(x, y, z) else {
                continue;
            };
            if block == BlockType::Air
                || self.block_support_status(block, x, y, z) != BlockSupportStatus::Unsupported
            {
                continue;
            }

            self.set_block(x, y, z, BlockType::Air);
            crate::lighting::update_sky_light_after_removed(self, x, y, z, dirty_chunks);
            crate::lighting::update_block_light_after_removed(
                self,
                x,
                y,
                z,
                block.properties().light_emission,
                dirty_chunks,
            );
            mark_block_mesh_dependencies(dirty_chunks, x, z);
            on_break((x, y, z), block);

            for affected in support_candidates_affected_by_change((x, y, z)) {
                if queued.insert(affected) {
                    queue.push_back(affected);
                }
            }
        }
    }
}

fn support_candidates_affected_by_change((x, y, z): BlockPos) -> [BlockPos; 10] {
    [
        (x, y, z),
        (x, y + 1, z),
        (x + 1, y, z),
        (x - 1, y, z),
        (x, y, z + 1),
        (x, y, z - 1),
        (x + 1, y + 1, z),
        (x - 1, y + 1, z),
        (x, y + 1, z + 1),
        (x, y + 1, z - 1),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dependencies(wx: i32, wz: i32) -> HashSet<(i32, i32)> {
        let mut result = HashSet::new();
        mark_block_mesh_dependencies(&mut result, wx, wz);
        result
    }

    #[test]
    fn interior_block_only_invalidates_its_own_chunk() {
        assert_eq!(dependencies(8, 8), HashSet::from([(0, 0)]));
    }

    #[test]
    fn chunk_edges_include_the_adjacent_chunk() {
        assert_eq!(dependencies(0, 8), HashSet::from([(0, 0), (-1, 0)]));
        assert_eq!(dependencies(15, 8), HashSet::from([(0, 0), (1, 0)]));
        assert_eq!(dependencies(8, 0), HashSet::from([(0, 0), (0, -1)]));
        assert_eq!(dependencies(8, 15), HashSet::from([(0, 0), (0, 1)]));
    }

    #[test]
    fn chunk_corners_include_the_diagonal_chunk() {
        assert_eq!(
            dependencies(15, 15),
            HashSet::from([(0, 0), (1, 0), (0, 1), (1, 1)])
        );
        assert_eq!(
            dependencies(0, 0),
            HashSet::from([(0, 0), (-1, 0), (0, -1), (-1, -1)])
        );
    }

    #[test]
    fn negative_world_coordinates_use_euclidean_chunk_boundaries() {
        assert_eq!(
            dependencies(-1, -1),
            HashSet::from([(-1, -1), (0, -1), (-1, 0), (0, 0)])
        );
        assert_eq!(
            dependencies(-16, -16),
            HashSet::from([(-1, -1), (-2, -1), (-1, -2), (-2, -2)])
        );
    }

    #[test]
    fn surrounding_chunks_contains_all_eight_neighbors() {
        assert_eq!(
            HashSet::from(surrounding_chunk_coords(3, -2)),
            HashSet::from([
                (2, -3),
                (3, -3),
                (4, -3),
                (2, -2),
                (4, -2),
                (2, -1),
                (3, -1),
                (4, -1),
            ])
        );
    }

    #[test]
    fn set_block_updates_torch_index_at_negative_world_coordinates() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((-1, -1), Chunk::new(-1, -1));

        manager.set_block(-1, 64, -1, BlockType::Torch);
        let chunk = manager.chunks.get(&(-1, -1)).unwrap();
        assert_eq!(chunk.torch_positions().len(), 1);
        assert_eq!(
            Chunk::decode_torch_position(chunk.torch_positions()[0]),
            (15, 64, 15)
        );

        manager.set_block(-1, 64, -1, BlockType::Air);
        assert!(manager
            .chunks
            .get(&(-1, -1))
            .unwrap()
            .torch_positions()
            .is_empty());
    }

    #[test]
    fn test_check_and_break_unsupported_above() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.set_block(5, 64, 5, BlockType::Dirt);
        manager.set_block(5, 65, 5, BlockType::Dandelion);
        manager.set_block(5, 66, 5, BlockType::Air);

        let mut dirty = HashSet::new();
        let mut broken = Vec::new();

        // Break dirt beneath dandelion
        manager.set_block(5, 64, 5, BlockType::Air);
        manager.check_and_break_unsupported_above(5, 64, 5, &mut dirty, |pos, block| {
            broken.push((pos, block));
        });

        assert_eq!(manager.get_block(5, 65, 5), BlockType::Air);
        assert_eq!(broken, vec![((5, 65, 5), BlockType::Dandelion)]);
    }

    #[test]
    fn player_placement_support_requires_water_for_cane_and_clear_sides_for_cactus() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.set_block(8, 99, 8, BlockType::Sand);

        assert!(!manager.can_place_block_with_support(BlockType::SugarCane, 8, 100, 8));
        manager.set_block(9, 99, 8, BlockType::Water);
        assert!(manager.can_place_block_with_support(BlockType::SugarCane, 8, 100, 8));

        assert!(manager.can_place_block_with_support(BlockType::Cactus, 8, 100, 8));
        manager.set_block(9, 100, 8, BlockType::Stone);
        assert!(!manager.can_place_block_with_support(BlockType::Cactus, 8, 100, 8));
    }

    #[test]
    fn removing_cane_water_breaks_the_entire_column() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.set_block(8, 99, 8, BlockType::Sand);
        manager.set_block(9, 99, 8, BlockType::Water);
        manager.set_block(8, 100, 8, BlockType::SugarCane);
        manager.set_block(8, 101, 8, BlockType::SugarCane);

        manager.set_block(9, 99, 8, BlockType::Air);
        let mut dirty = HashSet::new();
        let mut broken = Vec::new();
        manager.check_and_break_unsupported_above(9, 99, 8, &mut dirty, |position, block| {
            broken.push((position, block));
        });

        assert_eq!(manager.get_block(8, 100, 8), BlockType::Air);
        assert_eq!(manager.get_block(8, 101, 8), BlockType::Air);
        assert_eq!(
            broken,
            vec![
                ((8, 100, 8), BlockType::SugarCane),
                ((8, 101, 8), BlockType::SugarCane),
            ]
        );
    }

    #[test]
    fn adding_a_lateral_cactus_obstruction_breaks_and_cascades() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.set_block(8, 99, 8, BlockType::Sand);
        manager.set_block(8, 100, 8, BlockType::Cactus);
        manager.set_block(8, 101, 8, BlockType::Cactus);

        manager.set_block(9, 100, 8, BlockType::Stone);
        let mut dirty = HashSet::new();
        let mut broken = Vec::new();
        manager.check_and_break_unsupported_above(9, 100, 8, &mut dirty, |position, block| {
            broken.push((position, block));
        });

        assert_eq!(manager.get_block(8, 100, 8), BlockType::Air);
        assert_eq!(manager.get_block(8, 101, 8), BlockType::Air);
        assert_eq!(
            broken,
            vec![
                ((8, 100, 8), BlockType::Cactus),
                ((8, 101, 8), BlockType::Cactus),
            ]
        );
    }

    #[test]
    fn missing_boundary_chunk_is_unknown_until_loaded_and_never_forced() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.set_block(15, 99, 8, BlockType::Sand);
        manager.set_block(15, 100, 8, BlockType::SugarCane);

        assert_eq!(
            manager.block_support_status(BlockType::SugarCane, 15, 100, 8),
            BlockSupportStatus::Unknown
        );
        let mut dirty = HashSet::new();
        let mut broken = Vec::new();
        manager.check_and_break_unsupported_above(16, 99, 8, &mut dirty, |position, block| {
            broken.push((position, block));
        });
        assert_eq!(
            manager.chunks.len(),
            1,
            "support checks must not load chunks"
        );
        assert_eq!(manager.get_block(15, 100, 8), BlockType::SugarCane);
        assert!(broken.is_empty());

        manager.chunks.insert((1, 0), Chunk::new(1, 0));
        manager.set_block(16, 99, 8, BlockType::Water);
        manager.check_and_break_unsupported_for_loaded_chunk(
            1,
            0,
            &mut dirty,
            |position, block| broken.push((position, block)),
        );
        assert_eq!(manager.get_block(15, 100, 8), BlockType::SugarCane);
        assert!(broken.is_empty());
    }

    #[test]
    fn loading_a_boundary_obstruction_revalidates_neighboring_cactus() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.set_block(15, 99, 8, BlockType::Sand);
        manager.set_block(15, 100, 8, BlockType::Cactus);
        assert_eq!(
            manager.block_support_status(BlockType::Cactus, 15, 100, 8),
            BlockSupportStatus::Unknown
        );

        manager.chunks.insert((1, 0), Chunk::new(1, 0));
        manager.set_block(16, 100, 8, BlockType::Stone);
        let mut dirty = HashSet::new();
        let mut broken = Vec::new();
        manager.check_and_break_unsupported_for_loaded_chunk(
            1,
            0,
            &mut dirty,
            |position, block| broken.push((position, block)),
        );

        assert_eq!(manager.get_block(15, 100, 8), BlockType::Air);
        assert_eq!(broken, vec![((15, 100, 8), BlockType::Cactus)]);
        assert!(dirty.contains(&(0, 0)));
        assert!(dirty.contains(&(1, 0)));
    }
}
