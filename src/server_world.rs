//! The GPU-independent authoritative world.
//!
//! `ServerWorld` owns simulation state, not transport.  It uses the existing
//! CPU voxel/entity primitives (which are also used by headless tests) and
//! never imports wgpu, winit, audio, camera, or UI modules.

use crate::authority::contract::{
    AuthoritySnapshot, RevisionClock, SessionFishingHookState, SessionGameplayState,
    SessionInventorySlot, WorldMutation,
};
use crate::authority::fishing::{FishingDomainContext, FishingDomainError};
use crate::authority::transactions::{self, WorkstationContext};
use crate::block_entity::{default_stub_for_block, BlockEntity, ContainerAccess};
use crate::chunk_manager::ChunkManager;
use crate::commands::{self, Command, TimeCommand};
use crate::dimension::{generate_chunk_with_options, Dimension, WorldGenerationOptions};
use crate::entity::{EntityManager, EntityType};
use crate::game_rules::{ServerDifficulty, WorldRules, WorldType};
use crate::network::protocol::{
    ContainerAction, GameplayOperation, GameplayRequest, ItemWire, PlayerId, RejectReason,
};
use crate::redstone::RedstoneSystem;
use crate::save::{ChunkSaveData, EntitySaveData, MutationRevisionIndex};
use crate::world::BlockType;
use glam::Vec3;
use std::collections::{BTreeMap, BTreeSet};

const WORLD_BOUND: i32 = 30_000_000;
const FIXED_DT: f32 = 1.0 / 20.0;
const MAX_AUTOMATION_TRANSFERS: usize = 64;
const MAX_FLUID_UPDATES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldDispatchError {
    reason: RejectReason,
}

impl WorldDispatchError {
    pub const fn new(reason: RejectReason) -> Self {
        Self { reason }
    }

    pub const fn reason(self) -> RejectReason {
        self.reason
    }
}

/// All state required to advance one deterministic world tick.
pub struct ServerWorld {
    pub seed: u32,
    pub dimension: Dimension,
    pub world_type: WorldType,
    pub generate_structures: bool,
    pub rules: WorldRules,
    /// Server-owned difficulty shared by every loaded dimension.  It is kept
    /// outside `WorldRules` so adding this policy does not invalidate the
    /// existing binary level-save layout; `server.properties` is its durable
    /// source of truth.
    pub difficulty: ServerDifficulty,
    pub time: u64,
    pub revisions: RevisionClock,
    pub chunks: ChunkManager,
    pub entities: EntityManager,
    pub redstone: RedstoneSystem,
    pub recipe_manager: crate::crafting::RecipeManager,
    pub container_viewers: BTreeMap<(i32, i32, i32), BTreeSet<PlayerId>>,
    pub sleeping_players: BTreeSet<PlayerId>,
    block_revisions: BTreeMap<(i32, i32, i32), u64>,
    chunk_revisions: BTreeMap<(i32, i32), u64>,
    pub last_snapshot: AuthoritySnapshot,
}

impl ServerWorld {
    pub fn new(
        seed: u32,
        dimension: Dimension,
        world_type: WorldType,
        generate_structures: bool,
        rules: WorldRules,
        render_distance: i32,
    ) -> Self {
        Self::new_with_difficulty(
            seed,
            dimension,
            world_type,
            generate_structures,
            rules,
            render_distance,
            ServerDifficulty::default(),
        )
    }

