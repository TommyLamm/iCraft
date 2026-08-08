//! Deterministic CPU-only gameplay harness used by the Plan 19 acceptance
//! scenarios.  The harness deliberately exposes small, typed operations which
//! call the same recipe, inventory, block-entity, random-tick, structure,
//! dimension, entity and save seams used by the runtime.  Its fixture only
//! establishes deterministic starting terrain; progression results are produced
//! by those operations rather than by pre-seeding target blocks/entities.

use crate::block_entity::{default_stub_for_block, BlockEntity};
use crate::chunk_manager::ChunkManager;
use crate::dimension::{transform_position, Dimension};
use crate::entity::{EntityManager, EntityType};
use crate::inventory::{GameMode, Inventory, Item, ItemStack, ToolType};
use crate::physics::{PlayerPhysics, PLAYER_PHYSICS_TICK_DT};
use crate::recipes::RecipeManager;
use crate::redstone::RedstoneSystem;
use crate::save::{ChunkSaveData, EntitySaveData, SaveManager};
use crate::vehicle::MountManager;
use crate::village::poi::VillagerProfession;
use crate::village::trade::{generate_offers_for_level, MerchantSessionManager, VillagerLevel};
use crate::world::{BlockType, Chunk};
use glam::Vec3;
use std::collections::HashMap;
use std::path::Path;

pub const TICK_HZ: f64 = 20.0;
const TICK_DT: f32 = PLAYER_PHYSICS_TICK_DT;
const SEED: u32 = 0x1C4F_0007;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldTime {
    pub ticks: u64,
}

struct DimensionState {
    chunks: ChunkManager,
    entities: EntityManager,
}

/// Result of one authoritative mining operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiningResult {
    pub block: BlockType,
    pub drop: Option<Item>,
    pub tool_durability_before: Option<u32>,
    pub tool_durability_after: Option<u32>,
}

/// Result of a trade, including the offer's post-transaction use count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TradeResult {
    pub offer_index: usize,
    pub uses: u32,
    pub cooldown_ticks: u32,
}

/// A small deterministic simulation world.  Public fields intentionally expose
/// the same CPU state used by the old fixed-step tests; gameplay mutations go
/// through methods below so acceptance tests can describe real user actions.
pub struct SimHarness {
    pub chunks: ChunkManager,
    pub lighting_dirty: std::collections::HashSet<(i32, i32)>,
    pub redstone: RedstoneSystem,
    pub entities: EntityManager,
    pub player: PlayerPhysics,
    pub player_health: f32,
    pub player_hunger: f32,
    pub player_saturation: f32,
    pub inventory: Inventory,
    pub world_time: WorldTime,
    pub tick_count: u64,
    pub dimension: Dimension,
    pub spawn_point: Option<(i32, i32, i32)>,
    pub spawn_dimension: Option<Dimension>,
    pub dragon_defeated: bool,
    pub last_dimension_transition: Option<(Dimension, Dimension)>,
    pub merchant_sessions: MerchantSessionManager,
    pub trade_cooldowns: HashMap<u64, u32>,
    pub mount_manager: MountManager,
    pub minecart_states: HashMap<u64, crate::rail::MinecartState>,
    pub minecart_cargo: HashMap<u64, ItemStack>,
    recipes: RecipeManager,
    dimensions: HashMap<Dimension, DimensionState>,
    next_trade_session: u64,
}

impl SimHarness {
    pub fn new() -> Self {
        let dimension = Dimension::Overworld;
        let mut h = Self {
            chunks: Self::new_chunks(dimension),
            lighting_dirty: std::collections::HashSet::new(),
            redstone: RedstoneSystem::new(),
            entities: EntityManager::new(),
            player: PlayerPhysics::new(Vec3::new(8.5, 72.0, 8.5)),
            player_health: 20.0,
            player_hunger: 20.0,
            player_saturation: 5.0,
            inventory: Inventory::new(),
            world_time: WorldTime::default(),
            tick_count: 0,
            dimension,
            spawn_point: None,
            spawn_dimension: None,
            dragon_defeated: false,
            last_dimension_transition: None,
            merchant_sessions: MerchantSessionManager::new(),
            trade_cooldowns: HashMap::new(),
            mount_manager: MountManager::new(),
            minecart_states: HashMap::new(),
            minecart_cargo: HashMap::new(),
            recipes: RecipeManager::new(),
            dimensions: HashMap::new(),
            next_trade_session: 1,
        };
        h.setup_fixture_terrain();
        h
    }

    fn new_chunks(dimension: Dimension) -> ChunkManager {
        let mut chunks = ChunkManager::new_in_dimension(2, dimension);
        for cx in -1..=1 {
            for cz in -1..=1 {
                chunks
                    .chunks
                    .insert((cx, cz), Chunk::new_with_seed(cx, cz, SEED));
            }
        }
        chunks
    }

