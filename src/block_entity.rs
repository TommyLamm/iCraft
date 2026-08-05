#[allow(unused_imports)]
use crate::inventory::{Item, ItemStack};
use crate::recipes::{FuelDefinition, RecipeManager};
use crate::world::BlockType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChestBlockEntity {
    pub custom_name: Option<String>,
    pub inventory: crate::inventory::ContainerInventory,
    #[serde(default)]
    pub loot_table: Option<String>,
    #[serde(default)]
    pub loot_seed: Option<u64>,
}

impl ChestBlockEntity {
    pub fn ensure_loot_generated(&mut self, world_seed: u32, pos: (i32, i32, i32)) {
        if let Some(table_str) = self.loot_table.take() {
            let seed = self.loot_seed.take().unwrap_or_else(|| {
                let mut state = (world_seed as u64)
                    .wrapping_add((pos.0 as u64).wrapping_mul(0x9E37_79B9))
                    .wrapping_add((pos.1 as u64).wrapping_mul(0x85EB_CA6B))
                    .wrapping_add((pos.2 as u64).wrapping_mul(0xC2B2_AE35));
                if state == 0 {
                    state = 1;
                }
                state
            });
            if let Some(id) = crate::loot::LootTableId::from_str(&table_str) {
                let rolled = crate::loot::roll_loot_table(id, seed);
                for (slot_idx, item_stack) in rolled.into_iter().enumerate() {
                    if slot_idx < self.inventory.slots.len() {
                        self.inventory.slots[slot_idx] = Some(item_stack);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FurnaceBlockEntity {
    pub custom_name: Option<String>,
    pub slots: [Option<ItemStack>; 3],
    pub burn_time: u16,
    pub burn_total: u16,
    pub cook_progress: u16,
    pub cook_total: u16,
    pub accumulated_xp: f32,
    pub is_lit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FurnaceStub {
    pub custom_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LegacyBlockEntity {
    Chest(ChestBlockEntity),
    Furnace(FurnaceStub),
    Sign(SignStub),
}

impl From<LegacyBlockEntity> for BlockEntity {
    fn from(legacy: LegacyBlockEntity) -> Self {
        match legacy {
            LegacyBlockEntity::Chest(c) => BlockEntity::Chest(c),
            LegacyBlockEntity::Furnace(f) => {
                BlockEntity::Furnace(FurnaceBlockEntity::new_with_name(f.custom_name))
            }
            LegacyBlockEntity::Sign(s) => BlockEntity::Sign(s),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FurnaceTickResult {
    pub item_smelted: bool,
    pub lit_changed: bool,
    pub slot_changed: bool,
}

impl FurnaceBlockEntity {
    pub fn new() -> Self {
        Self::new_with_name(None)
    }

    pub fn new_with_name(custom_name: Option<String>) -> Self {
        Self {
            custom_name,
            slots: [None, None, None],
            burn_time: 0,
            burn_total: 0,
            cook_progress: 0,
            cook_total: 200,
            accumulated_xp: 0.0,
            is_lit: false,
        }
    }

    pub fn claim_xp(&mut self) -> f32 {
        let xp = self.accumulated_xp;
        self.accumulated_xp = 0.0;
        xp
    }

    pub fn tick(&mut self, recipes: &RecipeManager) -> FurnaceTickResult {
        let mut result = FurnaceTickResult {
            item_smelted: false,
            lit_changed: false,
            slot_changed: false,
        };

        // Input slot: 0, Fuel slot: 1, Output slot: 2
        let input_stack = self.slots[0].as_ref();
        let smelting_recipe = input_stack.and_then(|st| recipes.find_smelting_recipe(st.item));

        let can_smelt = if let Some(recipe) = smelting_recipe {
            let output_slot = self.slots[2].as_ref();
            match output_slot {
                None => true,
                Some(out_st) => {
                    let max_st = out_st.item.properties().max_stack;
                    out_st.item == recipe.output.item
                        && out_st.count + recipe.output.count <= max_st
                }
            }
        } else {
            false
        };

        // 1. Consume fuel if unlit (burn_time == 0) and smelting is available
        if self.burn_time == 0 && can_smelt {
            if let Some(fuel_st) = self.slots[1].as_mut() {
                let burn_dur = FuelDefinition::burn_time(fuel_st.item);
                if burn_dur > 0 && fuel_st.count > 0 {
                    self.burn_time = burn_dur;
                    self.burn_total = burn_dur;
                    fuel_st.count -= 1;
                    if fuel_st.count == 0 {
                        self.slots[1] = None;
                    }
                    result.slot_changed = true;
                }
            }
        }

        // 2. Decay active burn time
        if self.burn_time > 0 {
            self.burn_time -= 1;
            result.slot_changed = true;
        }

        // 3. Cook progress
        if self.burn_time > 0 && can_smelt {
            let recipe = smelting_recipe.unwrap();
            self.cook_total = recipe.cook_time;
            self.cook_progress += 1;
            result.slot_changed = true;

            if self.cook_progress >= self.cook_total {
                self.cook_progress = 0;

                // Consume 1 input item
                if let Some(input_st) = self.slots[0].as_mut() {
                    input_st.count -= 1;
                    if input_st.count == 0 {
                        self.slots[0] = None;
                    }
                }

                // Add output item
                if let Some(out_st) = self.slots[2].as_mut() {
                    out_st.count += recipe.output.count;
                } else {
                    self.slots[2] = Some(recipe.output.clone());
                }

                self.accumulated_xp += recipe.experience;
                result.item_smelted = true;
            }
        } else {
            if self.cook_progress > 0 {
                self.cook_progress = self.cook_progress.saturating_sub(2);
                result.slot_changed = true;
            }
        }

        // 4. Update lit status
        let new_is_lit = self.burn_time > 0;
        if new_is_lit != self.is_lit {
            self.is_lit = new_is_lit;
            result.lit_changed = true;
        }

        result
    }
}

impl Default for FurnaceBlockEntity {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for FurnaceBlockEntity {
    fn eq(&self, other: &Self) -> bool {
        self.custom_name == other.custom_name
            && self.slots == other.slots
            && self.burn_time == other.burn_time
            && self.burn_total == other.burn_total
            && self.cook_progress == other.cook_progress
            && self.cook_total == other.cook_total
            && self.accumulated_xp.to_bits() == other.accumulated_xp.to_bits()
            && self.is_lit == other.is_lit
    }
}

impl Eq for FurnaceBlockEntity {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignBlockEntity {
    pub lines: [String; 4],
}

impl SignBlockEntity {
    pub fn new() -> Self {
        Self {
            lines: [String::new(), String::new(), String::new(), String::new()],
        }
    }

    pub fn from_text(text: &str) -> Self {
        let mut sign = Self::new();
        for (i, line) in text.lines().take(4).enumerate() {
            sign.set_line(i, line);
        }
        sign
    }

    pub fn set_line(&mut self, line_idx: usize, text: &str) {
        if line_idx < 4 {
            let sanitized: String = text.chars().take(15).collect();
            self.lines[line_idx] = sanitized;
        }
    }
}

impl Default for SignBlockEntity {
    fn default() -> Self {
        Self::new()
    }
}

pub type SignStub = SignBlockEntity;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnerBlockEntity {
    pub entity_type: crate::entity::EntityType,
    #[serde(default)]
    pub spawn_delay: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockEntity {
    Chest(ChestBlockEntity),
    Furnace(FurnaceBlockEntity),
    Sign(SignBlockEntity),
    Spawner(SpawnerBlockEntity),
}

impl BlockEntity {
    pub fn matches_block_type(&self, block_type: BlockType) -> bool {
        match self {
            BlockEntity::Chest(_) => {
                matches!(block_type, BlockType::Chest | BlockType::EndCityChest)
            }
            BlockEntity::Furnace(_) => {
                matches!(block_type, BlockType::Furnace | BlockType::FurnaceLit)
            }
            BlockEntity::Sign(_) => matches!(block_type, BlockType::OakSign),
            BlockEntity::Spawner(_) => matches!(block_type, BlockType::Spawner),
        }
    }

    pub fn memory_usage(&self) -> usize {
        let base = std::mem::size_of::<Self>();
        let extra = match self {
            BlockEntity::Chest(c) => c.custom_name.as_ref().map_or(0, |s| s.capacity()),
            BlockEntity::Furnace(f) => f.custom_name.as_ref().map_or(0, |s| s.capacity()),
            BlockEntity::Sign(s) => s.lines.iter().map(|l| l.capacity()).sum(),
            BlockEntity::Spawner(_) => 0,
        };
        base + extra
    }
}

pub fn default_stub_for_block(block_type: BlockType) -> Option<BlockEntity> {
    match block_type {
        BlockType::Chest | BlockType::EndCityChest => Some(BlockEntity::Chest(ChestBlockEntity {
            custom_name: None,
            inventory: crate::inventory::ContainerInventory::new(),
            loot_table: None,
            loot_seed: None,
        })),
        BlockType::Furnace | BlockType::FurnaceLit => {
            Some(BlockEntity::Furnace(FurnaceBlockEntity::new()))
        }
        BlockType::OakSign => Some(BlockEntity::Sign(SignBlockEntity::new())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_entity_matching() {
        let chest = BlockEntity::Chest(ChestBlockEntity {
            custom_name: None,
            inventory: crate::inventory::ContainerInventory::new(),
            loot_table: None,
            loot_seed: None,
        });
        assert!(chest.matches_block_type(BlockType::Chest));
        assert!(chest.matches_block_type(BlockType::EndCityChest));
        assert!(!chest.matches_block_type(BlockType::Furnace));
        assert!(!chest.matches_block_type(BlockType::Dirt));

        let furnace = BlockEntity::Furnace(FurnaceBlockEntity::new());
        assert!(furnace.matches_block_type(BlockType::Furnace));
        assert!(furnace.matches_block_type(BlockType::FurnaceLit));
        assert!(!furnace.matches_block_type(BlockType::Chest));

        let sign = BlockEntity::Sign(SignBlockEntity::from_text("Hello"));
        assert!(sign.matches_block_type(BlockType::OakSign));
        assert!(!sign.matches_block_type(BlockType::Dirt));
    }

    #[test]
    fn test_sign_line_truncation() {
        let mut sign = SignBlockEntity::new();
        sign.set_line(0, "12345678901234567890"); // 20 chars
        assert_eq!(sign.lines[0], "123456789012345"); // Max 15 chars
    }

    #[test]
    fn test_furnace_smelting_flow() {
        let recipes = RecipeManager::new();
        let mut furnace = FurnaceBlockEntity::new();

        // Put 1 IronOre in input (0) and 1 Coal in fuel (1)
        furnace.slots[0] = Some(ItemStack::new(Item::IronOre, 1));
        furnace.slots[1] = Some(ItemStack::new(Item::Coal, 1));

        // Tick 1: consumes coal (1600), then decays by 1 -> 1599, cook_progress = 1
        let res = furnace.tick(&recipes);
        assert!(res.lit_changed);
        assert!(furnace.is_lit);
        assert_eq!(furnace.burn_time, 1599);
        assert_eq!(furnace.burn_total, 1600);
        assert_eq!(furnace.cook_progress, 1);
        assert!(furnace.slots[1].is_none()); // Coal consumed

        // Tick 199 more times -> cook_progress reaches 200, item smelted!
        for _ in 0..199 {
            furnace.tick(&recipes);
        }

        assert_eq!(furnace.slots[0], None); // IronOre consumed
        assert_eq!(furnace.slots[2], Some(ItemStack::new(Item::IronIngot, 1)));
        assert_eq!(furnace.accumulated_xp, 0.7);

        // Claim XP
        assert_eq!(furnace.claim_xp(), 0.7);
        assert_eq!(furnace.accumulated_xp, 0.0);
    }

    #[test]
    fn test_furnace_output_full_stops_fuel_consumption() {
        let recipes = RecipeManager::new();
        let mut furnace = FurnaceBlockEntity::new();

        furnace.slots[0] = Some(ItemStack::new(Item::IronOre, 1));
        furnace.slots[1] = Some(ItemStack::new(Item::Coal, 1));
        furnace.slots[2] = Some(ItemStack::new(Item::IronIngot, 64)); // Full output slot

        // Tick: should NOT consume fuel or cook
        let res = furnace.tick(&recipes);
        assert!(!res.lit_changed);
        assert!(!furnace.is_lit);
        assert_eq!(furnace.burn_time, 0);
        assert_eq!(furnace.slots[1], Some(ItemStack::new(Item::Coal, 1)));
    }

    #[test]
    fn test_legacy_furnace_stub_migration() {
        let legacy_stub = LegacyBlockEntity::Furnace(FurnaceStub {
            custom_name: Some("Old Furnace".to_string()),
        });
        let bytes = bincode::serialize(&legacy_stub).unwrap();

        let legacy_de: LegacyBlockEntity = bincode::deserialize(&bytes).unwrap();
        let migrated: BlockEntity = legacy_de.into();

        if let BlockEntity::Furnace(f) = migrated {
            assert_eq!(f.custom_name, Some("Old Furnace".to_string()));
            assert_eq!(f.burn_time, 0);
            assert_eq!(f.cook_progress, 0);
            assert_eq!(f.cook_total, 200);
            assert_eq!(f.slots, [None, None, None]);
        } else {
            panic!("Expected BlockEntity::Furnace");
        }
    }
}
