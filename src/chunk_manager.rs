use crate::block_entity::BlockEntity;
use crate::dimension::WorldHeight;
use crate::inventory::{ContainerInventory, ItemStack};
use crate::world::{
    BlockSupportStatus, BlockType, Chunk, MeshVoxel, SectionHaloSnapshot, SectionKey, CHUNK_DEPTH,
    CHUNK_WIDTH, FLUID_FALLING_BIT, FLUID_LEVEL_MASK, FLUID_RESERVED_MASK, FLUID_WATERLOGGED_BIT,
    SECTION_SIZE,
};
use std::collections::{HashMap, HashSet, VecDeque};

type BlockPos = (i32, i32, i32);

/// Immutable 3×3 column halo for entity physics. Built once per mover so the
/// collision loop never does a per-voxel `chunks.get`.
#[derive(Clone, Copy)]
pub struct ColumnNeighborhood<'a> {
    pub origin_cx: i32,
    pub origin_cz: i32,
    pub columns: [[Option<&'a Chunk>; 3]; 3],
    pub height: WorldHeight,
}

impl<'a> ColumnNeighborhood<'a> {
    fn column(&self, wx: i32, wz: i32) -> Option<&'a Chunk> {
        let cx = wx.div_euclid(CHUNK_WIDTH as i32);
        let cz = wz.div_euclid(CHUNK_DEPTH as i32);
        let dx = cx - self.origin_cx;
        let dz = cz - self.origin_cz;
        if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
            return None;
        }
        self.columns[(dz + 1) as usize][(dx + 1) as usize]
    }

    pub fn is_block_loaded(&self, wx: i32, _wy: i32, wz: i32) -> bool {
        self.column(wx, wz).is_some()
    }

    pub fn get_block(&self, wx: i32, wy: i32, wz: i32) -> BlockType {
        if !self.height.contains_y(wy) {
            return BlockType::Air;
        }
        let Some(chunk) = self.column(wx, wz) else {
            return BlockType::Air;
        };
        let bx = wx.rem_euclid(CHUNK_WIDTH as i32) as usize;
        let bz = wz.rem_euclid(CHUNK_DEPTH as i32) as usize;
        chunk.get_block_local(bx, wy, bz)
    }

    pub fn get_block_state(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if !self.height.contains_y(wy) {
            return 0;
        }
        let Some(chunk) = self.column(wx, wz) else {
            return 0;
        };
        let bx = wx.rem_euclid(CHUNK_WIDTH as i32);
        let bz = wz.rem_euclid(CHUNK_DEPTH as i32);
        chunk.get_block_state(bx, wy, bz)
    }
}