    /// Build only deterministic starting terrain and resource veins.  No
    /// inventory, container, portal, villager, vehicle or boss target is
    /// pre-created here.
    fn setup_fixture_terrain(&mut self) {
        for x in 0..16 {
            for z in 0..16 {
                self.chunks.set_block(x, 69, z, BlockType::Dirt);
                self.chunks.set_block(x, 70, z, BlockType::Air);
                self.chunks.set_block(x, 71, z, BlockType::Air);
            }
        }

        // A small deterministic tree and mineable vein.
        for x in 2..=5 {
            self.chunks.set_block(x, 70, 3, BlockType::OakLog);
        }
        self.chunks.set_block(2, 71, 3, BlockType::OakLog);
        for (x, block) in [
            (4, BlockType::Stone),
            (5, BlockType::Stone),
            (6, BlockType::Stone),
            (7, BlockType::CoalOre),
            (8, BlockType::Gravel),
        ] {
            self.chunks.set_block(x, 70, 2, block);
        }
        for x in 9..=11 {
            self.chunks.set_block(x, 70, 2, BlockType::IronOre);
        }
        for x in 0..=3 {
            self.chunks.set_block(x, 70, 1, BlockType::IronOre);
        }
        for x in 12..=15 {
            self.chunks.set_block(x, 70, 2, BlockType::DiamondOre);
        }
        // Additional diamonds let the End portal workflow obtain its twelve
        // eyes without injecting finished eyes into the player's inventory.
        for x in 0..=11 {
            self.chunks.set_block(x, 68, 4, BlockType::DiamondOre);
        }
        for x in 0..4 {
            self.chunks.set_block(x, 67, 11, BlockType::DiamondOre);
        }
        for x in 0..8 {
            self.chunks.set_block(x, 67, 10, BlockType::Stone);
        }
        for x in 0..14 {
            self.chunks.set_block(x, 68, 6, BlockType::Obsidian);
        }
        self.chunks.set_block(13, 70, 2, BlockType::TallGrass);
        self.chunks.set_block(14, 70, 2, BlockType::Water);
        self.chunks.set_fluid_level(14, 70, 2, 0);
        // A shelter bed is part of the deterministic starting terrain.  Bed
        // sleeping and spawn-point validation are still performed by actions.
        self.chunks.set_block(14, 69, 4, BlockType::Bed);
    }

    fn ensure_loaded_area(&mut self, x: i32, z: i32, radius: i32) {
        let cx = x.div_euclid(16);
        let cz = z.div_euclid(16);
        for nx in (cx - radius)..=(cx + radius) {
            for nz in (cz - radius)..=(cz + radius) {
                self.chunks
                    .chunks
                    .entry((nx, nz))
                    .or_insert_with(|| Chunk::new_with_seed(nx, nz, SEED));
            }
        }
    }

    fn set_block_with_entity(&mut self, pos: (i32, i32, i32), block: BlockType) {
        self.ensure_loaded_area(pos.0, pos.2, 1);
        self.chunks.set_block(pos.0, pos.1, pos.2, block);
        self.chunks
            .set_block_entity(pos.0, pos.1, pos.2, default_stub_for_block(block));
    }

    fn remove_item_count(&mut self, item: Item, count: u32) -> bool {
        if self.inventory.count_item(item) < count {
            return false;
        }
        for _ in 0..count {
            debug_assert!(self.inventory.remove_one(item));
        }
        true
    }

    fn add_stack(&mut self, stack: ItemStack) -> bool {
        self.inventory.add_stack(stack).is_none()
    }

    fn mutate_tool_durability(&mut self, slot: usize) -> Option<(u32, u32)> {
        let stack = if slot < 9 {
            self.inventory.hotbar[slot].as_mut()
        } else if slot < 36 {
            self.inventory.main[slot - 9].as_mut()
        } else {
            None
        }?;
        let before = stack.durability;
        if before == 0 {
            return Some((before, before));
        }
        stack.durability = before.saturating_sub(1);
        let after = stack.durability;
        if after == 0 {
            if slot < 9 {
                self.inventory.hotbar[slot] = None;
            } else {
                self.inventory.main[slot - 9] = None;
            }
        }
        Some((before, after))
    }

    fn item_drop(block: BlockType) -> Option<Item> {
        Some(match block {
            BlockType::Air | BlockType::Bedrock => return None,
            BlockType::OakLog => Item::OakLog,
            BlockType::Stone => Item::Cobblestone,
            BlockType::CoalOre => Item::Coal,
            BlockType::IronOre => Item::IronOre,
            BlockType::DiamondOre => Item::Diamond,
            BlockType::Gravel => Item::Gravel,
            BlockType::TallGrass => Item::Seeds,
            BlockType::WheatCrop => Item::Wheat,
            BlockType::CarrotCrop => Item::Carrot,
            BlockType::PotatoCrop => Item::Potato,
            BlockType::Obsidian => Item::Obsidian,
            BlockType::EndPortalFrame => Item::EndPortalFrame,
            BlockType::EndPortalFrameFilled => Item::EndPortalFrame,
            BlockType::NetherBrick => Item::NetherBrick,
            BlockType::EndStone => Item::EndStone,
            _ => return None,
        })
    }

    /// Mine one loaded block using the same preferred-tool and minimum-tier
    /// rules as the desktop interaction path.  The block is removed only after
    /// validation; its resulting drop enters the player inventory.
    pub fn mine_block(&mut self, pos: (i32, i32, i32), tool: Option<Item>) -> Option<MiningResult> {
        let block = self.chunks.get_block(pos.0, pos.1, pos.2);
        if block == BlockType::Air {
            return None;
        }
        let selected_slot = tool.and_then(|item| self.inventory.find_item(item).map(|(s, _)| s));
        let selected = selected_slot.and_then(|slot| {
            if slot < 9 {
                self.inventory.hotbar[slot]
            } else {
                self.inventory.main.get(slot - 9).copied().flatten()
            }
        });
        let tool_props = selected.and_then(|stack| stack.item.tool_properties());
        if block.preferred_tool() != ToolType::None
            && tool_props.is_some_and(|p| p.tool_type != block.preferred_tool())
        {
            return None;
        }
        if let Some(min_material) = block.min_harvest_material() {
            if !tool_props.is_some_and(|p| p.material >= min_material) {
                return None;
            }
        }
        let drop = Self::item_drop(block);
        self.chunks.set_block(pos.0, pos.1, pos.2, BlockType::Air);
        self.chunks.set_block_entity(pos.0, pos.1, pos.2, None);
        if let Some(item) = drop {
            let _ = self.add_stack(ItemStack::new(item, 1));
        }
        let (before, after) = selected_slot
            .and_then(|slot| self.mutate_tool_durability(slot))
            .map_or((None, None), |(before, after)| (Some(before), Some(after)));
        Some(MiningResult {
            block,
            drop,
            tool_durability_before: before,
            tool_durability_after: after,
        })
    }

