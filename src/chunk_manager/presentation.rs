//! Presentation-owned resident columns: dense grid, section mesh dirty sets,
//! and view distance. Does not enqueue fluids or mark save-dirty.

use super::{
    mark_block_mesh_dependencies, mark_section_mesh_dependencies, world_to_local, ColumnQuery,
    DenseColumnGrid, LightColumnHost,
};
use crate::world::{
    BlockType, Chunk, MeshVoxel, SectionHaloSnapshot, SectionKey, CHUNK_DEPTH, CHUNK_WIDTH,
};
use std::collections::HashSet;

pub struct PresentationChunks {
    pub chunks: DenseColumnGrid,
    pub view_distance: i32,
    pub dimension: crate::dimension::Dimension,
    load_generation: u64,
    pending_mesh_invalidations: HashSet<(i32, i32)>,
    pending_section_mesh_invalidations: HashSet<SectionKey>,
}

impl ColumnQuery for PresentationChunks {
    fn column_grid(&self) -> &DenseColumnGrid {
        &self.chunks
    }

    fn column_dimension(&self) -> crate::dimension::Dimension {
        self.dimension
    }
}

impl LightColumnHost for PresentationChunks {
    fn dimension(&self) -> crate::dimension::Dimension {
        self.dimension
    }

    fn chunks(&self) -> &DenseColumnGrid {
        &self.chunks
    }

    fn chunks_mut(&mut self) -> &mut DenseColumnGrid {
        &mut self.chunks
    }

    fn note_light_cell_change(&mut self, wx: i32, wy: i32, wz: i32) {
        if world_to_local(self.dimension, wx, wy, wz).is_some() {
            self.record_mesh_invalidation(wx, wy, wz);
        }
    }

    fn get_block(&self, wx: i32, wy: i32, wz: i32) -> BlockType {
        PresentationChunks::get_block(self, wx, wy, wz)
    }

    fn get_sky_light(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        PresentationChunks::get_sky_light(self, wx, wy, wz)
    }

    fn set_sky_light(&mut self, wx: i32, wy: i32, wz: i32, val: u8) {
        PresentationChunks::set_sky_light(self, wx, wy, wz, val)
    }

    fn get_block_light(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        PresentationChunks::get_block_light(self, wx, wy, wz)
    }

    fn set_block_light(&mut self, wx: i32, wy: i32, wz: i32, val: u8) {
        PresentationChunks::set_block_light(self, wx, wy, wz, val)
    }

    fn column_neighborhood(&self, cx: i32, cz: i32) -> [[Option<&Chunk>; 3]; 3] {
        PresentationChunks::column_neighborhood(self, cx, cz)
    }
}

impl PresentationChunks {
    pub fn new(view_distance: i32) -> Self {
        Self::new_in_dimension(view_distance, crate::dimension::Dimension::Overworld)
    }

    pub fn new_in_dimension(
        view_distance: i32,
        dimension: crate::dimension::Dimension,
    ) -> Self {
        let view_distance = view_distance.max(0);
        Self {
            chunks: DenseColumnGrid::with_distance(view_distance),
            view_distance,
            dimension,
            load_generation: 0,
            pending_mesh_invalidations: HashSet::new(),
            pending_section_mesh_invalidations: HashSet::new(),
        }
    }

    pub fn load_generation(&self) -> u64 {
        self.load_generation
    }

    pub fn bump_load_generation(&mut self) {
        self.load_generation = self.load_generation.wrapping_add(1);
    }

    pub fn insert_resident_chunk(&mut self, key: (i32, i32), chunk: Chunk) {
        self.chunks.insert(key, chunk);
        self.bump_load_generation();
    }

    pub fn remove_resident_chunk(&mut self, key: &(i32, i32)) -> Option<Chunk> {
        let removed = self.chunks.remove(key)?;
        self.bump_load_generation();
        Some(removed)
    }

