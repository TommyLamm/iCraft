use crate::world::{BlockType, Chunk, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
use std::collections::{HashMap, HashSet, VecDeque};

type BlockPos = (i32, i32, i32);

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
    water_updates: FluidUpdateQueue,
    lava_updates: FluidUpdateQueue,
}

impl ChunkManager {
    pub fn new(render_distance: i32) -> Self {
        Self {
            chunks: HashMap::new(),
            render_distance,
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
            return 15; // Above world is fully lit by sky
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
}