    /// Craft through RecipeManager and consume the exact ingredient counts.
    pub fn craft(&mut self, grid: &[Option<ItemStack>], grid_size: usize) -> Option<ItemStack> {
        let result = self.recipes.match_crafting_recipe(grid, grid_size)?;
        let mut ingredients = HashMap::<Item, u32>::new();
        for stack in grid.iter().flatten() {
            *ingredients.entry(stack.item).or_default() += 1;
        }
        if ingredients
            .iter()
            .any(|(&item, &count)| self.inventory.count_item(item) < count)
        {
            return None;
        }
        for (&item, &count) in &ingredients {
            if !self.remove_item_count(item, count) {
                return None;
            }
        }
        self.add_stack(result);
        Some(result)
    }

    pub fn craft_shapeless(&mut self, items: &[Item]) -> Option<ItemStack> {
        // RecipeManager scans a square crafting grid even for shapeless
        // recipes, so keep the backing vector grid_size^2 rather than passing
        // a short list that would index past its end.
        let grid_size = 3;
        let mut grid = vec![None; grid_size * grid_size];
        for (slot, item) in items.iter().copied().enumerate() {
            if slot >= grid.len() {
                return None;
            }
            grid[slot] = Some(ItemStack::new(item, 1));
        }
        self.craft(&grid, grid_size)
    }

    /// Place one inventory block and instantiate its block entity through the
    /// shared default-stub seam.
    pub fn place_item(&mut self, pos: (i32, i32, i32), item: Item) -> bool {
        let Some(block) = item.properties().block_type else {
            return false;
        };
        if self.chunks.get_block(pos.0, pos.1, pos.2) != BlockType::Air
            || !self.remove_item_count(item, 1)
        {
            return false;
        }
        self.set_block_with_entity(pos, block);
        true
    }

    pub fn container_put(&mut self, pos: (i32, i32, i32), stack: ItemStack) -> bool {
        if self.inventory.count_item(stack.item) < stack.count {
            return false;
        }
        let Some(mut entity) = self.chunks.get_block_entity(pos.0, pos.1, pos.2).cloned() else {
            return false;
        };
        for _ in 0..stack.count {
            if !entity.try_insert_item(None, ItemStack { count: 1, ..stack }) {
                return false;
            }
        }
        if !self.remove_item_count(stack.item, stack.count) {
            return false;
        }
        self.chunks
            .set_block_entity(pos.0, pos.1, pos.2, Some(entity));
        true
    }

    /// Place a stack in a specific player-accessible container slot.  This is
    /// used for sided automation tests (for example, a furnace fuel slot) and
    /// still consumes the item from the simulated player's inventory.
    pub fn container_put_slot(
        &mut self,
        pos: (i32, i32, i32),
        slot: usize,
        stack: ItemStack,
    ) -> bool {
        if stack.count == 0 || self.inventory.count_item(stack.item) < stack.count {
            return false;
        }
        let Some(mut entity) = self.chunks.get_block_entity(pos.0, pos.1, pos.2).cloned() else {
            return false;
        };
        if slot >= entity.slot_count() || entity.get_stack(slot).is_some() {
            return false;
        }
        entity.set_stack(slot, Some(stack));
        if !self.remove_item_count(stack.item, stack.count) {
            return false;
        }
        self.chunks
            .set_block_entity(pos.0, pos.1, pos.2, Some(entity));
        true
    }

    pub fn container_take(&mut self, pos: (i32, i32, i32), count: u32) -> Option<ItemStack> {
        let mut entity = self.chunks.get_block_entity(pos.0, pos.1, pos.2).cloned()?;
        let mut result: Option<ItemStack> = None;
        for _ in 0..count {
            let stack = entity.try_extract_item(None)?;
            if let Some(existing) = result.as_mut() {
                if existing.item != stack.item || !existing.can_merge_with(&stack) {
                    return None;
                }
                existing.count += stack.count;
            } else {
                result = Some(stack);
            }
        }
        self.chunks
            .set_block_entity(pos.0, pos.1, pos.2, Some(entity));
        let stack = result?;
        self.add_stack(stack);
        Some(stack)
    }

    pub fn furnace_tick(
        &mut self,
        pos: (i32, i32, i32),
    ) -> Option<crate::block_entity::FurnaceTickResult> {
        let mut furnace = match self.chunks.get_block_entity(pos.0, pos.1, pos.2)?.clone() {
            BlockEntity::Furnace(furnace) => furnace,
            _ => return None,
        };
        let result = furnace.tick(&self.recipes);
        let lit = furnace.is_lit;
        self.chunks.set_block(
            pos.0,
            pos.1,
            pos.2,
            if lit {
                BlockType::FurnaceLit
            } else {
                BlockType::Furnace
            },
        );
        self.chunks
            .set_block_entity(pos.0, pos.1, pos.2, Some(BlockEntity::Furnace(furnace)));
        Some(result)
    }

    pub fn furnace_claim_xp(&mut self, pos: (i32, i32, i32)) -> Option<f32> {
        let mut furnace = match self.chunks.get_block_entity(pos.0, pos.1, pos.2)?.clone() {
            BlockEntity::Furnace(furnace) => furnace,
            _ => return None,
        };
        let xp = furnace.claim_xp();
        self.chunks
            .set_block_entity(pos.0, pos.1, pos.2, Some(BlockEntity::Furnace(furnace)));
        Some(xp)
    }

    pub fn till(&mut self, pos: (i32, i32, i32), hoe: Item) -> bool {
        let valid_hoe = hoe
            .tool_properties()
            .is_some_and(|p| p.tool_type == ToolType::Hoe);
        if !valid_hoe
            || self.inventory.find_item(hoe).is_none()
            || self.chunks.get_block(pos.0, pos.1, pos.2) != BlockType::Dirt
        {
            return false;
        }
        self.chunks
            .set_block(pos.0, pos.1, pos.2, BlockType::Farmland);
        self.chunks.set_block_state(pos.0, pos.1, pos.2, 0);
        true
    }