    pub fn recenter(&mut self, center_cx: i32, center_cz: i32) {
        self.chunks.recenter(center_cx, center_cz);
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

    pub fn world_to_local(
        &self,
        wx: i32,
        wy: i32,
        wz: i32,
    ) -> Option<((i32, i32), (usize, i32, usize))> {
        world_to_local(self.dimension, wx, wy, wz)
    }

    pub fn get_block(&self, wx: i32, wy: i32, wz: i32) -> BlockType {
        ColumnQuery::get_block(self, wx, wy, wz)
    }

    pub fn get_loaded_block(&self, wx: i32, wy: i32, wz: i32) -> Option<BlockType> {
        ColumnQuery::get_loaded_block(self, wx, wy, wz)
    }

    pub fn highest_solid_y(&self, wx: i32, wz: i32) -> Option<i32> {
        ColumnQuery::highest_solid_y(self, wx, wz)
    }

    pub fn column_neighborhood(&self, cx: i32, cz: i32) -> [[Option<&Chunk>; 3]; 3] {
        ColumnQuery::column_neighborhood(self, cx, cz)
    }

    pub fn is_block_loaded(&self, x: i32, _y: i32, z: i32) -> bool {
        let cx = x.div_euclid(CHUNK_WIDTH as i32);
        let cz = z.div_euclid(CHUNK_DEPTH as i32);
        self.chunks.contains_key(&(cx, cz))
    }

    pub fn get_block_state(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.get_block_state(bx as i32, by as i32, bz as i32);
            }
        }
        0
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
        if let Some((_, (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            let cx = wx.div_euclid(CHUNK_WIDTH as i32);
            let cz = wz.div_euclid(CHUNK_DEPTH as i32);
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                if chunk.get_sky_light(bx, by, bz) != val {
                    chunk.set_sky_light(bx, by, bz, val);
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
        if let Some((_, (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            let cx = wx.div_euclid(CHUNK_WIDTH as i32);
            let cz = wz.div_euclid(CHUNK_DEPTH as i32);
            if let Some(chunk) = self.chunks.get_mut(&(cx, cz)) {
                if chunk.get_block_light(bx, by, bz) != val {
                    chunk.set_block_light(bx, by, bz, val);
                    self.record_mesh_invalidation(wx, wy, wz);
                }
            }
        }
    }

    pub fn get_fluid_raw(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        if let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) {
            if let Some(chunk) = self.chunks.get(&(cx, cz)) {
                return chunk.get_fluid_level(bx, by, bz);
            }
        }
        0
    }

    pub fn get_fluid_level(&self, wx: i32, wy: i32, wz: i32) -> u8 {
        self.get_fluid_raw(wx, wy, wz) & crate::world::FLUID_LEVEL_MASK
    }

    /// Write a revision-gated projection cell without authority fluid side
    /// effects. Does not enqueue fluids or mark save-dirty.
    pub fn apply_presentation_cell(
        &mut self,
        wx: i32,
        wy: i32,
        wz: i32,
        block: BlockType,
        state: u8,
        raw_fluid: u8,
    ) -> bool {
        let Some(((cx, cz), (bx, by, bz))) = self.world_to_local(wx, wy, wz) else {
            return false;
        };
        let Some(chunk) = self.chunks.get_mut(&(cx, cz)) else {
            return false;
        };
        let previous = chunk.get_block_local(bx, by, bz);
        let previous_state = chunk.get_block_state(bx as i32, by as i32, bz as i32);
        let previous_fluid = chunk.get_fluid_level(bx, by, bz);
        if previous == block && previous_state == state && previous_fluid == raw_fluid {
            return false;
        }
        if previous != block {
            chunk.set_block_local(bx, by, bz, block);
            chunk.update_heightmap(bx, bz);
        }
        if previous_state != state || previous != block {
            chunk.set_block_state(bx as i32, by as i32, bz as i32, state);
        }
        if previous_fluid != raw_fluid {
            chunk.set_fluid_level(bx, by, bz, raw_fluid);
        }
        self.record_mesh_invalidation(wx, wy, wz);
        true
    }

    /// Insert a join-client column from a revision-gated `ChunkData` payload.
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
                self.record_mesh_invalidation(wx, wy, wz);
            }
        }
    }

    pub fn container_slot_count(&self, x: i32, y: i32, z: i32) -> usize {
        if let Some(entity) = self.get_block_entity(x, y, z) {
            if matches!(entity, crate::block_entity::BlockEntity::Chest(_))
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

    pub fn container_slots(
        &self,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<Vec<Option<crate::inventory::ItemStack>>> {
        let entity = self.get_block_entity(x, y, z)?;
        if matches!(entity, crate::block_entity::BlockEntity::Chest(_)) {
            let primary = match entity {
                crate::block_entity::BlockEntity::Chest(chest) => chest.inventory.clone(),
                _ => return None,
            };
            let state = crate::world::BlockState::decode(self.get_block_state(x, y, z));
            if let Some(partner_pos) = crate::block_entity::double_chest_partner(self, (x, y, z)) {
                if let Some(crate::block_entity::BlockEntity::Chest(partner)) =
                    self.get_block_entity(partner_pos.0, partner_pos.1, partner_pos.2)
                {
                    let mut combined = vec![None; 54];
                    if state.chest_type == crate::world::ChestType::Left {
                        combined[..27].clone_from_slice(&primary.slots);
                        combined[27..54].clone_from_slice(&partner.inventory.slots);
                    } else {
                        combined[..27].clone_from_slice(&partner.inventory.slots);
                        combined[27..54].clone_from_slice(&primary.slots);
                    }
                    return Some(combined);
                }
            }
            return Some(primary.slots.to_vec());
        }
        Some(
            (0..entity.slot_count())
                .map(|slot| entity.get_stack(slot).copied())
                .collect(),
        )
    }

    pub fn set_container_slots(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        slots: &[Option<crate::inventory::ItemStack>],
    ) -> bool {
        if slots.iter().flatten().any(|stack| {
            stack.count == 0
                || stack.item == crate::inventory::Item::Air
                || stack.count > stack.item.properties().max_stack
        }) {
            return false;
        }
        if matches!(
            self.get_block_entity(x, y, z),
            Some(crate::block_entity::BlockEntity::Chest(_))
        ) {
            if slots.len() == 54 {
                let state = crate::world::BlockState::decode(self.get_block_state(x, y, z));
                let Some(partner_pos) =
                    crate::block_entity::double_chest_partner(self, (x, y, z))
                else {
                    return false;
                };
                let (primary_slice, partner_slice) =
                    if state.chest_type == crate::world::ChestType::Left {
                        (&slots[..27], &slots[27..54])
                    } else {
                        (&slots[27..54], &slots[..27])
                    };
                let primary = self.get_block_entity(x, y, z).and_then(|entity| match entity {
                    crate::block_entity::BlockEntity::Chest(chest) => Some(chest.clone()),
                    _ => None,
                });
                let partner = self
                    .get_block_entity(partner_pos.0, partner_pos.1, partner_pos.2)
                    .and_then(|entity| match entity {
                        crate::block_entity::BlockEntity::Chest(chest) => Some(chest.clone()),
                        _ => None,
                    });
                let (Some(mut primary), Some(mut partner)) = (primary, partner) else {
                    return false;
                };
                let mut p_arr = [None; 27];
                p_arr.copy_from_slice(primary_slice);
                let mut pt_arr = [None; 27];
                pt_arr.copy_from_slice(partner_slice);
                primary.inventory = crate::inventory::ContainerInventory { slots: p_arr };
                primary.revision = primary.revision.wrapping_add(1);
                partner.inventory = crate::inventory::ContainerInventory { slots: pt_arr };
                partner.revision = partner.revision.wrapping_add(1);
                self.set_block_entity(
                    x,
                    y,
                    z,
                    Some(crate::block_entity::BlockEntity::Chest(primary)),
                );
                self.set_block_entity(
                    partner_pos.0,
                    partner_pos.1,
                    partner_pos.2,
                    Some(crate::block_entity::BlockEntity::Chest(partner)),
                );
                return true;
            }
            if slots.len() == 27 {
                let Some(crate::block_entity::BlockEntity::Chest(mut chest)) =
                    self.get_block_entity(x, y, z).cloned()
                else {
                    return false;
                };
                let mut arr = [None; 27];
                arr.copy_from_slice(slots);
                chest.inventory = crate::inventory::ContainerInventory { slots: arr };
                chest.revision = chest.revision.wrapping_add(1);
                self.set_block_entity(x, y, z, Some(crate::block_entity::BlockEntity::Chest(chest)));
                return true;
            }
            return false;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::BlockType;

    #[test]
    fn presentation_cell_marks_mesh_not_fluids() {
        let mut manager = PresentationChunks::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.chunks.insert((1, 0), Chunk::new(1, 0));
        assert!(manager.apply_presentation_cell(15, 80, 8, BlockType::Stone, 0, 0));
        assert_eq!(
            manager.drain_mesh_invalidations(),
            HashSet::from([(0, 0), (1, 0)])
        );
        assert!(manager.drain_mesh_invalidations().is_empty());
    }

    #[test]
    fn set_sky_light_marks_section_mesh_dirty() {
        let mut manager = PresentationChunks::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.set_sky_light(8, 80, 8, 10);
        assert!(!manager.drain_section_mesh_invalidations().is_empty());
    }
}
