use super::*;
use super::entities::block_revision_fingerprint;

impl ServerWorld {
    pub fn new_with_difficulty(
        seed: u32,
        dimension: Dimension,
        world_type: WorldType,
        generate_structures: bool,
        rules: WorldRules,
        simulation_distance: i32,
        difficulty: Difficulty,
    ) -> Self {
        let mut world = Self {
            seed,
            dimension,
            world_type,
            generate_structures,
            rules: rules.normalized(),
            difficulty,
            time: 0,
            revisions: RevisionClock::new(),
            chunks: WorldColumns::new_in_dimension(simulation_distance.max(1), dimension),
            // Network player ids occupy the low numeric lane while authority-
            // created transient ids reserve the high bit. Persistent world
            // entities use the middle lane so a combat target can never be
            // mistaken for the authenticated player with the same raw id.
            entities: EntityManager::new_with_id_base(1u64 << 32),
            redstone: RedstoneSystem::new(),
            recipe_manager: crate::recipes::RecipeManager::new(),
            container_viewers: BTreeMap::new(),
            pending_container_closures: Vec::new(),
            pending_mutations: Vec::new(),
            pending_redstone_actions: Vec::new(),
            block_revisions: BTreeMap::new(),
            block_revision_checksum: 0,
            chunk_revisions: BTreeMap::new(),
            failed_restore_chunks: BTreeSet::new(),
            pending_chunk_generation: BTreeSet::new(),
            worldgen_mode: WorldgenMode::Sync,
            entities_persisted_epoch: 0,
            plate_occupants: Vec::new(),
            plate_occupant_columns: Vec::new(),
        };
        world.ensure_chunk(0, 0);
        world
    }

    pub fn set_worldgen_mode(&mut self, mode: WorldgenMode) {
        self.worldgen_mode = mode;
    }

    pub fn worldgen_mode(&self) -> WorldgenMode {
        self.worldgen_mode
    }

    pub fn pending_chunk_generation(&self) -> &BTreeSet<(i32, i32)> {
        &self.pending_chunk_generation
    }

    pub fn entities_dirty_for_save(&self) -> bool {
        self.entities.checksum_epoch() != self.entities_persisted_epoch
    }

    pub fn acknowledge_entities_persisted(&mut self) {
        self.entities_persisted_epoch = self.entities.checksum_epoch();
    }

    /// Insert a worker-generated column when still demanded and not
    /// fail-closed. Stale results for already-resident columns are ignored.
    pub fn apply_generated_chunk(&mut self, chunk_x: i32, chunk_z: i32, chunk: crate::world::Chunk) {
        let key = (chunk_x, chunk_z);
        self.pending_chunk_generation.remove(&key);
        if self.chunks.chunks.contains_key(&key) || self.failed_restore_chunks.contains(&key) {
            return;
        }
        self.chunks.insert_resident_chunk(key, chunk);
    }

    /// Whether a future/other authoritative spawn source may create a
    /// hostile entity.  Peaceful is an independent policy from the
    /// `do_mob_spawning` gamerule; the latter never freezes already-loaded
    /// hostiles.
    pub const fn allows_hostile_spawning(&self) -> bool {
        self.rules.do_mob_spawning && !matches!(self.difficulty, Difficulty::Peaceful)
    }

    /// Synchronously generate a missing column regardless of `worldgen_mode`.
    /// Used for spawn bootstrap and paths that cannot wait a tick.
    pub fn materialize_chunk(&mut self, chunk_x: i32, chunk_z: i32) {
        if self.chunks.chunks.contains_key(&(chunk_x, chunk_z))
            || self.failed_restore_chunks.contains(&(chunk_x, chunk_z))
        {
            return;
        }
        self.pending_chunk_generation.remove(&(chunk_x, chunk_z));
        let options = WorldGenerationOptions {
            world_type: self.world_type,
            generate_structures: self.generate_structures,
        };
        let chunk =
            generate_chunk_with_options(self.dimension, chunk_x, chunk_z, self.seed, options);
        self.chunks
            .insert_resident_chunk((chunk_x, chunk_z), chunk);
    }