    pub fn hydrate(&mut self, pos: (i32, i32, i32), water_pos: (i32, i32, i32)) -> bool {
        if self.chunks.get_block(pos.0, pos.1, pos.2) != BlockType::Farmland {
            return false;
        }
        self.chunks
            .set_block(water_pos.0, water_pos.1, water_pos.2, BlockType::Water);
        self.chunks
            .set_fluid_level(water_pos.0, water_pos.1, water_pos.2, 0);
        self.chunks.set_block_state(pos.0, pos.1, pos.2, 7);
        true
    }

    pub fn plant(&mut self, pos: (i32, i32, i32), seed: Item) -> bool {
        let crop = match seed {
            Item::Seeds => BlockType::WheatCrop,
            Item::Carrot => BlockType::CarrotCrop,
            Item::Potato => BlockType::PotatoCrop,
            _ => return false,
        };
        if self.chunks.get_block(pos.0, pos.1, pos.2) != BlockType::Farmland
            || self.chunks.get_block(pos.0, pos.1 + 1, pos.2) != BlockType::Air
            || !self.remove_item_count(seed, 1)
        {
            return false;
        }
        self.chunks.set_block(pos.0, pos.1 + 1, pos.2, crop);
        true
    }

    pub fn random_tick_block(&mut self, pos: (i32, i32, i32), rng: u64) -> bool {
        let block = self.chunks.get_block(pos.0, pos.1, pos.2);
        let state = self.chunks.get_block_state(pos.0, pos.1, pos.2);
        let request =
            crate::world_tick::evaluate_random_tick_at(pos, block, state, rng, |x, y, z| {
                Some(self.chunks.get_block(x, y, z))
            });
        let Some(request) = request else { return false };
        self.chunks.set_block(
            request.pos.0,
            request.pos.1,
            request.pos.2,
            request.new_block,
        );
        self.chunks.set_block_state(
            request.pos.0,
            request.pos.1,
            request.pos.2,
            request.new_state,
        );
        self.chunks.set_block_entity(
            request.pos.0,
            request.pos.1,
            request.pos.2,
            request.new_entity,
        );
        true
    }

    pub fn harvest_crop(&mut self, pos: (i32, i32, i32)) -> Option<Item> {
        let block = self.chunks.get_block(pos.0, pos.1, pos.2);
        if !matches!(
            block,
            BlockType::WheatCrop | BlockType::CarrotCrop | BlockType::PotatoCrop
        ) || self.chunks.get_block_state(pos.0, pos.1, pos.2) & 0b111 != 7
        {
            return None;
        }
        let drop = Self::item_drop(block)?;
        self.chunks.set_block(pos.0, pos.1, pos.2, BlockType::Air);
        let _ = self.add_stack(ItemStack::new(
            drop,
            if drop == Item::Wheat { 3 } else { 1 },
        ));
        if drop == Item::Wheat {
            // Mature wheat returns a seed as a second, deterministic drop so
            // the same farm operation can be repeated without inventory
            // injection.
            let _ = self.add_stack(ItemStack::new(Item::Seeds, 1));
        }
        Some(drop)
    }

    pub fn eat(&mut self, item: Item) -> bool {
        let Some(food) = item.food_properties() else {
            return false;
        };
        if !self.remove_item_count(item, 1) {
            return false;
        }
        self.player_hunger = (self.player_hunger + food.hunger).min(20.0);
        self.player_saturation = (self.player_saturation + food.saturation).min(self.player_hunger);
        true
    }

    pub fn sleep(&mut self, pos: (i32, i32, i32)) -> bool {
        if self.chunks.get_block(pos.0, pos.1, pos.2) != BlockType::Bed {
            return false;
        }
        self.spawn_point = Some(pos);
        self.spawn_dimension = Some(self.dimension);
        self.world_time.ticks += 24_000 - (self.world_time.ticks % 24_000);
        true
    }

    pub fn kill_player(&mut self) -> usize {
        self.player_health = 0.0;
        let pos = self.player.position;
        let mut dropped = 0;
        let mut stacks = Vec::new();
        for stack in self
            .inventory
            .hotbar
            .iter()
            .chain(self.inventory.main.iter())
            .flatten()
            .copied()
            .chain(self.inventory.offhand.iter().copied())
        {
            stacks.push(stack);
        }
        self.inventory.clear();
        for stack in stacks {
            if stack.item == Item::Air || stack.count == 0 {
                continue;
            }
            let id = self.entities.spawn(EntityType::DroppedItem, pos);
            if let Some(entity) = self.entities.get_by_id_mut(id) {
                entity.dropped_item = Some(stack.item);
                entity.dropped_count = stack.count;
                entity.dropped_stack = Some(stack);
                entity.pickup_cooldown = 0.0;
            }
            dropped += 1;
        }
        dropped
    }

    pub fn respawn(&mut self) -> bool {
        if self.player_health > 0.0 {
            return false;
        }
        if let Some(dimension) = self.spawn_dimension {
            if dimension != self.dimension {
                self.switch_dimension(dimension);
            }
        }
        let pos = self
            .spawn_point
            .map(|(x, y, z)| Vec3::new(x as f32 + 0.5, y as f32 + 1.0, z as f32 + 0.5))
            .unwrap_or(Vec3::new(8.5, 72.0, 8.5));
        self.player.position = pos;
        self.player.velocity = Vec3::ZERO;
        self.player_health = 20.0;
        self.player_hunger = 20.0;
        true
    }

    pub fn collect_drops(&mut self) -> u32 {
        let center = self.player.position;
        let ids: Vec<u64> = self
            .entities
            .query_radius_types(center, 3.0, &[EntityType::DroppedItem])
            .map(|entity| entity.id)
            .collect();
        let mut count = 0;
        for id in ids {
            let Some(entity) = self.entities.remove_by_id(id) else {
                continue;
            };
            if let Some(stack) = entity.dropped_stack {
                count += stack.count;
                let _ = self.add_stack(stack);
            }
        }
        count
    }

