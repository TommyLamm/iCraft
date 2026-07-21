use crate::world::{BlockType, Chunk, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
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
            water_updates: FluidUpdateQueue::new(),
            lava_updates: FluidUpdateQueue::new(),
        }
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
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.blocks[bx][by][bz];
            }
        }
        BlockType::Air
    }

    pub fn set_block(&mut self, wx: i32, wy: i32, wz: i32, block: BlockType) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                if chunk.blocks[bx][by][bz] == block {
                    return;
                }
                chunk.blocks[bx][by][bz] = block;
                if block != BlockType::Water && block != BlockType::Lava {
                    chunk.fluid_levels[bx][by][bz] = 0;
                }
                chunk.update_heightmap(bx, bz);
                self.schedule_fluid_neighbors(wx, wy, wz);
            }
        }
    }

    pub fn get_sky_light(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.sky_light[bx][by][bz];
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
                chunk.sky_light[bx][by][bz] = val;
            }
        }
    }

    pub fn get_block_light(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.block_light[bx][by][bz];
            }
        }
        0
    }

    pub fn set_block_light(&mut self, wx: i32, wy: i32, wz: i32, val: u8) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                chunk.block_light[bx][by][bz] = val;
            }
        }
    }

    pub fn get_fluid_level(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.fluid_levels[bx][by][bz] & 0x07;
            }
        }
        0
    }

    pub fn set_fluid_level(&mut self, wx: i32, wy: i32, wz: i32, level: u8) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                let current = chunk.fluid_levels[bx][by][bz];
                let updated = (current & 0xF8) | (level & 0x07);
                if current != updated {
                    chunk.fluid_levels[bx][by][bz] = updated;
                    self.schedule_fluid_neighbors(wx, wy, wz);
                }
            }
        }
    }

    pub fn get_fluid_falling(&self, wx: i32, wy: i32, wz: i32) -> bool {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return (chunk.fluid_levels[bx][by][bz] & 0x08) != 0;
            }
        }
        false
    }

    pub fn set_fluid_falling(&mut self, wx: i32, wy: i32, wz: i32, falling: bool) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                let current = chunk.fluid_levels[bx][by][bz];
                let updated = if falling {
                    current | 0x08
                } else {
                    current & !0x08
                };
                if current != updated {
                    chunk.fluid_levels[bx][by][bz] = updated;
                    self.schedule_fluid_neighbors(wx, wy, wz);
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
        let mut current_y = wy + 1;
        while current_y < CHUNK_HEIGHT as i32 {
            let block_above = self.get_block(wx, current_y, wz);
            if block_above == BlockType::Air {
                break;
            }
            let block_below = self.get_block(wx, current_y - 1, wz);
            if !block_above.can_stay_on(block_below) {
                self.set_block(wx, current_y, wz, BlockType::Air);

                crate::lighting::update_sky_light_after_removed(
                    self,
                    wx,
                    current_y,
                    wz,
                    dirty_chunks,
                );
                crate::lighting::update_block_light_after_removed(
                    self,
                    wx,
                    current_y,
                    wz,
                    block_above.properties().light_emission,
                    dirty_chunks,
                );
                mark_block_mesh_dependencies(dirty_chunks, wx, wz);

                on_break((wx, current_y, wz), block_above);

                current_y += 1;
            } else {
                break;
            }
        }
    }
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
}