    pub fn ensure_chunk(&mut self, chunk_x: i32, chunk_z: i32) {
        if self.chunks.chunks.contains_key(&(chunk_x, chunk_z))
            || self.failed_restore_chunks.contains(&(chunk_x, chunk_z))
        {
            return;
        }
        match self.worldgen_mode {
            WorldgenMode::Async => {
                self.pending_chunk_generation.insert((chunk_x, chunk_z));
            }
            WorldgenMode::Sync => {
                self.materialize_chunk(chunk_x, chunk_z);
            }
        }
    }

    pub fn chunk_is_resident(&self, chunk_x: i32, chunk_z: i32) -> bool {
        self.chunks.chunks.contains_key(&(chunk_x, chunk_z))
    }

    pub fn valid_coordinate(&self, x: i32, y: i32, z: i32) -> bool {
        self.dimension.height().contains_y(y)
            && x.unsigned_abs() <= WORLD_BOUND as u32
            && z.unsigned_abs() <= WORLD_BOUND as u32
    }

    pub fn safe_spawn_y(&mut self, x: i32, z: i32) -> i32 {
        let (cx, cz) = chunk_xz(x, z);
        self.materialize_chunk(cx, cz);
        let height = self.dimension.height();
        for y in (height.min_y()..height.max_y_exclusive()).rev() {
            if self.get_block(x, y, z).properties().is_solid {
                return (y + 1).clamp(height.min_y() + 1, height.max_y_exclusive() - 5);
            }
        }
        64
    }

    pub fn get_block(&self, x: i32, y: i32, z: i32) -> BlockType {
        self.chunks.get_block(x, y, z)
    }

    pub fn get_block_state(&self, x: i32, y: i32, z: i32) -> u8 {
        self.chunks.get_block_state(x, y, z)
    }

    pub fn get_block_entity(&self, x: i32, y: i32, z: i32) -> Option<&BlockEntity> {
        self.chunks.get_block_entity(x, y, z)
    }

    pub fn ensure_chest_loot(&mut self, position: (i32, i32, i32)) {
        let pending = matches!(
            self.get_block_entity(position.0, position.1, position.2),
            Some(BlockEntity::Chest(chest)) if chest.loot_table.is_some()
        );
        if !pending {
            return;
        }
        let revision = self.revisions.allocate();
        if let Some(BlockEntity::Chest(chest)) = self
            .chunks
            .get_block_entity_mut(position.0, position.1, position.2)
        {
            chest.ensure_loot_generated(self.seed, position);
            chest.revision = revision;
        }
        self.chunks
            .mark_block_entity_dirty(position.0, position.2);
        self.set_block_revision(position, revision);
        self.chunk_revisions.insert(
            chunk_xz(position.0, position.2),
            revision,
        );
        self.pending_mutations.push(WorldMutation {
            dimension: self.dimension as u8,
            position,
            block: self.get_block(position.0, position.1, position.2).to_wire(),
            state: self.get_block_state(position.0, position.1, position.2),
            raw_fluid: self
                .chunks
                .get_fluid_raw(position.0, position.1, position.2),
            revision,
        });
    }

    /// Return a serializable view of a container slot for the transport
    /// adapter. `None` is a valid empty slot; an out-of-range slot returns
    /// `None` as well and is rejected by dispatch before this helper is used.
    pub fn container_slot_wire(
        &mut self,
        position: (i32, i32, i32),
        slot: u16,
    ) -> Option<Option<ItemWire>> {
        self.ensure_chest_loot(position);
        let entity = self.get_block_entity(position.0, position.1, position.2)?;
        let access = ContainerAccess::for_entity(entity)?;
        if usize::from(slot) >= access.slot_count {
            return None;
        }
        let stack = match entity {
            BlockEntity::Chest(chest) => chest.inventory.slots[usize::from(slot)],
            BlockEntity::Furnace(furnace) => furnace.slots[usize::from(slot)],
            BlockEntity::Hopper(hopper) => hopper.slots[usize::from(slot)],
            BlockEntity::Dispenser(dispenser) => dispenser.slots[usize::from(slot)],
            BlockEntity::Dropper(dropper) => dropper.slots[usize::from(slot)],
            BlockEntity::Sign(_) | BlockEntity::Spawner(_) | BlockEntity::Observer(_) => None,
        };
        Some(stack.as_ref().map(ItemWire::from_stack))
    }