/// Adds every chunk whose mesh can depend on a block at the supplied world position.
/// AO corner samples make a diagonal chunk dependent on blocks at chunk corners.
pub fn mark_block_mesh_dependencies(dirty: &mut HashSet<(i32, i32)>, wx: i32, wz: i32) {
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

/// Marks the owner section and only sections that can observe a one-cell halo
/// sample (including edges/corners). This is the exact 3-D counterpart to the
/// legacy chunk dependency helper.
pub fn mark_section_mesh_dependencies(dirty: &mut HashSet<SectionKey>, wx: i32, wy: i32, wz: i32) {
    let sy = crate::world::world_y_to_section_y(wy) as i32;
    let cx = wx.div_euclid(CHUNK_WIDTH as i32);
    let cz = wz.div_euclid(CHUNK_DEPTH as i32);
    let lx = wx.rem_euclid(CHUNK_WIDTH as i32);
    let lz = wz.rem_euclid(CHUNK_DEPTH as i32);
    let ly = wy.rem_euclid(SECTION_SIZE as i32);
    let xs = if lx == 0 {
        [-1, 0]
    } else if lx == 15 {
        [0, 1]
    } else {
        [0, 0]
    };
    let zs = if lz == 0 {
        [-1, 0]
    } else if lz == 15 {
        [0, 1]
    } else {
        [0, 0]
    };
    let ys = if ly == 0 {
        [-1, 0]
    } else if ly == 15 {
        [0, 1]
    } else {
        [0, 0]
    };
    for dx in xs {
        for dz in zs {
            for dy in ys {
                let target = sy + dy;
                dirty.insert(SectionKey::new(cx + dx, target as i8, cz + dz));
            }
        }
    }
}

pub fn surrounding_chunk_coords(cx: i32, cz: i32) -> [(i32, i32); 8] {
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
    /// Monotonic counter bumped whenever the resident column set changes.
    /// Redstone compares this instead of scanning `known_chunks` against keys.
    load_generation: u64,
    water_updates: FluidUpdateQueue,
    lava_updates: FluidUpdateQueue,
    pending_mesh_invalidations: HashSet<(i32, i32)>,
    pending_section_mesh_invalidations: HashSet<SectionKey>,
}

impl ChunkManager {
    pub fn new(render_distance: i32) -> Self {
        Self::new_in_dimension(render_distance, crate::dimension::Dimension::Overworld)
    }

    pub fn new_in_dimension(render_distance: i32, dimension: crate::dimension::Dimension) -> Self {
        Self {
            chunks: HashMap::new(),
            render_distance,
            dimension,
            dirty_chunks: crate::save::DirtyChunkSet::new(),
            load_generation: 0,
            water_updates: FluidUpdateQueue::new(),
            lava_updates: FluidUpdateQueue::new(),
            pending_mesh_invalidations: HashSet::new(),
            pending_section_mesh_invalidations: HashSet::new(),
        }
    }

    /// Resident-column generation observed by redstone sleep / sync.
    pub fn load_generation(&self) -> u64 {
        self.load_generation
    }

    /// Call after inserting or removing a resident column so redstone can skip
    /// O(resident) key-set compares on the sleeping path.
    pub fn bump_load_generation(&mut self) {
        self.load_generation = self.load_generation.wrapping_add(1);
    }

    /// Insert a resident column and bump [`Self::load_generation`].
    pub fn insert_resident_chunk(&mut self, key: (i32, i32), chunk: Chunk) {
        self.chunks.insert(key, chunk);
        self.bump_load_generation();
    }

    /// Remove a resident column and bump [`Self::load_generation`].
    pub fn remove_resident_chunk(&mut self, key: &(i32, i32)) -> Option<Chunk> {
        let removed = self.chunks.remove(key)?;
        self.bump_load_generation();
        Some(removed)
    }

    fn record_mesh_invalidation(&mut self, wx: i32, wy: i32, wz: i32) {
        mark_block_mesh_dependencies(&mut self.pending_mesh_invalidations, wx, wz);
        mark_section_mesh_dependencies(&mut self.pending_section_mesh_invalidations, wx, wy, wz);
    }

    pub fn acknowledge_mesh_invalidation(&mut self, coord: &(i32, i32)) {
        self.pending_mesh_invalidations.remove(coord);
    }

    pub fn drain_mesh_invalidations(&mut self) -> HashSet<(i32, i32)> {
        std::mem::take(&mut self.pending_mesh_invalidations)
    }

    pub fn acknowledge_section_mesh_invalidation(&mut self, key: &SectionKey) {
        self.pending_section_mesh_invalidations.remove(key);
    }

    pub fn drain_section_mesh_invalidations(&mut self) -> HashSet<SectionKey> {
        std::mem::take(&mut self.pending_section_mesh_invalidations)
    }

    pub fn mark_dirty(&mut self, cx: i32, cz: i32) {
        self.dirty_chunks.mark_dirty(cx, cz);
    }

    /// Insert a join-client column from a revision-gated `ChunkData` payload.
    /// Never generates terrain; missing streams fail closed via restore.
    pub fn insert_authoritative_chunk_payload(
        &mut self,
        cx: i32,
        cz: i32,
        blocks: &[u8],
        block_states: &[u8],
        fluid_levels: &[u8],
        block_entities: &[u8],
    ) -> std::io::Result<()> {
        let mut chunk = Chunk::empty_in_dimension(self.dimension, cx, cz);
        crate::save::ChunkSaveData::restore_network_payload(
            &mut chunk,
            blocks,
            block_states,
            fluid_levels,
            block_entities,
        )?;
        self.chunks.insert((cx, cz), chunk);
        self.bump_load_generation();
        Ok(())
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
        let height = self.dimension.height();

        for (dx, dy, dz) in OFFSETS {
            let pos = (wx + dx, wy + dy, wz + dz);
            if height.contains_y(pos.1) {
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
    ) -> Option<((i32, i32), (usize, i32, usize))> {
        let height = self.dimension.height();
        if !height.contains_y(wy) {
            return None;
        }
        let cx = wx.div_euclid(CHUNK_WIDTH as i32);
        let cz = wz.div_euclid(CHUNK_DEPTH as i32);
        let bx = wx.rem_euclid(CHUNK_WIDTH as i32) as usize;
        let bz = wz.rem_euclid(CHUNK_DEPTH as i32) as usize;
        Some(((cx, cz), (bx, wy, bz)))
    }

    /// Returns the block at the coordinate. Air fallback applies when the
    /// column is loaded (empty cell) or Y is outside world height. Unloaded
    /// columns also currently return Air for historical callers; collision
    /// and support must use `get_loaded_block` / `is_block_loaded` instead.
    pub fn get_block(&self, wx: i32, wy: i32, wz: i32) -> BlockType {
        self.get_loaded_block(wx, wy, wz).unwrap_or(BlockType::Air)
    }

    /// Highest solid block in a loaded column, starting from the heightmap.
    /// Unloaded or empty columns return `None` (same as scanning Air).
    pub fn highest_solid_y(&self, wx: i32, wz: i32) -> Option<i32> {
        let height = self.dimension.height();
        let cx = wx.div_euclid(CHUNK_WIDTH as i32);
        let cz = wz.div_euclid(CHUNK_DEPTH as i32);
        let bx = wx.rem_euclid(CHUNK_WIDTH as i32) as usize;
        let bz = wz.rem_euclid(CHUNK_DEPTH as i32) as usize;
        let chunk = self.chunks.get(&(cx, cz))?;
        let mapped = chunk.heightmap[bx][bz];
        if mapped == crate::world::NO_HEIGHT {
            return None;
        }
        let start_y = (mapped as i32).clamp(height.min_y(), height.max_y_exclusive() - 1);
        for y in (height.min_y()..=start_y).rev() {
            if chunk.get_block_local(bx, y, bz).properties().is_solid {
                return Some(y);
            }
        }
        None
    }

    /// Center column plus the eight neighbors. Lighting seed and section halo
    /// capture this once so the hot loop never does a per-voxel HashMap get.
    pub fn column_neighborhood(&self, cx: i32, cz: i32) -> [[Option<&Chunk>; 3]; 3] {
        std::array::from_fn(|iz| {
            std::array::from_fn(|ix| self.chunks.get(&(cx + ix as i32 - 1, cz + iz as i32 - 1)))
        })
    }

    /// Physics / entity view of [`Self::column_neighborhood`] with height metadata.
    pub fn column_neighborhood_view(&self, cx: i32, cz: i32) -> ColumnNeighborhood<'_> {
        ColumnNeighborhood {
            origin_cx: cx,
            origin_cz: cz,
            columns: self.column_neighborhood(cx, cz),
            height: self.dimension.height(),
        }
    }

    /// Captures the complete 18^3 worker input for a section. Missing chunks
    /// and out-of-range Y use an explicit air/zero-light sentinel; sky above
    /// the dimension retains full skylight only when the dimension has sky.
    pub fn capture_section_halo(&self, key: SectionKey) -> SectionHaloSnapshot {
        let height = self.dimension.height();
        let has_sky = self.dimension.has_sky_light();
        let neighborhood = self.column_neighborhood(key.cx, key.cz);
        SectionHaloSnapshot::from_chunk(key, |wx, wy, wz| {
            if !height.contains_y(wy) {
                return MeshVoxel {
                    sky: if wy >= height.max_y_exclusive() && has_sky {
                        15
                    } else {
                        0
                    },
                    ..MeshVoxel::default()
                };
            }
            let cx = wx.div_euclid(CHUNK_WIDTH as i32);
            let cz = wz.div_euclid(CHUNK_DEPTH as i32);
            let dx = cx - key.cx;
            let dz = cz - key.cz;
            if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dz) {
                return MeshVoxel::default();
            }
            let Some(chunk) = neighborhood[(dz + 1) as usize][(dx + 1) as usize] else {
                return MeshVoxel::default();
            };
            let bx = wx.rem_euclid(CHUNK_WIDTH as i32) as usize;
            let bz = wz.rem_euclid(CHUNK_DEPTH as i32) as usize;
            MeshVoxel {
                block: chunk.get_block_local(bx, wy, bz),
                state: chunk.get_block_state(bx as i32, wy, bz as i32),
                sky: chunk.get_sky_light(bx, wy, bz),
                block_light: chunk.get_block_light(bx, wy, bz),
                raw_fluid: chunk.get_fluid_level(bx, wy, bz),
            }
        })
    }

    /// Returns `None` when the coordinate is outside world height or its chunk
    /// is not loaded. Support rules use this instead of treating missing chunk
    /// data as air.
    pub fn get_loaded_block(&self, wx: i32, wy: i32, wz: i32) -> Option<BlockType> {
        let ((cx, cz), (bx, by, bz)) = self.world_to_local(wx, wy, wz)?;
        let chunk = self.chunks.get(&(cx, cz))?;
        Some(chunk.get_block_local(bx, by, bz))
    }

    pub fn get_block_entity(
        &self,
        wx: i32,
        wy: i32,
        wz: i32,
    ) -> Option<&crate::block_entity::BlockEntity> {
        let ((cx, cz), (bx, by, bz)) = self.world_to_local(wx, wy, wz)?;
        let chunk = self.chunks.get(&(cx, cz))?;
        chunk.get_block_entity(bx as u8, by as i16, bz as u8)
    }

    pub fn get_block_entity_mut(
        &mut self,
        wx: i32,
        wy: i32,
        wz: i32,
    ) -> Option<&mut crate::block_entity::BlockEntity> {
        let ((cx, cz), (bx, by, bz)) = self.world_to_local(wx, wy, wz)?;
        let chunk = self.chunks.get_mut(&(cx, cz))?;
        chunk.get_block_entity_mut(bx as u8, by as i16, bz as u8)
    }

    pub fn set_block_entity(
        &mut self,
        wx: i32,
        wy: i32,
        wz: i32,
        entity: Option<crate::block_entity::BlockEntity>,
    ) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                if let Some(e) = entity {
                    let _ = chunk.insert_block_entity(bx as u8, by as i16, bz as u8, e);
                } else {
                    let _ = chunk.remove_block_entity(bx as u8, by as i16, bz as u8);
                }
                self.dirty_chunks.mark_dirty(cx, cz);
            }
        }
    }

    fn single_chest_inventory(&self, x: i32, y: i32, z: i32) -> Option<ContainerInventory> {
        match self.get_block_entity(x, y, z)? {
            BlockEntity::Chest(chest) => Some(chest.inventory.clone()),
            _ => None,
        }
    }

    fn chest_slots(&self, x: i32, y: i32, z: i32) -> Option<Vec<Option<ItemStack>>> {
        let primary_inv = self.single_chest_inventory(x, y, z)?;
        let state_raw = self.get_block_state(x, y, z);
        let state = crate::world::BlockState::decode(state_raw);
        if let Some(partner_pos) = crate::block_entity::double_chest_partner(self, (x, y, z)) {
            if let Some(partner_inv) =
                self.single_chest_inventory(partner_pos.0, partner_pos.1, partner_pos.2)
            {
                let mut combined_slots = vec![None; 54];
                if state.chest_type == crate::world::ChestType::Left {
                    combined_slots[..27].clone_from_slice(&primary_inv.slots);
                    combined_slots[27..54].clone_from_slice(&partner_inv.slots);
                } else {
                    combined_slots[..27].clone_from_slice(&partner_inv.slots);
                    combined_slots[27..54].clone_from_slice(&primary_inv.slots);
                }
                return Some(combined_slots);
            }
        }
        Some(primary_inv.slots.to_vec())
    }

    fn set_single_chest_inventory(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        inventory: ContainerInventory,
    ) -> bool {
        let Some(entity) = self.get_block_entity(x, y, z).cloned() else {
            return false;
        };
        if let BlockEntity::Chest(mut chest_be) = entity {
            chest_be.inventory = inventory;
            chest_be.revision = chest_be.revision.wrapping_add(1);
            self.set_block_entity(x, y, z, Some(BlockEntity::Chest(chest_be)));
            true
        } else {
            false
        }
    }

    fn set_chest_slots(&mut self, x: i32, y: i32, z: i32, slots: &[Option<ItemStack>]) -> bool {
        if slots.iter().flatten().any(|stack| {
            stack.count == 0
                || stack.item == crate::inventory::Item::Air
                || stack.count > stack.item.properties().max_stack
        }) {
            return false;
        }
        if slots.len() == 54 {
            let state_raw = self.get_block_state(x, y, z);
            let state = crate::world::BlockState::decode(state_raw);
            if let Some(partner_pos) = crate::block_entity::double_chest_partner(self, (x, y, z)) {
                let (primary_slice, partner_slice) =
                    if state.chest_type == crate::world::ChestType::Left {
                        (&slots[..27], &slots[27..54])
                    } else {
                        (&slots[27..54], &slots[..27])
                    };
                let mut p_arr = [None; 27];
                p_arr.copy_from_slice(primary_slice);
                let mut pt_arr = [None; 27];
                pt_arr.copy_from_slice(partner_slice);
                // Validate and prepare both halves before committing either
                // one. A malformed or unloaded partner must never leave a
                // half-updated double chest.
                let primary = self
                    .get_block_entity(x, y, z)
                    .and_then(|entity| match entity {
                        BlockEntity::Chest(chest) => Some(chest.clone()),
                        _ => None,
                    });
                let partner = self
                    .get_block_entity(partner_pos.0, partner_pos.1, partner_pos.2)
                    .and_then(|entity| match entity {
                        BlockEntity::Chest(chest) => Some(chest.clone()),
                        _ => None,
                    });
                let (Some(mut primary), Some(mut partner)) = (primary, partner) else {
                    return false;
                };
                primary.inventory = ContainerInventory { slots: p_arr };
                primary.revision = primary.revision.wrapping_add(1);
                partner.inventory = ContainerInventory { slots: pt_arr };
                partner.revision = partner.revision.wrapping_add(1);
                self.set_block_entity(x, y, z, Some(BlockEntity::Chest(primary)));
                self.set_block_entity(
                    partner_pos.0,
                    partner_pos.1,
                    partner_pos.2,
                    Some(BlockEntity::Chest(partner)),
                );
                return true;
            }
        }
        if slots.len() == 27 {
            let mut arr = [None; 27];
            arr.copy_from_slice(slots);
            return self.set_single_chest_inventory(x, y, z, ContainerInventory { slots: arr });
        }
        false
    }

    pub fn container_slot_count(&self, x: i32, y: i32, z: i32) -> usize {
        if let Some(entity) = self.get_block_entity(x, y, z) {
            if matches!(entity, BlockEntity::Chest(_))
                && crate::block_entity::double_chest_partner(self, (x, y, z)).is_some()
            {
                54
            } else {
                entity.slot_count()
            }
        } else {
            0
        }
    }

    /// Shared slot view used by UI and automation. Chest halves retain their
    /// existing deterministic 27/54 ordering; all other containers expose
    /// their native slot count and complete metadata-bearing stacks.
    pub fn container_slots(&self, x: i32, y: i32, z: i32) -> Option<Vec<Option<ItemStack>>> {
        let entity = self.get_block_entity(x, y, z)?;
        if matches!(entity, BlockEntity::Chest(_)) {
            return self.chest_slots(x, y, z);
        }
        Some(
            (0..entity.slot_count())
                .map(|slot| entity.get_stack(slot).copied())
                .collect(),
        )
    }

    /// Atomically commits a complete container slot vector after validating the
    /// target entity and exact length. This prevents a malformed/stale click
    /// from partially replacing a multi-slot inventory.
    pub fn set_container_slots(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        slots: &[Option<ItemStack>],
    ) -> bool {
        if matches!(self.get_block_entity(x, y, z), Some(BlockEntity::Chest(_))) {
            return self.set_chest_slots(x, y, z, slots);
        }
        let Some(mut entity) = self.get_block_entity(x, y, z).cloned() else {
            return false;
        };
        if slots.len() != entity.slot_count() {
            return false;
        }
        if !entity.replace_slots(slots) {
            return false;
        }
        self.set_block_entity(x, y, z, Some(entity));
        true
    }

    pub fn ensure_chest_loot_generated(&mut self, x: i32, y: i32, z: i32, world_seed: u32) {
        if let Some(BlockEntity::Chest(chest_be)) = self.get_block_entity_mut(x, y, z) {
            chest_be.ensure_loot_generated(world_seed, (x, y, z));
        }
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
                    self.record_mesh_invalidation(wx, wy, wz);
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
                // A changed block never inherits the previous cell's fluid
                // byte.  Fluid placement sets its canonical level/falling
                // state immediately after this call; clearing here prevents a
                // waterlogged slab bit from leaking into a replacement block.
                chunk.set_fluid_level(bx, by, bz, 0);
                chunk.update_heightmap(bx, bz);
                self.schedule_fluid_neighbors(wx, wy, wz);
                self.dirty_chunks.mark_dirty(cx, cz);
                self.record_mesh_invalidation(wx, wy, wz);
            }
        }
    }

    pub fn get_sky_light(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.get_sky_light(bx, by, bz);
            }
        }
        let height = self.dimension.height();
        if wy >= height.max_y_exclusive() && self.dimension.has_sky_light() {
            return 15;
        }
        0
    }

    pub fn set_sky_light(&mut self, wx: i32, wy: i32, wz: i32, val: u8) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                if chunk.get_sky_light(bx, by, bz) != val {
                    chunk.set_sky_light(bx, by, bz, val);
                    self.dirty_chunks.mark_dirty(cx, cz);
                    self.record_mesh_invalidation(wx, wy, wz);
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
                    self.record_mesh_invalidation(wx, wy, wz);
                }
            }
        }
    }

    pub fn get_fluid_level(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.get_fluid_level(bx, by, bz) & FLUID_LEVEL_MASK;
            }
        }
        0
    }

    pub fn set_fluid_level(&mut self, wx: i32, wy: i32, wz: i32, level: u8) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                let current = chunk.get_fluid_level(bx, by, bz);
                let updated = (current & !FLUID_LEVEL_MASK) | (level & FLUID_LEVEL_MASK);
                if current != updated {
                    chunk.set_fluid_level(bx, by, bz, updated);
                    self.schedule_fluid_neighbors(wx, wy, wz);
                    self.dirty_chunks.mark_dirty(cx, cz);
                    self.record_mesh_invalidation(wx, wy, wz);
                }
            }
        }
    }

    pub fn get_fluid_falling(&self, wx: i32, wy: i32, wz: i32) -> bool {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return (chunk.get_fluid_level(bx, by, bz) & FLUID_FALLING_BIT) != 0;
            }
        }
        false
    }

    pub fn set_fluid_falling(&mut self, wx: i32, wy: i32, wz: i32, falling: bool) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                let current = chunk.get_fluid_level(bx, by, bz);
                let updated = if falling {
                    current | FLUID_FALLING_BIT
                } else {
                    current & !FLUID_FALLING_BIT
                };
                if current != updated {
                    chunk.set_fluid_level(bx, by, bz, updated);
                    self.schedule_fluid_neighbors(wx, wy, wz);
                    self.dirty_chunks.mark_dirty(cx, cz);
                    self.record_mesh_invalidation(wx, wy, wz);
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
        let height = self.dimension.height();
        let y_range = (height.min_y() + 1)..height.max_y_exclusive();
        let mut candidates = Vec::new();

        for x in min_x..min_x + CHUNK_WIDTH as i32 {
            for z in min_z..min_z + CHUNK_DEPTH as i32 {
                for y in y_range.clone() {
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
                for y in y_range.clone() {
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
                for y in y_range.clone() {
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

    /// Returns whether the chunk containing a world coordinate is currently
    /// loaded.  Automation must use this rather than treating an unloaded
    /// destination as air, otherwise a hopper could consume its source item at
    /// a streaming boundary.
    pub fn is_block_loaded(&self, x: i32, _y: i32, z: i32) -> bool {
        let cx = x.div_euclid(CHUNK_WIDTH as i32);
        let cz = z.div_euclid(CHUNK_DEPTH as i32);
        self.chunks.contains_key(&(cx, cz))
    }

    /// Marks a block entity mutation for the normal latest-wins save path.
    pub fn mark_block_entity_dirty(&mut self, x: i32, z: i32) {
        let cx = x.div_euclid(CHUNK_WIDTH as i32);
        let cz = z.div_euclid(CHUNK_DEPTH as i32);
        if self.chunks.contains_key(&(cx, cz)) {
            self.dirty_chunks.mark_dirty(cx, cz);
        }
    }

    /// Return the complete raw fluid byte.  `get_fluid_level` intentionally
    /// remains level-only for existing callers; replication and mesh workers
    /// use this lossless form so bit 7 cannot be dropped.
    pub fn get_fluid_raw(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.get_fluid_level(bx, by, bz);
            }
        }
        0
    }

    /// Set a complete raw fluid byte and schedule the same neighborhood work
    /// as level/falling setters.  Callers must validate block eligibility for
    /// bit 7; malformed wire state is never promoted to a source here.
    pub fn set_fluid_raw(&mut self, wx: i32, wy: i32, wz: i32, raw: u8) {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                let current = chunk.get_fluid_level(bx, by, bz);
                if current != raw {
                    chunk.set_fluid_level(bx, by, bz, raw);
                    self.schedule_fluid_neighbors(wx, wy, wz);
                    self.dirty_chunks.mark_dirty(cx, cz);
                    self.record_mesh_invalidation(wx, wy, wz);
                }
            }
        }
    }

    pub fn is_waterlogged(&self, wx: i32, wy: i32, wz: i32) -> bool {
        self.get_block(wx, wy, wz).is_waterloggable()
            && (self.get_fluid_raw(wx, wy, wz) & FLUID_WATERLOGGED_BIT) != 0
    }

    /// Toggle waterlogging on an existing eligible solid.  Low level/falling
    /// bits are canonicalized away while reserved bits remain untouched.
    pub fn set_waterlogged(&mut self, wx: i32, wy: i32, wz: i32, waterlogged: bool) -> bool {
        if !self.get_block(wx, wy, wz).is_waterloggable() {
            return false;
        }
        let current = self.get_fluid_raw(wx, wy, wz);
        let updated = if waterlogged {
            (current & FLUID_RESERVED_MASK) | FLUID_WATERLOGGED_BIT
        } else {
            current & FLUID_RESERVED_MASK
        };
        if current == updated {
            return false;
        }
        self.set_fluid_raw(wx, wy, wz, updated);
        true
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

    #[test]
    fn authoritative_payload_inserts_without_local_worldgen() {
        let mut source = Chunk::empty(2, -3);
        source.set_block_local(1, 70, 2, BlockType::GoldOre);
        let payload = crate::save::ChunkSaveData::from_chunk(&source).unwrap();

        let mut manager = ChunkManager::new(2);
        manager
            .insert_authoritative_chunk_payload(
                2,
                -3,
                &payload.blocks,
                &payload.block_states,
                &payload.fluid_levels,
                &payload.block_entities,
            )
            .unwrap();

        let chunk = manager.chunks.get(&(2, -3)).expect("column inserted");
        assert_eq!(chunk.get_block_local(1, 70, 2), BlockType::GoldOre);
        assert_eq!(chunk.get_block_local(8, 80, 8), BlockType::Air);
    }

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
    fn direct_storage_mutations_emit_mesh_invalidation_dependencies() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.chunks.insert((1, 0), Chunk::new(1, 0));

        manager.set_block(15, 80, 8, BlockType::Stone);

        assert_eq!(
            manager.drain_mesh_invalidations(),
            HashSet::from([(0, 0), (1, 0)])
        );
        assert!(manager.drain_mesh_invalidations().is_empty());
    }

    #[test]
    fn raw_fluid_roundtrip_preserves_waterlogged_bit_and_boundary_mesh_dependencies() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.chunks.insert((1, 0), Chunk::new(1, 0));
        manager.set_block(15, 80, 8, BlockType::OakSlab);
        manager.drain_mesh_invalidations();

        manager.set_fluid_raw(15, 80, 8, 0xff);
        assert_eq!(manager.get_fluid_raw(15, 80, 8), 0xff);
        assert!(manager.is_waterlogged(15, 80, 8));
        assert_eq!(
            manager.drain_mesh_invalidations(),
            HashSet::from([(0, 0), (1, 0)])
        );

        assert!(manager.set_waterlogged(15, 80, 8, false));
        assert_eq!(manager.get_fluid_raw(15, 80, 8), FLUID_RESERVED_MASK);
        manager.set_fluid_raw(15, 80, 8, 0xff);
        assert!(manager.set_waterlogged(15, 80, 8, false));
        assert_eq!(manager.get_fluid_raw(15, 80, 8), FLUID_RESERVED_MASK);
        assert!(manager.set_waterlogged(15, 80, 8, true));
        assert_eq!(
            manager.get_fluid_raw(15, 80, 8),
            FLUID_RESERVED_MASK | FLUID_WATERLOGGED_BIT
        );
        assert!(manager.set_waterlogged(15, 80, 8, false));
        assert_eq!(manager.get_fluid_raw(15, 80, 8), FLUID_RESERVED_MASK);
        manager.set_block(15, 80, 8, BlockType::Stone);
        assert_eq!(manager.get_fluid_raw(15, 80, 8), 0);
        assert!(!manager.is_waterlogged(15, 80, 8));
    }

    #[test]
    fn section_dependencies_include_xyz_edges_and_corners() {
        let mut set = HashSet::new();
        mark_section_mesh_dependencies(&mut set, 15, 15, 15);
        assert_eq!(set.len(), 8);
        assert!(set.contains(&SectionKey::new(0, 0, 0)));
        assert!(set.contains(&SectionKey::new(1, 1, 1)));
        let mut interior = HashSet::new();
        mark_section_mesh_dependencies(&mut interior, 7, 7, 7);
        assert_eq!(interior, [SectionKey::new(0, 0, 0)].into_iter().collect());
    }

    #[test]
    fn capture_section_halo_uses_neighbor_columns() {
        let mut manager = ChunkManager::new(0);
        let mut center = Chunk::empty(0, 0);
        let mut east = Chunk::empty(1, 0);
        center.set_block_local(15, 8, 8, BlockType::Stone);
        east.set_block_local(0, 8, 8, BlockType::Dirt);
        east.set_sky_light(0, 8, 8, 9);
        manager.chunks.insert((0, 0), center);
        manager.chunks.insert((1, 0), east);

        let halo = manager.capture_section_halo(SectionKey::new(0, 0, 0));
        assert_eq!(halo.get_block(16, 9, 9), BlockType::Stone);
        assert_eq!(halo.get_block(17, 9, 9), BlockType::Dirt);
        assert_eq!(halo.get(17, 9, 9).sky, 9);
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
        manager.chunks.insert((0, 0), Chunk::empty(0, 0));
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

        manager.chunks.insert((1, 0), Chunk::empty(1, 0));
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
        manager.chunks.insert((0, 0), Chunk::empty(0, 0));
        manager.set_block(15, 99, 8, BlockType::Sand);
        manager.set_block(15, 100, 8, BlockType::Cactus);
        assert_eq!(
            manager.block_support_status(BlockType::Cactus, 15, 100, 8),
            BlockSupportStatus::Unknown
        );

        manager.chunks.insert((1, 0), Chunk::empty(1, 0));
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

    #[test]
    fn container_slot_commit_rejects_invalid_stack_without_partial_write() {
        let mut chunk_manager = ChunkManager::new(2);
        chunk_manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));
        chunk_manager.set_block(0, 64, 0, crate::world::BlockType::Chest);
        chunk_manager.set_block_entity(
            0,
            64,
            0,
            Some(BlockEntity::Chest(
                crate::block_entity::ChestBlockEntity::new(),
            )),
        );
        let original = chunk_manager.container_slots(0, 64, 0).unwrap();
        let mut invalid = original.clone();
        invalid[3] = Some(ItemStack::new(crate::inventory::Item::Stone, 0));

        assert!(!chunk_manager.set_container_slots(0, 64, 0, &invalid));
        assert_eq!(chunk_manager.container_slots(0, 64, 0).unwrap(), original);
    }
}