    pub fn new_with_difficulty(
        seed: u32,
        dimension: Dimension,
        world_type: WorldType,
        generate_structures: bool,
        rules: WorldRules,
        render_distance: i32,
        difficulty: ServerDifficulty,
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
            chunks: ChunkManager::new_in_dimension(render_distance.max(1), dimension),
            entities: EntityManager::new(),
            redstone: RedstoneSystem::new(),
            recipe_manager: crate::crafting::RecipeManager::new(),
            container_viewers: BTreeMap::new(),
            sleeping_players: BTreeSet::new(),
            block_revisions: BTreeMap::new(),
            chunk_revisions: BTreeMap::new(),
            last_snapshot: AuthoritySnapshot::empty(),
        };
        world.ensure_chunk(0, 0);
        world
    }

    /// Whether a future/other authoritative spawn source may create a
    /// hostile entity.  Peaceful is an independent policy from the
    /// `do_mob_spawning` gamerule; the latter never freezes already-loaded
    /// hostiles.
    pub const fn allows_hostile_spawning(&self) -> bool {
        self.rules.do_mob_spawning && !matches!(self.difficulty, ServerDifficulty::Peaceful)
    }

    pub fn ensure_chunk(&mut self, chunk_x: i32, chunk_z: i32) {
        if self.chunks.chunks.contains_key(&(chunk_x, chunk_z)) {
            return;
        }
        let options = WorldGenerationOptions {
            world_type: self.world_type,
            generate_structures: self.generate_structures,
        };
        let chunk =
            generate_chunk_with_options(self.dimension, chunk_x, chunk_z, self.seed, options);
        self.chunks.chunks.insert((chunk_x, chunk_z), chunk);
    }

    pub fn valid_coordinate(&self, x: i32, y: i32, z: i32) -> bool {
        self.dimension.height().contains_y(y)
            && x.unsigned_abs() <= WORLD_BOUND as u32
            && z.unsigned_abs() <= WORLD_BOUND as u32
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

    /// Return a serializable view of a container slot for the transport
    /// adapter. `None` is a valid empty slot; an out-of-range slot returns
    /// `None` as well and is rejected by dispatch before this helper is used.
    pub fn container_slot_wire(
        &self,
        position: (i32, i32, i32),
        slot: u16,
    ) -> Option<Option<ItemWire>> {
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

    pub fn container_slots_wire(&self, position: (i32, i32, i32)) -> Option<Vec<Option<ItemWire>>> {
        let entity = self.get_block_entity(position.0, position.1, position.2)?;
        let count = ContainerAccess::for_entity(entity)?.slot_count;
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
                    .iter()
                    .filter(|((x, _y, z), _)| {
                        x.div_euclid(16) == chunk_x && z.div_euclid(16) == chunk_z
                    })
                    .map(|(_, revision)| *revision)
                    .max()
                    .unwrap_or(0)
            })
    }

    /// Build the bounded per-chunk revision index consumed by SaveManager.
    pub fn mutation_revision_index(&self) -> MutationRevisionIndex {
        let mut index = MutationRevisionIndex::default();
        for (&(chunk_x, chunk_z), &revision) in &self.chunk_revisions {
            let _ = index.ensure_at_least(self.dimension, chunk_x, chunk_z, revision);
        }
        for (&(x, _y, z), &revision) in &self.block_revisions {
            let _ =
                index.ensure_at_least(self.dimension, x.div_euclid(16), z.div_euclid(16), revision);
        }
        index
    }

    /// Restore a persisted chunk into the authoritative map. Existing
    /// generated terrain is replaced by the saved payload, while the world
    /// revision clock observes the payload revision before accepting requests.
    pub fn restore_saved_chunk(&mut self, data: &ChunkSaveData) {
        self.ensure_chunk(data.chunk_x, data.chunk_z);
        if let Some(chunk) = self.chunks.chunks.get_mut(&(data.chunk_x, data.chunk_z)) {
            data.restore_to_chunk(chunk);
        }
        self.chunk_revisions
            .insert((data.chunk_x, data.chunk_z), data.mutation_revision);
        self.revisions.observe(data.mutation_revision);
    }

    /// Restore persistent entities once during authority startup. Entity IDs
    /// are reallocated by EntityManager; gameplay state, ownership and item
    /// metadata remain in the serialized EntitySaveData payload.
    pub fn restore_saved_entities(&mut self, data: &[EntitySaveData]) {
        self.entities = EntityManager::new();
        for entity in data {
            self.entities.add_restored_entity(entity);
        }
    }

    /// Remove a player's container viewer registrations on logout, dimension
    /// change, or a failed reconnect. Empty viewer sets are pruned so they do
    /// not keep routing slots to a departed identity.
    pub fn close_container_viewers(&mut self, player_id: PlayerId) {
        self.container_viewers.retain(|_, viewers| {
            viewers.remove(&player_id);
            !viewers.is_empty()
        });
    }

    pub fn remove_passenger(&mut self, player_id: PlayerId) {
        for entity in &mut self.entities.entities {
            entity
                .passengers
                .retain(|passenger| *passenger != player_id);
        }
    }

    pub fn remove_authority_entity(&mut self, entity_id: u64) {
        let _ = self.entities.remove_by_id(entity_id);
    }

    pub fn fishing_context(
        &self,
        gameplay: &SessionGameplayState,
        player_position: [f32; 3],
        consume_durability: bool,
    ) -> Result<FishingDomainContext, FishingDomainError> {
        let hook = gameplay
            .fishing_hook
            .ok_or(FishingDomainError::NoActiveHook)?;
        let probe = crate::authority::fishing::water_probe_position(gameplay)?;
        let block_position = [
            probe[0].div_euclid(1_000),
            probe[1].div_euclid(1_000),
            probe[2].div_euclid(1_000),
        ];
        let open_water = self.get_block(block_position[0], block_position[1], block_position[2])
            == BlockType::Water;
        Ok(FishingDomainContext {
            world_seed: self.seed as u64 ^ (u64::from(self.dimension as u8) << 32),
            hook_entity_id: hook.entity_id,
            player_position_milli: position_to_milli(player_position)
                .ok_or(FishingDomainError::InvalidContext)?,
            open_water,
            water_surface_y_milli: open_water
                .then_some(block_position[1].saturating_mul(1_000).saturating_add(800)),
            consume_durability,
        })
    }

    pub fn sync_authority_hook(
        &mut self,
        previous: Option<SessionFishingHookState>,
        current: Option<SessionFishingHookState>,
        player_id: PlayerId,
    ) {
        if let Some(previous) = previous {
            if current.map_or(true, |current| current.entity_id != previous.entity_id) {
                let _ = self.entities.remove_by_id(previous.entity_id);
            }
        }
        let Some(current) = current else {
            return;
        };
        let position = milli_to_vec3(current.position_milli);
        let velocity = milli_to_vec3(current.velocity_milli);
        if let Some(entity) = self.entities.get_by_id_mut(current.entity_id) {
            entity.position = position;
            entity.velocity = velocity;
            entity.owner_id = Some(player_id);
        } else {
            let mut entity =
                crate::entity::Entity::new(current.entity_id, EntityType::FishingHook, position);
            entity.velocity = velocity;
            entity.owner_id = Some(player_id);
            self.entities.entities.push(entity);
        }
        self.entities.rebuild_indexes();
    }

    pub fn take_furnace_output(
        &mut self,
        gameplay: &mut SessionGameplayState,
        position: [i32; 3],
        count: u16,
    ) -> Result<WorldMutation, RejectReason> {
        let block = self.get_block(position[0], position[1], position[2]);
        let Some(BlockEntity::Furnace(furnace)) = self
            .get_block_entity(position[0], position[1], position[2])
            .cloned()
        else {
            return Err(RejectReason::InvalidState);
        };
        let mut next_furnace = furnace;
        transactions::execute_furnace_take_output(
            gameplay,
            &mut next_furnace,
            WorkstationContext::at(position, block),
            count,
        )
        .map_err(|_| RejectReason::InvalidState)?;
        self.chunks.set_block_entity(
            position[0],
            position[1],
            position[2],
            Some(BlockEntity::Furnace(next_furnace)),
        );
        Ok(self.touch_revision(position[0], position[1], position[2]))
    }

    pub fn bookshelf_power(&self, position: [i32; 3]) -> u8 {
        let mut count = 0u8;
        for y in [position[1], position[1] + 1] {
            for dx in -2i32..=2 {
                for dz in -2i32..=2 {
                    if dx.abs().max(dz.abs()) != 2 {
                        continue;
                    }
                    let gap = (position[0] + dx.signum(), y, position[2] + dz.signum());
                    if self.get_block(gap.0, gap.1, gap.2) == BlockType::Air
                        && self.get_block(position[0] + dx, y, position[2] + dz)
                            == BlockType::Bookshelf
                    {
                        count = count.saturating_add(1).min(15);
                    }
                }
            }
        }
        count
    }

    pub fn has_line_of_sight(&self, from: [f32; 3], to: [f32; 3]) -> bool {
        let origin = Vec3::from_array(from) + Vec3::new(0.0, 1.62, 0.0);
        let target = Vec3::from_array(to) + Vec3::new(0.0, 0.9, 0.0);
        !crate::culling::is_los_blocked(origin, target, |x, y, z| {
            crate::culling::is_section_occluder(self.get_block(x, y, z))
        })
    }

    pub fn spawn_authority_drop(
        &mut self,
        entity_id: u64,
        slot: SessionInventorySlot,
        position: [f32; 3],
    ) -> bool {
        if entity_id == 0 || self.entities.get_by_id(entity_id).is_some() {
            return false;
        }
        let Some(mut stack) = slot.item.to_stack() else {
            return false;
        };
        stack.can_break = slot.can_break;
        stack.can_place_on = slot.can_place_on;
        let mut entity = crate::entity::Entity::new(
            entity_id,
            EntityType::DroppedItem,
            Vec3::from_array(position),
        );
        entity.dropped_item = Some(stack.item);
        entity.dropped_count = stack.count;
        entity.dropped_stack = Some(stack);
        entity.pickup_cooldown = 0.5;
        self.entities.entities.push(entity);
        self.entities.rebuild_indexes();
        true
    }

    pub fn spawn_authority_experience(
        &mut self,
        entity_id: u64,
        experience: u32,
        position: [f32; 3],
    ) -> bool {
        if entity_id == 0 || experience == 0 || self.entities.get_by_id(entity_id).is_some() {
            return false;
        }
        let mut entity = crate::entity::Entity::new(
            entity_id,
            EntityType::ExperienceOrb,
            Vec3::from_array(position),
        );
        entity.xp_value = experience;
        self.entities.entities.push(entity);
        self.entities.rebuild_indexes();
        true
    }

    pub fn container_viewers_at(
        &self,
        position: (i32, i32, i32),
    ) -> impl Iterator<Item = &PlayerId> {
        self.container_viewers
            .get(&position)
            .into_iter()
            .flat_map(|viewers| viewers.iter())
    }

    /// Apply a real voxel mutation and return the revision-bearing event.
    pub fn set_block(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        block: BlockType,
        state: u8,
    ) -> Result<Option<WorldMutation>, WorldDispatchError> {
        if !self.valid_coordinate(x, y, z) {
            return Err(WorldDispatchError::new(RejectReason::InvalidCoordinate));
        }
        self.ensure_chunk(x.div_euclid(16), z.div_euclid(16));
        let old_block = self.get_block(x, y, z);
        let old_state = self.get_block_state(x, y, z);
        if old_block == block && old_state == state {
            return Ok(None);
        }
        self.chunks.set_block(x, y, z, block);
        self.chunks.set_block_state(x, y, z, state);
        if let Some(entity) = default_stub_for_block(block) {
            if self.chunks.get_block_entity(x, y, z).is_none() {
                self.chunks.set_block_entity(x, y, z, Some(entity));
            }
        } else if self.chunks.get_block_entity(x, y, z).is_some() {
            self.chunks.set_block_entity(x, y, z, None);
        }
        let revision = self.revisions.allocate();
        self.block_revisions.insert((x, y, z), revision);
        self.chunk_revisions
            .insert((x.div_euclid(16), z.div_euclid(16)), revision);
        Ok(Some(WorldMutation {
            dimension: self.dimension as u8,
            position: (x, y, z),
            block: block.to_wire(),
            state,
            revision,
        }))
    }

    /// Seed the optional world-creation chest in the authoritative world. The
    /// caller decides whether this is a new Overworld; renderer roots only
    /// project the returned revision-bearing mutation.
    pub fn place_bonus_chest(&mut self, position: (i32, i32, i32)) -> Option<WorldMutation> {
        if self.get_block(position.0, position.1, position.2) != BlockType::Air {
            return None;
        }
        let mutation = self
            .set_block(position.0, position.1, position.2, BlockType::Chest, 0)
            .ok()??;
        if let Some(BlockEntity::Chest(chest)) = self
            .chunks
            .get_block_entity_mut(position.0, position.1, position.2)
        {
            use crate::inventory::{Item, ItemStack};
            for (slot, item, count) in [
                (0, Item::OakLog, 4),
                (1, Item::OakPlanks, 8),
                (2, Item::Stick, 8),
                (3, Item::Bread, 4),
            ] {
                chest.set_stack(slot, Some(ItemStack::new(item, count)));
            }
        }
        Some(mutation)
    }

    pub fn validate_request(
        &self,
        request: &GameplayRequest,
        dimension: Dimension,
        position: [f32; 3],
        operator: bool,
    ) -> Result<(), RejectReason> {
        if dimension != self.dimension {
            return Err(RejectReason::InvalidDimension);
        }
        if !position.iter().all(|value| value.is_finite()) {
            return Err(RejectReason::InvalidState);
        }
        if let Some((x, y, z)) = operation_position(&request.operation) {
            if !self.valid_coordinate(x, y, z) {
                return Err(RejectReason::InvalidCoordinate);
            }
            let distance = Vec3::from_array(position)
                .distance_squared(Vec3::new(x as f32, y as f32, z as f32));
            if distance > 8.0 * 8.0 {
                return Err(RejectReason::TooFar);
            }
        }
        if matches!(&request.operation, GameplayOperation::Command { .. }) && !operator {
            return Err(RejectReason::PermissionDenied);
        }
        Ok(())
    }

    /// Apply one authenticated melee hit to a living world entity.  The
    /// session/player state is owned by AuthorityCore; this method only
    /// mutates the headless entity and therefore cannot create a renderer-side
    /// second authority.
    pub fn apply_combat(
        &mut self,
        target: u64,
        attacker_position: [f32; 3],
    ) -> Result<(), RejectReason> {
        let Some(entity) = self.entities.get_by_id_mut(target) else {
            return Err(RejectReason::InvalidState);
        };
        if !entity.entity_type.is_living() || entity.health <= 0.0 {
            return Err(RejectReason::InvalidState);
        }
        let distance = entity
            .position
            .distance_squared(Vec3::from_array(attacker_position));
        if !distance.is_finite() || distance > 8.0 * 8.0 {
            return Err(RejectReason::TooFar);
        }
        entity.health = (entity.health - 1.0).max(0.0);
        if entity.health <= 0.0 {
            entity.action_cooldown = 0.0;
        }
        Ok(())
    }

    /// Seed a session-facing villager into the headless world when the local
    /// presentation loaded a persisted entity before the in-process authority
    /// was created.  Existing IDs/types are never overwritten, preserving
    /// duplicate-identity and deterministic trade semantics.
    pub fn ensure_villager(
        &mut self,
        villager_id: u64,
        position: [f32; 3],
        profession: crate::village::poi::VillagerProfession,
        level: crate::village::trade::VillagerLevel,
        offers: Vec<crate::village::trade::TradeOffer>,
    ) -> bool {
        if let Some(entity) = self.entities.get_by_id(villager_id) {
            return entity.entity_type == EntityType::Villager;
        }
        let mut entity = crate::entity::Entity::new(
            villager_id,
            EntityType::Villager,
            Vec3::from_array(position),
        );
        entity.profession = profession;
        entity.villager_level = level;
        entity.offers = offers;
        self.entities.entities.push(entity);
        self.entities.rebuild_indexes();
        true
    }

    pub fn ensure_vehicle(
        &mut self,
        vehicle_id: u64,
        entity_type: EntityType,
        position: [f32; 3],
    ) -> bool {
        if !matches!(
            entity_type,
            EntityType::Boat | EntityType::Minecart | EntityType::Horse
        ) {
            return false;
        }
        if let Some(entity) = self.entities.get_by_id(vehicle_id) {
            return entity.entity_type == entity_type;
        }
        self.entities.entities.push(crate::entity::Entity::new(
            vehicle_id,
            entity_type,
            Vec3::from_array(position),
        ));
        self.entities.rebuild_indexes();
        true
    }

    pub fn ensure_entity(
        &mut self,
        entity_id: u64,
        entity_type: EntityType,
        position: [f32; 3],
        health: f32,
    ) -> bool {
        if let Some(entity) = self.entities.get_by_id(entity_id) {
            return entity.entity_type == entity_type;
        }
        let mut entity =
            crate::entity::Entity::new(entity_id, entity_type, Vec3::from_array(position));
        entity.health = health.clamp(0.0, entity.max_health);
        self.entities.entities.push(entity);
        self.entities.rebuild_indexes();
        true
    }

    /// Execute a villager offer atomically against a session's compact
    /// inventory.  Costs are checked before either item is removed and the
    /// complete state is restored if the sell stack cannot fit.
    pub fn apply_trade(
        &mut self,
        gameplay: &mut SessionGameplayState,
        villager_id: u64,
        offer_index: u16,
        player_position: [f32; 3],
    ) -> Result<(), RejectReason> {
        let Some(villager) = self.entities.get_by_id(villager_id) else {
            return Err(RejectReason::InvalidState);
        };
        if villager.entity_type != EntityType::Villager || villager.health <= 0.0 {
            return Err(RejectReason::InvalidState);
        }
        if villager
            .position
            .distance_squared(Vec3::from_array(player_position))
            > 8.0 * 8.0
        {
            return Err(RejectReason::TooFar);
        }
        let Some(offer) = villager.offers.get(usize::from(offer_index)).cloned() else {
            return Err(RejectReason::InvalidState);
        };
        if offer.is_out_of_stock() {
            return Err(RejectReason::InvalidState);
        }
        let cost_a = offer.effective_cost_a(0.0);
        let cost_b = offer.buy_b.map(|stack| stack.count).unwrap_or(0);
        let buy_a = offer.buy_a.item.to_u32();
        let buy_b = offer.buy_b.map(|stack| stack.item.to_u32());
        if gameplay.count_item(buy_a) < cost_a
            || buy_b.is_some_and(|item| gameplay.count_item(item) < cost_b)
        {
            return Err(RejectReason::InvalidState);
        }
        let original = *gameplay;
        let _ = gameplay.remove_item(buy_a, cost_a);
        if let Some(item) = buy_b {
            if !gameplay.remove_item(item, cost_b) {
                *gameplay = original;
                return Err(RejectReason::InvalidState);
            }
        }
        let sell = SessionInventorySlot::from_wire(
            crate::network::protocol::ItemWire::from_stack(&offer.sell),
            offer.sell.can_break,
            offer.sell.can_place_on,
        );
        if !gameplay.add_slot(sell) {
            *gameplay = original;
            return Err(RejectReason::InvalidState);
        }
        if (0..crate::authority::contract::SESSION_INVENTORY_SLOTS).any(|index| {
            transactions::brew_locks_slot(&original, index as u8)
                && original.inventory[index] != gameplay.inventory[index]
        }) {
            *gameplay = original;
            return Err(RejectReason::InvalidState);
        }
        let Some(villager) = self.entities.get_by_id_mut(villager_id) else {
            *gameplay = original;
            return Err(RejectReason::InvalidState);
        };
        let Some(offer) = villager.offers.get_mut(usize::from(offer_index)) else {
            *gameplay = original;
            return Err(RejectReason::InvalidState);
        };
        offer.uses = offer.uses.saturating_add(1);
        villager.villager_xp = villager.villager_xp.saturating_add(offer.xp_reward);
        Ok(())
    }

    /// Mount or dismount a player in the headless entity graph.  Entity
    /// passenger lists are authoritative; presentation roots only project the
    /// resulting `mounted_entity` session value.
    pub fn apply_mount(
        &mut self,
        player_id: PlayerId,
        entity_id: u64,
        player_position: [f32; 3],
    ) -> Result<Option<u64>, RejectReason> {
        if entity_id == 0 {
            for entity in &mut self.entities.entities {
                entity
                    .passengers
                    .retain(|passenger| *passenger != player_id);
            }
            return Ok(None);
        }
        let Some(vehicle) = self.entities.get_by_id(entity_id) else {
            return Err(RejectReason::InvalidState);
        };
        if !matches!(
            vehicle.entity_type,
            EntityType::Boat | EntityType::Minecart | EntityType::Horse
        ) {
            return Err(RejectReason::InvalidState);
        }
        if vehicle
            .position
            .distance_squared(Vec3::from_array(player_position))
            > 8.0 * 8.0
        {
            return Err(RejectReason::TooFar);
        }
        if vehicle.passengers.contains(&player_id) {
            return Ok(Some(entity_id));
        }
        let capacity = if vehicle.entity_type == EntityType::Boat {
            2
        } else {
            1
        };
        if vehicle.passengers.len() >= capacity {
            return Err(RejectReason::InvalidState);
        }
        for entity in &mut self.entities.entities {
            entity
                .passengers
                .retain(|passenger| *passenger != player_id);
        }
        let Some(vehicle) = self.entities.get_by_id_mut(entity_id) else {
            return Err(RejectReason::InvalidState);
        };
        vehicle.passengers.push(player_id);
        Ok(Some(entity_id))
    }

    /// Dispatch a request after session/sequence/revision validation.
    pub fn dispatch(
        &mut self,
        request: &GameplayRequest,
        player_id: PlayerId,
        operator: bool,
    ) -> Result<Option<WorldMutation>, WorldDispatchError> {
        match &request.operation {
            GameplayOperation::BlockUse { x, y, z, block } => {
                let block = BlockType::from_wire(*block)
                    .ok_or_else(|| WorldDispatchError::new(RejectReason::InvalidState))?;
                if self.get_block(*x, *y, *z) == block {
                    return Err(WorldDispatchError::new(RejectReason::InvalidState));
                }
                self.set_block(*x, *y, *z, block, 0)
            }
            GameplayOperation::Container {
                action,
                x,
                y,
                z,
                slot,
            } => {
                let action = ContainerAction::from_wire(*action)
                    .ok_or_else(|| WorldDispatchError::new(RejectReason::InvalidState))?;
                self.dispatch_container(action, *x, *y, *z, *slot, player_id, None)
            }
            GameplayOperation::ContainerClick {
                x,
                y,
                z,
                slot,
                is_left: _,
                dragged,
            } => self.dispatch_container(
                ContainerAction::Click,
                *x,
                *y,
                *z,
                *slot,
                player_id,
                dragged.as_ref(),
            ),
            GameplayOperation::Sleep { x, y, z } => {
                if self.get_block(*x, *y, *z) != BlockType::Bed {
                    return Err(WorldDispatchError::new(RejectReason::InvalidState));
                }
                if !self.sleeping_players.insert(player_id) {
                    return Err(WorldDispatchError::new(RejectReason::InvalidState));
                }
                Ok(Some(self.touch_revision(*x, *y, *z)))
            }
            GameplayOperation::Command { command } => {
                self.dispatch_command(command, operator)?;
                Ok(None)
            }
            GameplayOperation::ItemUse { .. }
            | GameplayOperation::Combat { .. }
            | GameplayOperation::Trade { .. }
            | GameplayOperation::Mount { .. }
            | GameplayOperation::Fishing { .. }
            | GameplayOperation::FurnaceTakeOutput { .. }
            | GameplayOperation::Craft { .. }
            | GameplayOperation::Enchant { .. }
            | GameplayOperation::Brew { .. }
            | GameplayOperation::Anvil { .. }
            | GameplayOperation::UseState { .. } => {
                Err(WorldDispatchError::new(RejectReason::Unsupported))
            }
        }
    }

    fn dispatch_container(
        &mut self,
        action: ContainerAction,
        x: i32,
        y: i32,
        z: i32,
        slot: u16,
        player_id: PlayerId,
        dragged: Option<&ItemWire>,
    ) -> Result<Option<WorldMutation>, WorldDispatchError> {
        let Some(entity) = self.chunks.get_block_entity(x, y, z) else {
            return Err(WorldDispatchError::new(RejectReason::InvalidState));
        };
        let Some(access) = ContainerAccess::for_entity(entity) else {
            return Err(WorldDispatchError::new(RejectReason::InvalidState));
        };
        if usize::from(slot) >= access.slot_count {
            return Err(WorldDispatchError::new(RejectReason::InvalidState));
        }
        let position = (x, y, z);
        match action {
            ContainerAction::Open => {
                self.container_viewers
                    .entry(position)
                    .or_default()
                    .insert(player_id);
            }
            ContainerAction::Close => {
                self.container_viewers
                    .entry(position)
                    .or_default()
                    .remove(&player_id);
                if self
                    .container_viewers
                    .get(&position)
                    .is_some_and(BTreeSet::is_empty)
                {
                    self.container_viewers.remove(&position);
                }
            }
            ContainerAction::Click => {
                let is_viewer = self
                    .container_viewers
                    .get(&position)
                    .is_some_and(|viewers| viewers.contains(&player_id));
                if !is_viewer {
                    return Err(WorldDispatchError::new(RejectReason::PermissionDenied));
                }
                if let Some(dragged) = dragged {
                    self.replace_container_slot(position, slot, dragged)?;
                } else {
                    self.extract_container_slot(position, slot)?;
                }
            }
        }
        Ok(Some(self.touch_revision(x, y, z)))
    }

    /// The compact gameplay envelope carries a slot but no cursor payload.
    /// A click therefore performs the deterministic server-side primitive of
    /// extracting one item; the resulting slot is returned through the
    /// `SendContainerClickResult` adapter. Rich cursor/drag payloads remain a
    /// protocol extension, never a renderer-side mutation.
    fn extract_container_slot(
        &mut self,
        position: (i32, i32, i32),
        slot: u16,
    ) -> Result<(), WorldDispatchError> {
        let slot = usize::from(slot);
        let Some(entity) = self
            .chunks
            .get_block_entity_mut(position.0, position.1, position.2)
        else {
            return Err(WorldDispatchError::new(RejectReason::InvalidState));
        };
        let stack = match entity {
            BlockEntity::Chest(chest) => &mut chest.inventory.slots[slot],
            BlockEntity::Furnace(furnace) => &mut furnace.slots[slot],
            BlockEntity::Hopper(hopper) => &mut hopper.slots[slot],
            BlockEntity::Dispenser(dispenser) => &mut dispenser.slots[slot],
            BlockEntity::Dropper(dropper) => &mut dropper.slots[slot],
            BlockEntity::Sign(_) | BlockEntity::Spawner(_) | BlockEntity::Observer(_) => {
                return Err(WorldDispatchError::new(RejectReason::InvalidState));
            }
        };
        let Some(existing) = stack.as_mut() else {
            return Err(WorldDispatchError::new(RejectReason::InvalidState));
        };
        existing.count = existing.count.saturating_sub(1);
        if existing.count == 0 {
            *stack = None;
        }
        match entity {
            BlockEntity::Chest(chest) => chest.revision = chest.revision.wrapping_add(1),
            BlockEntity::Furnace(furnace) => furnace.revision = furnace.revision.wrapping_add(1),
            BlockEntity::Hopper(hopper) => hopper.revision = hopper.revision.wrapping_add(1),
            BlockEntity::Dispenser(dispenser) => {
                dispenser.revision = dispenser.revision.wrapping_add(1)
            }
            BlockEntity::Dropper(dropper) => dropper.revision = dropper.revision.wrapping_add(1),
            BlockEntity::Sign(_) | BlockEntity::Spawner(_) | BlockEntity::Observer(_) => {}
        }
        Ok(())
    }

    fn replace_container_slot(
        &mut self,
        position: (i32, i32, i32),
        slot: u16,
        wire: &ItemWire,
    ) -> Result<(), WorldDispatchError> {
        let Some(value) = wire.to_stack() else {
            return Err(WorldDispatchError::new(RejectReason::InvalidState));
        };
        let slot = usize::from(slot);
        let Some(entity) = self
            .chunks
            .get_block_entity_mut(position.0, position.1, position.2)
        else {
            return Err(WorldDispatchError::new(RejectReason::InvalidState));
        };
        match entity {
            BlockEntity::Chest(chest) => chest.inventory.slots[slot] = Some(value),
            BlockEntity::Furnace(furnace) => furnace.slots[slot] = Some(value),
            BlockEntity::Hopper(hopper) => hopper.slots[slot] = Some(value),
            BlockEntity::Dispenser(dispenser) => dispenser.slots[slot] = Some(value),
            BlockEntity::Dropper(dropper) => dropper.slots[slot] = Some(value),
            BlockEntity::Sign(_) | BlockEntity::Spawner(_) | BlockEntity::Observer(_) => {
                return Err(WorldDispatchError::new(RejectReason::InvalidState));
            }
        }
        match entity {
            BlockEntity::Chest(chest) => chest.revision = chest.revision.wrapping_add(1),
            BlockEntity::Furnace(furnace) => furnace.revision = furnace.revision.wrapping_add(1),
            BlockEntity::Hopper(hopper) => hopper.revision = hopper.revision.wrapping_add(1),
            BlockEntity::Dispenser(dispenser) => {
                dispenser.revision = dispenser.revision.wrapping_add(1)
            }
            BlockEntity::Dropper(dropper) => dropper.revision = dropper.revision.wrapping_add(1),
            BlockEntity::Sign(_) | BlockEntity::Spawner(_) | BlockEntity::Observer(_) => {}
        }
        Ok(())
    }

    fn dispatch_command(&mut self, input: &str, _operator: bool) -> Result<(), WorldDispatchError> {
        let command = commands::parse(input)
            .map_err(|_| WorldDispatchError::new(RejectReason::InvalidState))?;
        match command {
            Command::GameRule { rule, value } => {
                let Some(value) = value else {
                    return Err(WorldDispatchError::new(RejectReason::InvalidState));
                };
                if let Ok(bool_value) = value.parse::<bool>() {
                    self.rules
                        .set(&rule, bool_value)
                        .map_err(|_| WorldDispatchError::new(RejectReason::InvalidState))?;
                } else if matches!(
                    rule.as_str(),
                    "playerssleepingpercentage" | "sleepingpercentage" | "sleeping_percentage"
                ) {
                    let percentage = value
                        .parse::<u8>()
                        .map_err(|_| WorldDispatchError::new(RejectReason::InvalidState))?;
                    self.rules.set_sleeping_percentage(percentage);
                } else {
                    return Err(WorldDispatchError::new(RejectReason::InvalidState));
                }
            }
            Command::Time(TimeCommand::Set(time)) => self.time = time,
            Command::Time(TimeCommand::Add(time)) => self.time = self.time.wrapping_add(time),
            // Session commands are handled by AuthorityCore because they need
            // access to authenticated session state.
            Command::GameMode { .. } | Command::Teleport { .. } => {
                return Err(WorldDispatchError::new(RejectReason::Unsupported));
            }
            Command::Help(_)
            | Command::Difficulty(_)
            | Command::Weather(_)
            | Command::Give { .. }
            | Command::Kill(_)
            | Command::SpawnPoint { .. }
            | Command::SetWorldSpawn(_)
            | Command::Locate(_)
            | Command::Seed
            | Command::SaveAll => {
                return Err(WorldDispatchError::new(RejectReason::Unsupported));
            }
        }
        Ok(())
    }

    fn touch_revision(&mut self, x: i32, y: i32, z: i32) -> WorldMutation {
        let revision = self.revisions.allocate();
        self.block_revisions.insert((x, y, z), revision);
        self.chunk_revisions
            .insert((x.div_euclid(16), z.div_euclid(16)), revision);
        WorldMutation {
            dimension: self.dimension as u8,
            position: (x, y, z),
            block: self.get_block(x, y, z).to_wire(),
            state: self.get_block_state(x, y, z),
            revision,
        }
    }

    /// Advance exactly one 20 Hz tick.  All iteration order is normalized so
    /// the checksum and mutation revisions are topology-independent.
    pub fn tick(&mut self, players: &[(PlayerId, [f32; 3])]) -> AuthoritySnapshot {
        if self.rules.do_daylight_cycle {
            self.time = self.time.wrapping_add(1);
        }
        let mut mutations = Vec::new();

        let mut occupants: Vec<_> = players
            .iter()
            .filter_map(|(_, position)| {
                position.iter().all(|value| value.is_finite()).then_some((
                    position[0].floor() as i32,
                    position[1].floor() as i32,
                    position[2].floor() as i32,
                ))
            })
            .collect();
        occupants.sort_unstable();
        occupants.dedup();
        let redstone = self.redstone.tick(&mut self.chunks, &occupants);
        let mut redstone_mutations = redstone.mutations;
        redstone_mutations.sort_by_key(|mutation| mutation.pos);
        for mutation in redstone_mutations {
            if let Ok(Some(event)) = self.set_block(
                mutation.pos.0,
                mutation.pos.1,
                mutation.pos.2,
                mutation.new_block,
                0,
            ) {
                mutations.push(event);
            }
        }

        // These systems mutate actual block entities/chunks, not a shadow map.
        let _ = crate::world_tick::tick_hoppers_with_entities(
            &mut self.chunks,
            Some(&mut self.entities),
            MAX_AUTOMATION_TRANSFERS,
        );
        for is_lava in [false, true] {
            let (_, fluid_mutations) =
                crate::fluid::tick_fluids(&mut self.chunks, is_lava, MAX_FLUID_UPDATES);
            for (position, block) in fluid_mutations {
                if let Ok(Some(event)) =
                    self.set_block(position.0, position.1, position.2, block, 0)
                {
                    mutations.push(event);
                }
            }
        }

        // Random ticks (crop growth, fire and leaf decay) run in the same
        // deterministic headless world as redstone/fluid automation.  The
        // renderer never performs a second random-tick pass for a boundary.
        let (mut random_ticks, _) = crate::world_tick::sample_random_ticks(
            &self.chunks,
            self.seed as u64,
            self.time,
            self.dimension as u8,
            128,
        );
        if !self.rules.do_fire_tick {
            random_ticks.retain(|mutation| {
                self.get_block(mutation.pos.0, mutation.pos.1, mutation.pos.2) != BlockType::Fire
            });
        }
        random_ticks.sort_by_key(|mutation| mutation.pos);
        for mutation in random_ticks {
            if let Ok(Some(event)) = self.set_block(
                mutation.pos.0,
                mutation.pos.1,
                mutation.pos.2,
                mutation.new_block,
                mutation.new_state,
            ) {
                mutations.push(event);
            }
        }

        mutations.extend(self.tick_furnaces());

        self.tick_entities(players);
        mutations.sort_by_key(|mutation| mutation.revision);
        let checksum = self.checksum(&mutations);
        let snapshot = AuthoritySnapshot {
            tick: self.time,
            revision: self.revisions.current(),
            checksum,
            mutations,
            session_updates: Vec::new(),
        };
        self.last_snapshot = snapshot.clone();
        snapshot
    }

    fn tick_furnaces(&mut self) -> Vec<WorldMutation> {
        let mut positions = Vec::new();
        for (&(cx, cz), chunk) in &self.chunks.chunks {
            for (local, entity) in chunk.iter_block_entities() {
                if matches!(entity, BlockEntity::Furnace(_)) {
                    positions.push((
                        cx * 16 + local.0 as i32,
                        local.1 as i32,
                        cz * 16 + local.2 as i32,
                    ));
                }
            }
        }
        positions.sort_unstable();
        let mut mutations = Vec::new();
        for (x, y, z) in positions {
            let Some(BlockEntity::Furnace(furnace)) = self.chunks.get_block_entity_mut(x, y, z)
            else {
                continue;
            };
            let was_lit = furnace.is_lit;
            let result = furnace.tick(&self.recipe_manager);
            if !result.slot_changed && !result.lit_changed {
                continue;
            }
            furnace.revision = furnace.revision.wrapping_add(1);
            let is_lit = furnace.is_lit;
            let _ = furnace;
            if was_lit != is_lit {
                let block = if is_lit {
                    BlockType::FurnaceLit
                } else {
                    BlockType::Furnace
                };
                if let Ok(Some(event)) = self.set_block(x, y, z, block, 0) {
                    mutations.push(event);
                }
            } else {
                mutations.push(self.touch_revision(x, y, z));
            }
        }
        mutations
    }

    fn tick_entities(&mut self, players: &[(PlayerId, [f32; 3])]) {
        // Peaceful is an authority policy, not merely a spawn-rate hint:
        // already-loaded hostile entities are removed at the next fixed tick.
        // `do_mob_spawning=false` deliberately does not take this path, so it
        // cannot freeze an existing hostile entity's AI.
        if matches!(self.difficulty, ServerDifficulty::Peaceful) {
            let before = self.entities.entities.len();
            self.entities
                .entities
                .retain(|entity| !entity.entity_type.is_hostile());
            if self.entities.entities.len() != before {
                self.entities.rebuild_indexes();
            }
        }
        let mut player_positions: Vec<_> = players.to_vec();
        player_positions.sort_by_key(|(id, _)| *id);
        let chunks = &self.chunks;
        for entity in &mut self.entities.entities {
            if entity.entity_type == EntityType::FishingHook {
                continue;
            }
            entity.action_cooldown = (entity.action_cooldown - FIXED_DT).max(0.0);
            entity.invulnerable_time = (entity.invulnerable_time - FIXED_DT).max(0.0);
            entity.fire_aspect_timer = (entity.fire_aspect_timer - FIXED_DT).max(0.0);
            if entity.entity_type.is_hostile()
                && !matches!(self.difficulty, ServerDifficulty::Peaceful)
            {
                if let Some((_, target)) =
                    player_positions.iter().min_by(|(_, left), (_, right)| {
                        entity
                            .position
                            .distance_squared(Vec3::from_array(*left))
                            .total_cmp(&entity.position.distance_squared(Vec3::from_array(*right)))
                    })
                {
                    let direction =
                        (Vec3::from_array(*target) - entity.position).normalize_or_zero();
                    let speed = self.difficulty.hostile_chase_speed_milli() as f32 / 1_000.0;
                    entity.velocity.x = direction.x * 1.2 * speed;
                    entity.velocity.z = direction.z * 1.2 * speed;
                    entity.target_player = true;
                }
            }
            entity.ai_phase = entity.ai_phase.wrapping_add(1);
            entity.ai_timer += FIXED_DT;
            entity.update_physics(FIXED_DT, chunks);
        }
        self.entities.sync_positions();
    }

    pub(crate) fn checksum(&self, mutations: &[WorldMutation]) -> u64 {
        // Stable FNV-1a over authoritative values.  HashMap iteration is never
        // used directly; chunks and block revisions are sorted first.
        let mut hash = 0xcbf29ce484222325u64;
        let mut write = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
        };
        write(&self.time.to_le_bytes());
        write(&self.revisions.current().to_le_bytes());
        write(&[
            self.rules.keep_inventory as u8,
            self.rules.mob_griefing as u8,
        ]);
        write(&[
            self.rules.do_daylight_cycle as u8,
            self.rules.do_mob_spawning as u8,
            self.difficulty.as_u8(),
        ]);
        for mutation in mutations {
            write(&mutation.dimension.to_le_bytes());
            write(&mutation.position.0.to_le_bytes());
            write(&mutation.position.1.to_le_bytes());
            write(&mutation.position.2.to_le_bytes());
            write(&mutation.block.to_le_bytes());
            write(&mutation.state.to_le_bytes());
            write(&mutation.revision.to_le_bytes());
        }
        for (&position, &revision) in &self.block_revisions {
            write(&position.0.to_le_bytes());
            write(&position.1.to_le_bytes());
            write(&position.2.to_le_bytes());
            write(&revision.to_le_bytes());
        }
        let mut entities: Vec<_> = self
            .entities
            .entities
            .iter()
            .map(|entity| {
                (
                    entity.id,
                    entity.position.x.to_bits(),
                    entity.position.y.to_bits(),
                    entity.position.z.to_bits(),
                    entity.ai_phase,
                )
            })
            .collect();
        entities.sort_unstable();
        for entity in entities {
            write(&entity.0.to_le_bytes());
            write(&entity.1.to_le_bytes());
            write(&entity.2.to_le_bytes());
            write(&entity.3.to_le_bytes());
            write(&entity.4.to_le_bytes());
        }
        hash
    }
}