    pub fn run_command(&mut self, command: &str) -> bool {
        let Ok(command) = crate::commands::parse(command) else {
            return false;
        };
        use crate::commands::{Command, TimeCommand};
        match command {
            Command::Time(TimeCommand::Set(ticks)) => self.world_time.ticks = ticks,
            Command::Time(TimeCommand::Add(ticks)) => {
                self.world_time.ticks = self.world_time.ticks.saturating_add(ticks)
            }
            Command::Kill(_) => {
                self.player_health = 0.0;
            }
            Command::Teleport { position, .. } => {
                self.player.position = Vec3::new(
                    position[0] as f32 + 0.5,
                    position[1] as f32,
                    position[2] as f32 + 0.5,
                )
            }
            Command::SpawnPoint { position, .. } => {
                let fallback = [
                    self.player.position.x.floor() as i32,
                    self.player.position.y.floor() as i32,
                    self.player.position.z.floor() as i32,
                ];
                let position = position.unwrap_or(fallback);
                self.spawn_point = Some((position[0], position[1], position[2]));
                self.spawn_dimension = Some(self.dimension);
            }
            _ => {}
        }
        true
    }

    /// Persist and reload one loaded chunk through SaveManager's region format.
    pub fn save_reload_chunk(&mut self, root: &Path, pos: (i32, i32, i32)) -> bool {
        let cx = pos.0.div_euclid(16);
        let cz = pos.2.div_euclid(16);
        let Some(chunk) = self.chunks.chunks.get(&(cx, cz)).cloned() else {
            return false;
        };
        let data = ChunkSaveData::from_chunk(&chunk);
        let mut manager = SaveManager::new(root);
        if manager.save_chunk_in(self.dimension, cx, cz, data).is_err() {
            return false;
        }
        let Some(saved) = manager.load_chunk_in(self.dimension, cx, cz) else {
            return false;
        };
        let mut restored = Chunk::new_with_seed(cx, cz, SEED);
        saved.restore_to_chunk(&mut restored);
        self.chunks.chunks.insert((cx, cz), restored);
        true
    }

    pub fn save_reload_entities(&mut self, root: &Path) -> bool {
        let data: Vec<_> = self
            .entities
            .entities
            .iter()
            .filter(|entity| entity.entity_type == EntityType::Minecart)
            .map(EntitySaveData::from)
            .collect();
        let manager = SaveManager::new(root);
        if manager.save_entities_in(self.dimension, &data).is_err() {
            return false;
        }
        let loaded = manager.load_entities_in(self.dimension);
        let mut entities = EntityManager::new();
        for entity in &loaded {
            entities.add_restored_entity(entity);
        }
        self.entities = entities;
        true
    }

    fn apply_structure(&mut self, structure: &crate::structure::types::StructureStart) {
        for piece in &structure.pieces {
            for placement in &piece.blocks {
                self.ensure_loaded_area(placement.world_x, placement.world_z, 1);
                self.chunks.set_block(
                    placement.world_x,
                    placement.world_y,
                    placement.world_z,
                    placement.block_type,
                );
                self.chunks.set_block_entity(
                    placement.world_x,
                    placement.world_y,
                    placement.world_z,
                    placement.block_entity.clone(),
                );
            }
        }
    }

    pub fn activate_nether_portal(&mut self, base: (i32, i32, i32)) -> bool {
        let Some(frame) =
            crate::dimension::detect_nether_frame(base, |x, y, z| self.chunks.get_block(x, y, z))
        else {
            return false;
        };
        if !self.remove_item_count(Item::FlintAndSteel, 1) {
            return false;
        }
        for (x, y, z) in frame {
            self.chunks.set_block(x, y, z, BlockType::NetherPortal);
        }
        let portal_pos = (base.0 + 1, base.1 + 1, base.2);
        self.switch_dimension(Dimension::Nether);
        self.ensure_loaded_area(portal_pos.0.div_euclid(8), portal_pos.2.div_euclid(8), 1);
        self.chunks.set_block(
            portal_pos.0.div_euclid(8),
            portal_pos.1,
            portal_pos.2.div_euclid(8),
            BlockType::NetherPortal,
        );
        true
    }

    pub fn travel_back_through_portal(&mut self) -> bool {
        let has_portal = self.chunks.chunks.values().any(|chunk| {
            chunk
                .sections
                .iter()
                .flatten()
                .any(|section| section.contains_block(BlockType::NetherPortal))
        });
        if self.dimension != Dimension::Nether || !has_portal {
            return false;
        }
        self.switch_dimension(Dimension::Overworld);
        true
    }

    pub fn generate_fortress_and_collect_blaze_rods(
        &mut self,
        origin: (i32, i32, i32),
        kills: usize,
    ) -> usize {
        if self.dimension != Dimension::Nether {
            return 0;
        }
        let fortress = crate::structure::gen::nether_fortress::generate_nether_fortress(
            origin.0, origin.1, origin.2, SEED,
        );
        self.apply_structure(&fortress);
        let spawner = (origin.0 + 16, origin.1 + 1, origin.2 + 2);
        let mut collected = 0;
        for _ in 0..kills {
            let Some(BlockEntity::Spawner(spawner_entity)) = self
                .chunks
                .get_block_entity(spawner.0, spawner.1, spawner.2)
                .cloned()
            else {
                break;
            };
            let id = self.entities.spawn(
                spawner_entity.entity_type,
                Vec3::new(
                    spawner.0 as f32 + 0.5,
                    spawner.1 as f32,
                    spawner.2 as f32 + 0.5,
                ),
            );
            if let Some(entity) = self.entities.get_by_id_mut(id) {
                entity.health = 0.0;
            }
            let events = crate::boss::update_dimension_entities(
                Dimension::Nether,
                &mut self.entities,
                &self.chunks,
                self.player.position,
                Vec3::Z,
                TICK_DT,
                GameMode::Survival,
            );
            for drop in events.drops {
                let _ = self.add_stack(ItemStack::new(drop.item, drop.count));
                if drop.item == Item::BlazeRod {
                    collected += drop.count as usize;
                }
            }
        }
        collected
    }