    pub fn container_item_slots(
        &self,
        position: (i32, i32, i32),
    ) -> Option<Vec<Option<ItemStack>>> {
        self.chunks
            .container_slots(position.0, position.1, position.2)
    }

    pub fn commit_container_item_slots(
        &mut self,
        position: (i32, i32, i32),
        slots: &[Option<ItemStack>],
    ) -> Result<WorldMutation, RejectReason> {
        if !self
            .chunks
            .set_container_slots(position.0, position.1, position.2, slots)
        {
            return Err(RejectReason::InvalidState);
        }
        self.redstone.mark_container_changed(&self.chunks, position);
        Ok(self.touch_revision(position.0, position.1, position.2))
    }

    pub fn container_slots_wire(
        &mut self,
        position: (i32, i32, i32),
    ) -> Option<Vec<Option<ItemWire>>> {
        self.ensure_chest_loot(position);
        let count = {
            let entity = self.get_block_entity(position.0, position.1, position.2)?;
            ContainerAccess::for_entity(entity)?.slot_count
        };
        (0..count)
            .map(|slot| self.container_slot_wire(position, slot as u16))
            .collect()
    }

    /// Highest authoritative mutation revision recorded in a chunk. This is
    /// persisted alongside the chunk payload so an unload/reload cannot make
    /// a stale client delta appear newer than the saved world.
    pub fn chunk_revision(&self, chunk_x: i32, chunk_z: i32) -> u64 {
        self.chunk_revisions
            .get(&(chunk_x, chunk_z))
            .copied()
            .unwrap_or_else(|| {
                self.block_revisions
                    .get(&(chunk_x, chunk_z))
                    .and_then(|column| column.values().copied().max())
                    .unwrap_or(0)
            })
    }

    /// Build the bounded per-chunk revision index consumed by SaveManager.
    pub fn mutation_revision_index(&self) -> MutationRevisionIndex {
        let mut index = MutationRevisionIndex::default();
        for (&(chunk_x, chunk_z), &revision) in &self.chunk_revisions {
            let _ = index.ensure_at_least(self.dimension, chunk_x, chunk_z, revision);
        }
        for (&(chunk_x, chunk_z), column) in &self.block_revisions {
            for &revision in column.values() {
                let _ = index.ensure_at_least(self.dimension, chunk_x, chunk_z, revision);
            }
        }
        index
    }

    /// Columns that failed inflate/length checks during restore. `save_all`
    /// must never write these coordinates.
    pub fn failed_restore_chunks(&self) -> &BTreeSet<(i32, i32)> {
        &self.failed_restore_chunks
    }

    pub(super) fn column_in_set(&self, columns: &BTreeSet<(i32, i32)>, x: i32, z: i32) -> bool {
        columns.contains(&chunk_xz(x, z))
    }

    /// Serialize one resident column for persistence. Failed-restore
    /// coordinates are omitted so `save_all` cannot generate-over-save.
    pub fn chunk_save_payload(&self, cx: i32, cz: i32) -> Option<ChunkSaveData> {
        if self.failed_restore_chunks.contains(&(cx, cz)) {
            return None;
        }
        let chunk = self.chunks.chunks.get(&(cx, cz))?;
        let metadata = self.redstone.collect_chunk_metadata(&self.chunks, cx, cz);
        let mut data = ChunkSaveData::from_chunk_with_redstone(chunk, &metadata).ok()?;
        data.mutation_revision = self.chunk_revision(cx, cz);
        Some(data)
    }