fn operation_position(operation: &GameplayOperation) -> Option<(i32, i32, i32)> {
    match operation {
        GameplayOperation::BlockUse { x, y, z, .. }
        | GameplayOperation::Sleep { x, y, z }
        | GameplayOperation::Container { x, y, z, .. }
        | GameplayOperation::ContainerClick { x, y, z, .. }
        | GameplayOperation::FurnaceTakeOutput { x, y, z, .. }
        | GameplayOperation::Enchant { x, y, z, .. }
        | GameplayOperation::Brew { x, y, z, .. }
        | GameplayOperation::Anvil { x, y, z, .. } => Some((*x, *y, *z)),
        GameplayOperation::Craft {
            station: Some([x, y, z]),
            ..
        } => Some((*x, *y, *z)),
        _ => None,
    }
}

fn position_to_milli(position: [f32; 3]) -> Option<[i32; 3]> {
    let mut result = [0; 3];
    for (index, value) in position.into_iter().enumerate() {
        if !value.is_finite() || value.abs() > 2_000_000.0 {
            return None;
        }
        result[index] = (value * 1_000.0).round() as i32;
    }
    Some(result)
}

fn milli_to_vec3(position: [i32; 3]) -> Vec3 {
    Vec3::new(
        position[0] as f32 / 1_000.0,
        position[1] as f32 / 1_000.0,
        position[2] as f32 / 1_000.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::contract::AuthorityTopology;
    use crate::authority::{AuthorityConfig, AuthorityCore};
    use crate::entity::EntityType;
    use crate::network::protocol::{GameplayOutcome, GameplayRequest};

    #[test]
    fn block_mutation_changes_real_chunk_and_revision() {
        let mut world = ServerWorld::new(
            7,
            Dimension::Overworld,
            WorldType::Superflat,
            false,
            WorldRules::default(),
            2,
        );
        let old = world.get_block(8, 80, 8);
        let mutation = world
            .set_block(8, 80, 8, BlockType::Chest, 0)
            .unwrap()
            .unwrap();
        assert_ne!(old, BlockType::Chest);
        assert_eq!(world.get_block(8, 80, 8), BlockType::Chest);
        assert_eq!(world.get_block_entity(8, 80, 8).is_some(), true);
        assert_eq!(mutation.revision, 1);
    }

    #[test]
    fn fixed_tick_checksum_is_deterministic() {
        let make = || {
            let mut world = ServerWorld::new(
                7,
                Dimension::Overworld,
                WorldType::Superflat,
                false,
                WorldRules::default(),
                2,
            );
            world
                .entities
                .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
            world.tick(&[(7, [8.0, 80.0, 8.0])])
        };
        assert_eq!(make(), make());
    }

    #[test]
    fn difficulty_controls_hostile_policy_without_binding_pvp_or_spawn_rule() {
        let mut rules = WorldRules::default();
        rules.pvp = true;
        rules.do_mob_spawning = false;
        let mut peaceful = ServerWorld::new_with_difficulty(
            7,
            Dimension::Overworld,
            WorldType::Superflat,
            false,
            rules,
            2,
            ServerDifficulty::Peaceful,
        );
        assert!(!peaceful.allows_hostile_spawning());
        assert!(peaceful.rules.pvp);
        let peaceful_id = peaceful
            .entities
            .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
        peaceful.tick(&[(7, [8.0, 80.0, 8.0])]);
        assert!(peaceful.entities.get_by_id(peaceful_id).is_none());

        let mut easy_rules = rules;
        easy_rules.do_mob_spawning = false;
        let mut easy = ServerWorld::new_with_difficulty(
            7,
            Dimension::Overworld,
            WorldType::Superflat,
            false,
            easy_rules,
            2,
            ServerDifficulty::Easy,
        );
        assert!(!easy.allows_hostile_spawning());
        let easy_id = easy
            .entities
            .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
        easy.tick(&[(7, [8.0, 80.0, 8.0])]);
        assert!(easy.entities.get_by_id(easy_id).is_some());
        assert!(easy
            .entities
            .get_by_id(easy_id)
            .is_some_and(|entity| entity.velocity.x < 0.0));

        let mut normal = ServerWorld::new_with_difficulty(
            7,
            Dimension::Overworld,
            WorldType::Superflat,
            false,
            WorldRules::default(),
            2,
            ServerDifficulty::Normal,
        );
        let normal_id = normal
            .entities
            .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
        normal.tick(&[(7, [8.0, 80.0, 8.0])]);

        let mut hard = ServerWorld::new_with_difficulty(
            7,
            Dimension::Overworld,
            WorldType::Superflat,
            false,
            WorldRules::default(),
            2,
            ServerDifficulty::Hard,
        );
        let hard_id = hard
            .entities
            .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
        hard.tick(&[(7, [8.0, 80.0, 8.0])]);
        let easy_speed = easy.entities.get_by_id(easy_id).unwrap().velocity.x.abs();
        let normal_speed = normal
            .entities
            .get_by_id(normal_id)
            .unwrap()
            .velocity
            .x
            .abs();
        let hard_speed = hard.entities.get_by_id(hard_id).unwrap().velocity.x.abs();
        assert!(easy_speed < normal_speed && normal_speed < hard_speed);
    }

    #[test]
    fn malformed_combat_action_is_explicitly_rejected() {
        let mut core = AuthorityCore::new(AuthorityConfig::default(), AuthorityTopology::Dedicated);
        core.register_session(crate::authority::contract::SessionContract::new(
            7,
            "alex",
            0,
            [8.0, 80.0, 8.0],
            true,
            true,
        ))
        .unwrap();
        let request = GameplayRequest {
            request_id: 1,
            client_sequence: 1,
            session_id: 7,
            dimension: 0,
            client_revision: 0,
            operation: GameplayOperation::Combat {
                target: 42,
                action: 1,
            },
        };
        assert!(matches!(
            core.submit_request(request).outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::InvalidState
            }
        ));
    }

    #[test]
    fn mount_requires_range_and_updates_authoritative_passengers() {
        let mut world = ServerWorld::new(
            7,
            Dimension::Overworld,
            WorldType::Superflat,
            false,
            WorldRules::default(),
            2,
        );
        assert!(world.ensure_vehicle(11, EntityType::Boat, [9.0, 80.0, 8.0]));
        assert_eq!(world.apply_mount(7, 11, [8.0, 80.0, 8.0]), Ok(Some(11)));
        assert!(world
            .entities
            .get_by_id(11)
            .unwrap()
            .passengers
            .contains(&7));
        assert!(world.ensure_vehicle(12, EntityType::Boat, [100.0, 80.0, 8.0]));
        assert_eq!(
            world.apply_mount(7, 12, [8.0, 80.0, 8.0]),
            Err(RejectReason::TooFar)
        );
        assert!(world
            .entities
            .get_by_id(11)
            .unwrap()
            .passengers
            .contains(&7));
    }

    #[test]
    fn trade_second_cost_failure_rolls_back_first_cost() {
        let mut world = ServerWorld::new(
            7,
            Dimension::Overworld,
            WorldType::Superflat,
            false,
            WorldRules::default(),
            2,
        );
        assert!(world.ensure_villager(
            21,
            [9.0, 80.0, 8.0],
            crate::village::poi::VillagerProfession::Farmer,
            crate::village::trade::VillagerLevel::Novice,
            vec![crate::village::trade::TradeOffer::new(
                crate::inventory::ItemStack::new(crate::inventory::Item::Wheat, 2),
                Some(crate::inventory::ItemStack::new(
                    crate::inventory::Item::Carrot,
                    1
                )),
                crate::inventory::ItemStack::new(crate::inventory::Item::Emerald, 1),
                4,
                1,
            )],
        ));
        let mut gameplay = SessionGameplayState::default();
        let mut wheat = crate::network::protocol::ItemWire::empty();
        wheat.item = crate::inventory::Item::Wheat as u32;
        wheat.count = 2;
        gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wheat, 0, 0));
        let before = gameplay;
        assert_eq!(
            world.apply_trade(&mut gameplay, 21, 0, [8.0, 80.0, 8.0]),
            Err(RejectReason::InvalidState)
        );
        assert_eq!(gameplay, before);
        assert_eq!(world.entities.get_by_id(21).unwrap().offers[0].uses, 0);
    }
}