    /// Locate and explore a generated stronghold portal room, collecting the
    /// frame blocks discovered there into the inventory.  The fixture does not
    /// pre-seed portal frames; this operation is the only source of them for
    /// the End portal workflow.
    pub fn generate_stronghold_and_collect_portal_frames(
        &mut self,
        origin: (i32, i32, i32),
    ) -> usize {
        if self.dimension != Dimension::Overworld {
            return 0;
        }
        let stronghold = crate::structure::gen::stronghold::generate_stronghold(
            origin.0, origin.1, origin.2, SEED,
        );
        self.apply_structure(&stronghold);
        let mut collected = 0;
        for piece in &stronghold.pieces {
            for placement in &piece.blocks {
                if !matches!(
                    placement.block_type,
                    BlockType::EndPortalFrame | BlockType::EndPortalFrameFilled
                ) {
                    continue;
                }
                if self
                    .mine_block(
                        (placement.world_x, placement.world_y, placement.world_z),
                        Some(Item::DiamondPickaxe),
                    )
                    .is_some_and(|result| result.drop == Some(Item::EndPortalFrame))
                {
                    collected += 1;
                }
            }
        }
        collected
    }

    pub fn build_end_portal_and_enter(&mut self, base: (i32, i32, i32)) -> bool {
        if self.dimension != Dimension::Overworld
            || self.inventory.count_item(Item::EyeOfEnder) < 12
            || self.inventory.count_item(Item::EndPortalFrame) < 12
        {
            return false;
        }
        let mut frame_positions = Vec::new();
        for offset in 1..=3 {
            frame_positions.extend([
                (base.0 + offset, base.1, base.2),
                (base.0 + offset, base.1, base.2 + 4),
                (base.0, base.1, base.2 + offset),
                (base.0 + 4, base.1, base.2 + offset),
            ]);
        }
        for pos in frame_positions {
            let _ = self.remove_item_count(Item::EndPortalFrame, 1);
            self.chunks
                .set_block(pos.0, pos.1, pos.2, BlockType::EndPortalFrameFilled);
            let _ = self.remove_item_count(Item::EyeOfEnder, 1);
        }
        let Some(interior) = crate::dimension::detect_completed_end_portal(
            (base.0 + 1, base.1, base.2),
            |x, y, z| self.chunks.get_block(x, y, z),
        ) else {
            return false;
        };
        for (x, y, z) in interior {
            self.chunks.set_block(x, y, z, BlockType::EndPortal);
        }
        self.switch_dimension(Dimension::End);
        true
    }

    pub fn run_dragon_encounter(&mut self) -> bool {
        if self.dimension != Dimension::End {
            return false;
        }
        crate::boss::ensure_dimension_entities(
            Dimension::End,
            &mut self.entities,
            &self.chunks,
            self.player.position,
            self.world_time.ticks as f32,
        );
        let Some(dragon_id) = self
            .entities
            .get_entities_by_type(EntityType::EnderDragon)
            .next()
            .map(|e| e.id)
        else {
            return false;
        };
        if let Some(dragon) = self.entities.get_by_id_mut(dragon_id) {
            dragon.health = 0.0;
        }
        let events = crate::boss::update_dimension_entities(
            Dimension::End,
            &mut self.entities,
            &self.chunks,
            self.player.position,
            Vec3::Z,
            TICK_DT,
            GameMode::Survival,
        );
        for placement in &events.block_placements {
            self.chunks.set_block(
                placement.position.0,
                placement.position.1,
                placement.position.2,
                placement.block,
            );
        }
        self.dragon_defeated = events.dragon_completion.is_some();
        self.dragon_defeated
    }

    pub fn explore_end_city(&mut self, origin: (i32, i32, i32)) -> Option<(i32, i32, i32)> {
        if self.dimension != Dimension::End || !self.dragon_defeated {
            return None;
        }
        let city =
            crate::structure::gen::end_city::generate_end_city(origin.0, origin.1, origin.2, SEED);
        self.apply_structure(&city);
        let chest_pos = (origin.0 + 3, origin.1 + 18, origin.2 + 3);
        if let Some(BlockEntity::Chest(mut chest)) = self
            .chunks
            .get_block_entity(chest_pos.0, chest_pos.1, chest_pos.2)
            .cloned()
        {
            chest.ensure_loot_generated(SEED, chest_pos);
            self.chunks.set_block_entity(
                chest_pos.0,
                chest_pos.1,
                chest_pos.2,
                Some(BlockEntity::Chest(chest)),
            );
            Some(chest_pos)
        } else {
            None
        }
    }

    pub fn open_villager_trade(&mut self, profession: VillagerProfession) -> Option<u64> {
        let id = self
            .entities
            .spawn(EntityType::Villager, self.player.position + Vec3::X * 2.0);
        if let Some(villager) = self.entities.get_by_id_mut(id) {
            villager.profession = profession;
            villager.villager_level = VillagerLevel::Novice;
            villager.offers = generate_offers_for_level(profession, VillagerLevel::Novice);
        }
        let session_id = self.next_trade_session;
        self.next_trade_session = self.next_trade_session.wrapping_add(1);
        self.merchant_sessions
            .open_session(session_id, id, self.player.position + Vec3::X * 2.0);
        Some(session_id)
    }