    /// Drop columns outside `keep` after the caller has flushed dirty ones
    /// that this session restored or generated. A failed serialize/save leaves
    /// the column resident so player builds are not discarded.
    pub fn evict_unkept_chunks<F>(&mut self, keep: &BTreeSet<(i32, i32)>, mut flush: F)
    where
        F: FnMut(i32, i32, ChunkSaveData) -> std::io::Result<()>,
    {
        let mut evict: Vec<_> = self
            .chunks
            .chunks
            .keys()
            .filter(|key| !keep.contains(key))
            .collect();
        evict.sort_unstable();
        for (cx, cz) in evict {
            if self.failed_restore_chunks.contains(&(cx, cz)) {
                self.remove_resident_chunk(cx, cz);
                continue;
            }
            let dirty = self.chunks.dirty_chunks.is_dirty(cx, cz);
            if dirty {
                let Some(data) = self.chunk_save_payload(cx, cz) else {
                    continue;
                };
                if flush(cx, cz, data).is_err() {
                    continue;
                }
            }
            self.remove_resident_chunk(cx, cz);
        }
    }

    pub(super) fn remove_resident_chunk(&mut self, cx: i32, cz: i32) {
        self.chunks.remove_resident_chunk(&(cx, cz));
        self.chunks.dirty_chunks.remove(cx, cz);
        self.chunk_revisions.remove(&(cx, cz));
        if let Some(column) = self.block_revisions.remove(&(cx, cz)) {
            for (position, revision) in column {
                self.block_revision_checksum ^= block_revision_fingerprint(position, revision);
            }
        }
    }

    pub(super) fn set_block_revision(&mut self, position: (i32, i32, i32), revision: u64) {
        let column = chunk_xz(position.0, position.2);
        let column_map = self.block_revisions.entry(column).or_default();
        if let Some(old) = column_map.insert(position, revision) {
            if old == revision {
                return;
            }
            self.block_revision_checksum ^= block_revision_fingerprint(position, old);
        }
        self.block_revision_checksum ^= block_revision_fingerprint(position, revision);
    }

    #[allow(dead_code)]
    pub(super) fn clear_block_revision(&mut self, position: (i32, i32, i32)) {
        let column = chunk_xz(position.0, position.2);
        let Some(column_map) = self.block_revisions.get_mut(&column) else {
            return;
        };
        if let Some(old) = column_map.remove(&position) {
            self.block_revision_checksum ^= block_revision_fingerprint(position, old);
        }
        if column_map.is_empty() {
            self.block_revisions.remove(&column);
        }
    }

    /// Restore a persisted chunk into the authoritative map. The payload is
    /// decoded before any insert so a corrupt inner zlib cannot be replaced
    /// by generated terrain and then saved back over player builds.
    pub fn restore_saved_chunk(&mut self, data: &ChunkSaveData) -> std::io::Result<()> {
        let key = (data.chunk_x, data.chunk_z);
        let mut decoded =
            crate::world::Chunk::empty_in_dimension(self.dimension, data.chunk_x, data.chunk_z);
        if let Err(error) = data.restore_to_chunk(&mut decoded) {
            self.failed_restore_chunks.insert(key);
            let _ = self.chunks.remove_resident_chunk(&key);
            self.chunk_revisions.remove(&key);
            return Err(error);
        }
        self.chunks.insert_resident_chunk(key, decoded);
        let redstone_metadata = data.redstone_metadata();
        self.redstone.restore_chunk_metadata(
            &self.chunks,
            data.chunk_x,
            data.chunk_z,
            &redstone_metadata,
        );
        self.chunk_revisions.insert(key, data.mutation_revision);
        self.revisions.observe(data.mutation_revision);
        Ok(())
    }

    /// Restore persistent entities once during authority startup. Entity IDs
    /// are reallocated by EntityManager; gameplay state, ownership and item
    /// metadata remain in the serialized EntitySaveData payload.
    pub fn restore_saved_entities(&mut self, data: &[EntitySaveData]) {
        self.entities = EntityManager::new();
        for entity in data {
            self.entities.add_restored_entity(entity);
        }
        self.acknowledge_entities_persisted();
    }

}