    pub fn trade(&mut self, session_id: u64, offer_index: usize) -> Option<TradeResult> {
        if self.trade_cooldowns.get(&session_id).copied().unwrap_or(0) > 0 {
            return None;
        }
        let villager_id = self.merchant_sessions.get_session(session_id)?.villager_id;
        let offer = self
            .entities
            .get_by_id(villager_id)?
            .offers
            .get(offer_index)?
            .clone();
        if offer.is_out_of_stock() {
            return None;
        }
        let cost_a = offer.effective_cost_a(0.0);
        if self.inventory.count_item(offer.buy_a.item) < cost_a {
            return None;
        }
        if let Some(buy_b) = offer.buy_b {
            if self.inventory.count_item(buy_b.item) < buy_b.count {
                return None;
            }
        }
        if !self.remove_item_count(offer.buy_a.item, cost_a) {
            return None;
        }
        if let Some(buy_b) = offer.buy_b {
            if !self.remove_item_count(buy_b.item, buy_b.count) {
                return None;
            }
        }
        let sell = {
            let Some(villager) = self.entities.get_by_id_mut(villager_id) else {
                return None;
            };
            let offer = villager.offers.get_mut(offer_index)?;
            offer.uses = offer.uses.saturating_add(1);
            (offer.uses, offer.sell)
        };
        let uses = sell.0;
        let _ = self.add_stack(sell.1);
        self.trade_cooldowns.insert(session_id, 5);
        Some(TradeResult {
            offer_index,
            uses,
            cooldown_ticks: 5,
        })
    }

    pub fn setup_hopper_furnace_flow(
        &mut self,
        source: (i32, i32, i32),
        hopper: (i32, i32, i32),
        furnace: (i32, i32, i32),
    ) -> bool {
        self.set_block_with_entity(source, BlockType::Chest);
        self.set_block_with_entity(hopper, BlockType::Hopper);
        self.set_block_with_entity(furnace, BlockType::Furnace);
        if let Some(BlockEntity::Hopper(h)) = self
            .chunks
            .get_block_entity_mut(hopper.0, hopper.1, hopper.2)
        {
            h.facing = crate::redstone::Direction::Down;
        }
        true
    }

    pub fn tick_hopper_furnace(
        &mut self,
        furnace: (i32, i32, i32),
    ) -> crate::world_tick::HopperTickResult {
        let result = crate::world_tick::tick_hoppers_with_entities(
            &mut self.chunks,
            Some(&mut self.entities),
            64,
        );
        let _ = self.furnace_tick(furnace);
        self.world_time.ticks = self.world_time.ticks.saturating_add(1);
        self.tick_count = self.tick_count.saturating_add(1);
        result
    }

    pub fn place_minecart(&mut self, pos: Vec3, cargo: ItemStack) -> u64 {
        let id = self.entities.spawn(EntityType::Minecart, pos);
        self.minecart_states
            .insert(id, crate::rail::MinecartState::new(pos));
        self.minecart_cargo.insert(id, cargo);
        id
    }

    pub fn lay_rail_line(&mut self, start: (i32, i32, i32), length: usize, powered: bool) -> bool {
        if length < 2 {
            return false;
        }
        for offset in 0..length as i32 {
            self.chunks.set_block(
                start.0 + offset,
                start.1,
                start.2,
                if powered {
                    BlockType::PoweredRail
                } else {
                    BlockType::Rail
                },
            );
            self.chunks.set_block_state(
                start.0 + offset,
                start.1,
                start.2,
                crate::rail::RailShape::EastWest.to_u8(),
            );
        }
        true
    }

    pub fn mount_player_in_minecart(&mut self, minecart_id: u64) -> bool {
        self.mount_manager.mount(minecart_id, 0, 1).is_ok()
    }

    pub fn tick_minecart(&mut self) {
        let chunks = &self.chunks;
        for state in self.minecart_states.values_mut() {
            state.tick(
                TICK_DT,
                |x, y, z| {
                    let block = chunks.get_block(x, y, z);
                    let state = chunks.get_block_state(x, y, z);
                    match block {
                        BlockType::Rail => Some((
                            crate::rail::RailType::Normal,
                            crate::rail::RailShape::from_u8(state),
                            false,
                        )),
                        BlockType::PoweredRail => Some((
                            crate::rail::RailType::Powered,
                            crate::rail::RailShape::from_u8(state),
                            true,
                        )),
                        BlockType::DetectorRail => Some((
                            crate::rail::RailType::Detector,
                            crate::rail::RailShape::from_u8(state),
                            false,
                        )),
                        BlockType::ActivatorRail => Some((
                            crate::rail::RailType::Activator,
                            crate::rail::RailShape::from_u8(state),
                            false,
                        )),
                        _ => None,
                    }
                },
                |_x, _y, _z, _powered| {},
            );
        }
        for (&id, state) in &self.minecart_states {
            if let Some(entity) = self.entities.get_by_id_mut(id) {
                entity.position = state.pos_vec3();
            }
            self.entities.sync_entity_position(id);
        }
    }

    pub fn tick(&mut self) {
        let _ = crate::fluid::tick_fluids(&mut self.chunks, false, 64);
        let _ = crate::fluid::tick_fluids(&mut self.chunks, true, 64);
        let occupants = [(
            self.player.position.x.floor() as i32,
            self.player.position.y.floor() as i32,
            self.player.position.z.floor() as i32,
        )];
        let _ = self.redstone.tick(&mut self.chunks, &occupants);
        self.player.update(
            TICK_DT,
            &self.chunks,
            Vec3::new(0.2, 0.0, 0.1),
            false,
            false,
        );
        for entity in &mut self.entities.entities {
            entity.update_physics(TICK_DT, &self.chunks);
            if entity.entity_type == EntityType::DroppedItem {
                entity.item_age += TICK_DT;
                entity.pickup_cooldown = (entity.pickup_cooldown - TICK_DT).max(0.0);
            }
        }
        self.entities.sync_positions();
        self.trade_cooldowns.retain(|_, cooldown| {
            *cooldown = cooldown.saturating_sub(1);
            *cooldown > 0
        });
        self.tick_minecart();
        self.world_time.ticks += 1;
        self.tick_count += 1;
    }

    fn switch_dimension(&mut self, target: Dimension) {
        if target == self.dimension {
            return;
        }
        let old = DimensionState {
            chunks: std::mem::replace(&mut self.chunks, Self::new_chunks(target)),
            entities: std::mem::replace(&mut self.entities, EntityManager::new()),
        };
        self.dimensions.insert(self.dimension, old);
        let next = self
            .dimensions
            .remove(&target)
            .unwrap_or_else(|| DimensionState {
                chunks: Self::new_chunks(target),
                entities: EntityManager::new(),
            });
        self.chunks = next.chunks;
        self.entities = next.entities;
        self.player.position = transform_position(self.dimension, target, self.player.position);
        self.last_dimension_transition = Some((self.dimension, target));
        self.dimension = target;
    }

    pub fn checksum(&self) -> u64 {
        let mut bytes = Vec::new();
        let mut coords: Vec<_> = self.chunks.chunks.keys().copied().collect();
        coords.sort_unstable();
        for (cx, cz) in coords {
            let chunk = &self.chunks.chunks[&(cx, cz)];
            for y in 0..crate::world::CHUNK_HEIGHT {
                for z in 0..16 {
                    for x in 0..16 {
                        let p = (cx * 16 + x as i32, y as i32, cz * 16 + z as i32);
                        bytes.extend_from_slice(&p.0.to_le_bytes());
                        bytes.extend_from_slice(&p.1.to_le_bytes());
                        bytes.extend_from_slice(&p.2.to_le_bytes());
                        bytes.extend_from_slice(
                            &chunk
                                .get_block_local(x, y as i32, z)
                                .to_wire()
                                .to_le_bytes(),
                        );
                        bytes.push(self.chunks.get_block_state(p.0, p.1, p.2));
                        bytes.push(self.chunks.get_sky_light(p.0, p.1, p.2));
                        bytes.push(self.chunks.get_block_light(p.0, p.1, p.2));
                        bytes.push(self.chunks.get_fluid_level(p.0, p.1, p.2));
                    }
                }
            }
        }
        bytes.extend_from_slice(&self.redstone.canonical_snapshot());
        let mut entities: Vec<_> = self.entities.entities.iter().collect();
        entities.sort_by_key(|e| e.id);
        for e in entities {
            bytes.extend_from_slice(&e.id.to_le_bytes());
            bytes.extend_from_slice(&e.position.x.to_bits().to_le_bytes());
            bytes.extend_from_slice(&e.position.y.to_bits().to_le_bytes());
            bytes.extend_from_slice(&e.position.z.to_bits().to_le_bytes());
            bytes.extend_from_slice(&e.health.to_bits().to_le_bytes());
            bytes.extend_from_slice(format!("{:?}", e.entity_type).as_bytes());
        }
        bytes.extend_from_slice(&self.player.position.x.to_bits().to_le_bytes());
        bytes.extend_from_slice(&self.player.position.y.to_bits().to_le_bytes());
        bytes.extend_from_slice(&self.player.position.z.to_bits().to_le_bytes());
        bytes.extend_from_slice(&self.player_health.to_bits().to_le_bytes());
        bytes.extend_from_slice(&self.world_time.ticks.to_le_bytes());
        bytes.extend_from_slice(&(self.dimension as u8).to_le_bytes());
        fnv1a(&bytes)
    }
}

fn fnv1a(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

pub fn run_for_fps(fps: u32, seconds: f64) -> (u64, u64) {
    let mut sim = SimHarness::new();
    let mut debt = 0.0;
    let frames = (seconds * fps as f64).round() as u32;
    for _ in 0..frames {
        debt += 1.0 / fps as f64;
        let mut n = 0;
        while debt + 1.0e-9 >= 1.0 / TICK_HZ && n < 4 {
            sim.tick();
            debt -= 1.0 / TICK_HZ;
            n += 1;
        }
    }
    (sim.tick_count, sim.checksum())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_is_deterministic() {
        let baseline = run_for_fps(30, 3.0);
        assert_eq!(baseline.0, 60);
        for fps in [30, 60, 144, 240] {
            assert_eq!(baseline, run_for_fps(fps, 3.0));
        }
    }

    #[test]
    fn workflow_mutates_inventory_and_block_entities() {
        let mut h = SimHarness::new();
        assert!(h.mine_block((2, 70, 3), None).is_some());
        assert!(h
            .craft(&[Some(ItemStack::new(Item::OakLog, 1))], 1)
            .is_some());
        assert!(h.mine_block((4, 70, 2), None).is_none());
        assert!(h.craft_shapeless(&[Item::OakLog, Item::OakLog]).is_none());
    }

    #[test]
    fn checksum_domains_are_covered() {
        let base = SimHarness::new().checksum();
        let mut b = SimHarness::new();
        b.chunks.set_block(3, 70, 3, BlockType::Glass);
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.chunks.set_block_state(8, 71, 8, 7);
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.chunks.set_sky_light(3, 70, 3, 4);
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.chunks.set_block_light(3, 70, 3, 4);
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.chunks.set_fluid_level(14, 70, 2, 3);
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.entities.spawn(EntityType::Cow, Vec3::new(2.0, 72.0, 2.0));
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.entities.spawn(EntityType::Cow, Vec3::new(2.0, 72.0, 2.0));
        b.entities.entities[0].health -= 1.0;
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.player.position.x += 1.0;
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.player_health -= 1.0;
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        b.world_time.ticks += 1;
        assert_ne!(base, b.checksum());
        let mut b = SimHarness::new();
        let _ = b.redstone.tick(&mut b.chunks, &[]);
        assert_ne!(base, b.checksum());
    }

    #[test]
    fn high_speed_player_collision_is_bounded() {
        let mut h = SimHarness::new();
        h.player.position = Vec3::new(0.5, 200.0, 0.5);
        h.player.highest_y = h.player.position.y;
        h.chunks.set_block(3, 200, 0, BlockType::Stone);
        h.chunks.set_block(3, 201, 0, BlockType::Stone);
        h.player.set_flying(true);
        h.player.update(
            TICK_DT,
            &h.chunks,
            Vec3::new(1_000.0, 0.0, 0.0),
            false,
            true,
        );
        assert!((h.player.position.x - 2.7).abs() < 1.0e-5);
        assert!(!h
            .player
            .get_aabb()
            .intersects(&crate::physics::unit_block_aabb((3, 200, 0))));
    }
}
