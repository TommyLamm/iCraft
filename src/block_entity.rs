#[allow(unused_imports)]
use crate::inventory::{Item, ItemStack};
use crate::recipes::{FuelDefinition, RecipeManager};
use crate::world::BlockType;
use serde::{Deserialize, Serialize};

/// The stable capability surface shared by the container UI and automation.
/// Callers use this value for validation and never infer slot layout from a
/// block id, which keeps sided furnace rules in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerAccess {
    pub kind: ContainerKind,
    pub slot_count: usize,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    Chest,
    Furnace,
    Hopper,
    Dispenser,
    Dropper,
}

impl ContainerAccess {
    pub fn for_entity(entity: &BlockEntity) -> Option<Self> {
        let (kind, slot_count, revision) = match entity {
            BlockEntity::Chest(chest) => (
                ContainerKind::Chest,
                chest.inventory.slots.len(),
                chest.revision,
            ),
            BlockEntity::Furnace(furnace) => (
                ContainerKind::Furnace,
                furnace.slots.len(),
                furnace.revision,
            ),
            BlockEntity::Hopper(hopper) => {
                (ContainerKind::Hopper, hopper.slots.len(), hopper.revision)
            }
            BlockEntity::Dispenser(dispenser) => (
                ContainerKind::Dispenser,
                dispenser.slots.len(),
                dispenser.revision,
            ),
            BlockEntity::Dropper(dropper) => (
                ContainerKind::Dropper,
                dropper.slots.len(),
                dropper.revision,
            ),
            BlockEntity::Sign(_) | BlockEntity::Spawner(_) | BlockEntity::Observer(_) => {
                return None
            }
        };
        Some(Self {
            kind,
            slot_count,
            revision,
        })
    }

    pub fn can_insert(
        self,
        slot: usize,
        item: &ItemStack,
        side: Option<crate::redstone::Direction>,
    ) -> bool {
        if slot >= self.slot_count || item.count == 0 || item.item == Item::Air {
            return false;
        }
        match self.kind {
            ContainerKind::Chest
            | ContainerKind::Hopper
            | ContainerKind::Dispenser
            | ContainerKind::Dropper => true,
            ContainerKind::Furnace => match side {
                Some(crate::redstone::Direction::Up) => slot == 0,
                Some(crate::redstone::Direction::Down) => false,
                Some(_) => slot == 1 && FuelDefinition::burn_time(item.item) > 0,
                None => slot < 2 && (slot == 0 || FuelDefinition::burn_time(item.item) > 0),
            },
        }
    }

    pub fn can_extract(self, slot: usize, side: Option<crate::redstone::Direction>) -> bool {
        if slot >= self.slot_count {
            return false;
        }
        match self.kind {
            ContainerKind::Chest
            | ContainerKind::Hopper
            | ContainerKind::Dispenser
            | ContainerKind::Dropper => true,
            ContainerKind::Furnace => match side {
                Some(crate::redstone::Direction::Down) => slot == 2,
                Some(_) => slot == 2,
                // `None` is the player UI capability.  Players may take
                // input, fuel, or output; sided automation remains limited
                // to the output slot above.
                None => true,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChestBlockEntity {
    pub custom_name: Option<String>,
    pub inventory: crate::inventory::ContainerInventory,
    #[serde(default)]
    pub loot_table: Option<String>,
    #[serde(default)]
    pub loot_seed: Option<u64>,
    #[serde(default)]
    pub revision: u64,
}

impl Default for ChestBlockEntity {
    fn default() -> Self {
        Self {
            custom_name: None,
            inventory: crate::inventory::ContainerInventory::new(),
            loot_table: None,
            loot_seed: None,
            revision: 0,
        }
    }
}

impl ChestBlockEntity {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test/legacy convenience used by single-chest world fixtures.
    pub fn set_stack(&mut self, slot: usize, stack: Option<ItemStack>) {
        if slot < self.inventory.slots.len() {
            self.inventory.slots[slot] = stack;
            self.revision = self.revision.wrapping_add(1);
        }
    }

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
    #[serde(default)]
    pub revision: u64,
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
            revision: 0,
        }
    }

    pub fn set_stack(&mut self, slot: usize, stack: Option<ItemStack>) {
        if slot < self.slots.len() {
            self.slots[slot] = stack;
            self.revision = self.revision.wrapping_add(1);
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
pub struct HopperBlockEntity {
    pub custom_name: Option<String>,
    pub slots: [Option<ItemStack>; 5],
    pub transfer_cooldown: u8,
    #[serde(default)]
    pub facing: crate::redstone::Direction,
    #[serde(default)]
    pub is_powered: bool,
    #[serde(default)]
    pub revision: u64,
}

impl HopperBlockEntity {
    pub fn new() -> Self {
        Self {
            custom_name: None,
            slots: [None, None, None, None, None],
            transfer_cooldown: 0,
            facing: crate::redstone::Direction::Down,
            is_powered: false,
            revision: 0,
        }
    }

    pub fn with_facing(facing: crate::redstone::Direction) -> Self {
        Self {
            facing,
            ..Self::new()
        }
    }
}

impl Default for HopperBlockEntity {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispenserBlockEntity {
    pub custom_name: Option<String>,
    pub slots: [Option<ItemStack>; 9],
    #[serde(default)]
    pub revision: u64,
}

impl DispenserBlockEntity {
    pub fn new() -> Self {
        Self {
            custom_name: None,
            slots: [None, None, None, None, None, None, None, None, None],
            revision: 0,
        }
    }
}

impl Default for DispenserBlockEntity {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropperBlockEntity {
    pub custom_name: Option<String>,
    pub slots: [Option<ItemStack>; 9],
    #[serde(default)]
    pub revision: u64,
}

impl DropperBlockEntity {
    pub fn new() -> Self {
        Self {
            custom_name: None,
            slots: [None, None, None, None, None, None, None, None, None],
            revision: 0,
        }
    }
}

impl Default for DropperBlockEntity {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObserverBlockEntity {
    #[serde(default)]
    pub facing: crate::redstone::Direction,
    /// Two redstone ticks are represented by a pending pulse countdown.
    #[serde(default)]
    pub pending_pulse: u8,
    /// Baseline is persisted so a chunk reload does not create a phantom pulse.
    #[serde(default)]
    pub baseline_initialized: bool,
    #[serde(default = "default_observed_block")]
    pub observed_block: BlockType,
    #[serde(default)]
    pub observed_state: u8,
    #[serde(default)]
    pub observed_entity_revision: u64,
    #[serde(default)]
    pub observed_entity_present: bool,
    #[serde(default)]
    pub revision: u64,
}

fn default_observed_block() -> BlockType {
    BlockType::Air
}

impl ObserverBlockEntity {
    pub fn new() -> Self {
        Self {
            facing: crate::redstone::Direction::North,
            pending_pulse: 0,
            baseline_initialized: false,
            observed_block: BlockType::Air,
            observed_state: 0,
            observed_entity_revision: 0,
            observed_entity_present: false,
            revision: 0,
        }
    }
}

impl Default for ObserverBlockEntity {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockEntity {
    Chest(ChestBlockEntity),
    Furnace(FurnaceBlockEntity),
    Sign(SignBlockEntity),
    Spawner(SpawnerBlockEntity),
    Hopper(HopperBlockEntity),
    Dispenser(DispenserBlockEntity),
    Dropper(DropperBlockEntity),
    Observer(ObserverBlockEntity),
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
            BlockEntity::Hopper(_) => matches!(block_type, BlockType::Hopper),
            BlockEntity::Dispenser(_) => matches!(block_type, BlockType::Dispenser),
            BlockEntity::Dropper(_) => matches!(block_type, BlockType::Dropper),
            BlockEntity::Observer(_) => matches!(block_type, BlockType::Observer),
        }
    }

    pub fn slot_count(&self) -> usize {
        ContainerAccess::for_entity(self)
            .map(|access| access.slot_count)
            .unwrap_or(0)
    }

    pub fn get_stack(&self, slot: usize) -> Option<&ItemStack> {
        match self {
            BlockEntity::Chest(c) => c.inventory.slots.get(slot).and_then(|s| s.as_ref()),
            BlockEntity::Furnace(f) => f.slots.get(slot).and_then(|s| s.as_ref()),
            BlockEntity::Hopper(h) => h.slots.get(slot).and_then(|s| s.as_ref()),
            BlockEntity::Dispenser(d) => d.slots.get(slot).and_then(|s| s.as_ref()),
            BlockEntity::Dropper(d) => d.slots.get(slot).and_then(|s| s.as_ref()),
            _ => None,
        }
    }

    pub fn get_stack_mut(&mut self, slot: usize) -> Option<&mut ItemStack> {
        match self {
            BlockEntity::Chest(c) => c.inventory.slots.get_mut(slot)?.as_mut(),
            BlockEntity::Furnace(f) => f.slots.get_mut(slot)?.as_mut(),
            BlockEntity::Hopper(h) => h.slots.get_mut(slot)?.as_mut(),
            BlockEntity::Dispenser(d) => d.slots.get_mut(slot)?.as_mut(),
            BlockEntity::Dropper(d) => d.slots.get_mut(slot)?.as_mut(),
            _ => None,
        }
    }

    pub fn select_random_non_empty_slot(&self, seed: u64) -> Option<usize> {
        let slots: &[Option<ItemStack>] = match self {
            BlockEntity::Chest(c) => &c.inventory.slots,
            BlockEntity::Furnace(f) => &f.slots,
            BlockEntity::Hopper(h) => &h.slots,
            BlockEntity::Dispenser(d) => &d.slots,
            BlockEntity::Dropper(d) => &d.slots,
            _ => return None,
        };
        let non_empty: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.as_ref().filter(|s| s.count > 0).map(|_| i))
            .collect();
        if non_empty.is_empty() {
            None
        } else {
            let idx = (seed as usize) % non_empty.len();
            Some(non_empty[idx])
        }
    }

    pub fn set_stack(&mut self, slot: usize, stack: Option<ItemStack>) {
        match self {
            BlockEntity::Chest(c) => {
                if slot < c.inventory.slots.len() {
                    c.inventory.slots[slot] = stack;
                    c.revision = c.revision.wrapping_add(1);
                }
            }
            BlockEntity::Furnace(f) => {
                if slot < f.slots.len() {
                    f.slots[slot] = stack;
                    f.revision = f.revision.wrapping_add(1);
                }
            }
            BlockEntity::Hopper(h) => {
                if slot < h.slots.len() {
                    h.slots[slot] = stack;
                    h.revision = h.revision.wrapping_add(1);
                }
            }
            BlockEntity::Dispenser(d) => {
                if slot < d.slots.len() {
                    d.slots[slot] = stack;
                    d.revision = d.revision.wrapping_add(1);
                }
            }
            BlockEntity::Dropper(d) => {
                if slot < d.slots.len() {
                    d.slots[slot] = stack;
                    d.revision = d.revision.wrapping_add(1);
                }
            }
            BlockEntity::Observer(_) => {}
            _ => {}
        }
    }

    /// Atomically replaces the complete slot vector used by a UI transaction
    /// or a replicated snapshot. The caller validates the touched slot's sided
    /// capability before calling this method; this method only commits after
    /// the exact slot count and stack invariants are known to be valid.
    pub fn replace_slots(&mut self, slots: &[Option<ItemStack>]) -> bool {
        if slots.len() != self.slot_count()
            || slots.iter().flatten().any(|stack| {
                stack.count == 0
                    || stack.item == Item::Air
                    || stack.count > stack.item.properties().max_stack
            })
        {
            return false;
        }
        let changed =
            (0..self.slot_count()).any(|slot| self.get_stack(slot).copied() != slots[slot]);
        if !changed {
            return true;
        }
        match self {
            BlockEntity::Chest(chest) => {
                let mut next = [None; 27];
                next.copy_from_slice(slots);
                chest.inventory.slots = next;
                chest.revision = chest.revision.wrapping_add(1);
            }
            BlockEntity::Furnace(furnace) => {
                furnace.slots.copy_from_slice(slots);
                furnace.revision = furnace.revision.wrapping_add(1);
            }
            BlockEntity::Hopper(hopper) => {
                hopper.slots.copy_from_slice(slots);
                hopper.revision = hopper.revision.wrapping_add(1);
            }
            BlockEntity::Dispenser(dispenser) => {
                dispenser.slots.copy_from_slice(slots);
                dispenser.revision = dispenser.revision.wrapping_add(1);
            }
            BlockEntity::Dropper(dropper) => {
                dropper.slots.copy_from_slice(slots);
                dropper.revision = dropper.revision.wrapping_add(1);
            }
            BlockEntity::Sign(_) | BlockEntity::Spawner(_) | BlockEntity::Observer(_) => {
                return false
            }
        }
        true
    }

    pub fn can_insert_item(
        &self,
        slot: usize,
        item: &ItemStack,
        side: Option<crate::redstone::Direction>,
    ) -> bool {
        ContainerAccess::for_entity(self)
            .map(|access| access.can_insert(slot, item, side))
            .unwrap_or(false)
    }

    pub fn can_extract_item(&self, slot: usize, side: Option<crate::redstone::Direction>) -> bool {
        ContainerAccess::for_entity(self)
            .map(|access| access.can_extract(slot, side))
            .unwrap_or(false)
    }

    pub fn try_insert_item(
        &mut self,
        side: Option<crate::redstone::Direction>,
        item: ItemStack,
    ) -> bool {
        let count = self.slot_count();
        if item.count == 0 || item.item == Item::Air {
            return false;
        }
        let max_stack = item.item.properties().max_stack;
        for i in 0..count {
            if !self.can_insert_item(i, &item, side) {
                continue;
            }
            if let Some(existing) = self.get_stack(i) {
                if existing.can_merge_with(&item) && existing.count < max_stack {
                    let mut updated = *existing;
                    updated.count += 1;
                    self.set_stack(i, Some(updated));
                    return true;
                }
            }
        }
        for i in 0..count {
            if !self.can_insert_item(i, &item, side) {
                continue;
            }
            if self.get_stack(i).is_none() {
                self.set_stack(i, Some(ItemStack { count: 1, ..item }));
                return true;
            }
        }
        false
    }

    pub fn try_extract_item(
        &mut self,
        side: Option<crate::redstone::Direction>,
    ) -> Option<ItemStack> {
        let count = self.slot_count();
        for i in 0..count {
            if !self.can_extract_item(i, side) {
                continue;
            }
            if let Some(existing) = self.get_stack(i) {
                if existing.count > 0 {
                    let extracted = ItemStack {
                        count: 1,
                        ..*existing
                    };
                    if existing.count == 1 {
                        self.set_stack(i, None);
                    } else {
                        let mut updated = *existing;
                        updated.count -= 1;
                        self.set_stack(i, Some(updated));
                    }
                    return Some(extracted);
                }
            }
        }
        None
    }

    pub fn revision(&self) -> u64 {
        match self {
            BlockEntity::Chest(c) => c.revision,
            BlockEntity::Furnace(f) => f.revision,
            BlockEntity::Hopper(h) => h.revision,
            BlockEntity::Dispenser(d) => d.revision,
            BlockEntity::Dropper(d) => d.revision,
            BlockEntity::Observer(o) => o.revision,
            _ => 0,
        }
    }

    /// Reconciles a replicated container with the host's monotonic revision.
    /// Local UI writes may temporarily mutate the mirror, but the next host
    /// snapshot must be able to restore the authoritative value exactly.
    pub fn set_revision(&mut self, revision: u64) {
        match self {
            BlockEntity::Chest(c) => c.revision = revision,
            BlockEntity::Furnace(f) => f.revision = revision,
            BlockEntity::Hopper(h) => h.revision = revision,
            BlockEntity::Dispenser(d) => d.revision = revision,
            BlockEntity::Dropper(d) => d.revision = revision,
            BlockEntity::Observer(o) => o.revision = revision,
            _ => {}
        }
    }

    pub fn mark_dirty(&mut self) {
        match self {
            BlockEntity::Chest(c) => c.revision = c.revision.wrapping_add(1),
            BlockEntity::Furnace(f) => f.revision = f.revision.wrapping_add(1),
            BlockEntity::Hopper(h) => h.revision = h.revision.wrapping_add(1),
            BlockEntity::Dispenser(d) => d.revision = d.revision.wrapping_add(1),
            BlockEntity::Dropper(d) => d.revision = d.revision.wrapping_add(1),
            BlockEntity::Observer(o) => o.revision = o.revision.wrapping_add(1),
            _ => {}
        }
    }

    pub fn memory_usage(&self) -> usize {
        let base = std::mem::size_of::<Self>();
        let extra = match self {
            BlockEntity::Chest(c) => c.custom_name.as_ref().map_or(0, |s| s.capacity()),
            BlockEntity::Furnace(f) => f.custom_name.as_ref().map_or(0, |s| s.capacity()),
            BlockEntity::Sign(s) => s.lines.iter().map(|l| l.capacity()).sum(),
            BlockEntity::Spawner(_) => 0,
            BlockEntity::Hopper(h) => h.custom_name.as_ref().map_or(0, |s| s.capacity()),
            BlockEntity::Dispenser(d) => d.custom_name.as_ref().map_or(0, |s| s.capacity()),
            BlockEntity::Dropper(d) => d.custom_name.as_ref().map_or(0, |s| s.capacity()),
            BlockEntity::Observer(_) => 0,
        };
        base + extra
    }

    /// Removes every item from a container while preserving the complete
    /// `ItemStack` metadata.  The returned vector is deterministic by slot
    /// order and is used by break/unload paths to avoid losing enchantments,
    /// potion data, durability, or custom names.
    pub fn drain_stacks(&mut self) -> Vec<ItemStack> {
        let mut drained = Vec::new();
        let slot_count = self.slot_count();
        for slot in 0..slot_count {
            if let Some(stack) = self.get_stack(slot).copied() {
                drained.push(stack);
                self.set_stack(slot, None);
            }
        }
        drained
    }
}

pub fn double_chest_partner(
    manager: &crate::chunk_manager::ChunkManager,
    pos: (i32, i32, i32),
) -> Option<(i32, i32, i32)> {
    let state_raw = manager.get_block_state(pos.0, pos.1, pos.2);
    let state = crate::world::BlockState::decode(state_raw);
    let (dx, dz) = match (state.facing, state.chest_type) {
        (crate::redstone::Direction::North, crate::world::ChestType::Left) => (-1, 0),
        (crate::redstone::Direction::North, crate::world::ChestType::Right) => (1, 0),
        (crate::redstone::Direction::East, crate::world::ChestType::Left) => (0, -1),
        (crate::redstone::Direction::East, crate::world::ChestType::Right) => (0, 1),
        (crate::redstone::Direction::South, crate::world::ChestType::Left) => (1, 0),
        (crate::redstone::Direction::South, crate::world::ChestType::Right) => (-1, 0),
        (crate::redstone::Direction::West, crate::world::ChestType::Left) => (0, 1),
        (crate::redstone::Direction::West, crate::world::ChestType::Right) => (0, -1),
        _ => return None,
    };
    let partner = (pos.0 + dx, pos.1, pos.2 + dz);
    let partner_block = manager.get_block(partner.0, partner.1, partner.2);
    if matches!(
        partner_block,
        crate::world::BlockType::Chest | crate::world::BlockType::EndCityChest
    ) {
        Some(partner)
    } else {
        None
    }
}

pub fn calculate_container_comparator_signal(
    manager: &crate::chunk_manager::ChunkManager,
    pos: (i32, i32, i32),
) -> u8 {
    let block_entity = match manager.get_block_entity(pos.0, pos.1, pos.2) {
        Some(be) => be,
        None => return 0,
    };

    let partner_pos = double_chest_partner(manager, pos);
    let partner_be = partner_pos.and_then(|p| manager.get_block_entity(p.0, p.1, p.2));

    let mut total_slots = block_entity.slot_count();
    let mut sum_fullness = 0.0f32;
    let mut total_items = 0u32;

    for i in 0..block_entity.slot_count() {
        if let Some(stack) = block_entity.get_stack(i) {
            total_items += stack.count as u32;
            let max_stack = stack.item.properties().max_stack as f32;
            if max_stack > 0.0 {
                sum_fullness += stack.count as f32 / max_stack;
            }
        }
    }

    if let Some(pbe) = partner_be {
        total_slots += pbe.slot_count();
        for i in 0..pbe.slot_count() {
            if let Some(stack) = pbe.get_stack(i) {
                total_items += stack.count as u32;
                let max_stack = stack.item.properties().max_stack as f32;
                if max_stack > 0.0 {
                    sum_fullness += stack.count as f32 / max_stack;
                }
            }
        }
    }

    if total_items == 0 || total_slots == 0 {
        return 0;
    }

    let signal = ((sum_fullness / total_slots as f32) * 14.0).floor() as u8 + 1;
    signal.min(15)
}

pub fn default_stub_for_block(block_type: BlockType) -> Option<BlockEntity> {
    match block_type {
        BlockType::Chest | BlockType::EndCityChest => Some(BlockEntity::Chest(ChestBlockEntity {
            custom_name: None,
            inventory: crate::inventory::ContainerInventory::new(),
            loot_table: None,
            loot_seed: None,
            revision: 0,
        })),
        BlockType::Furnace | BlockType::FurnaceLit => {
            Some(BlockEntity::Furnace(FurnaceBlockEntity::new()))
        }
        BlockType::OakSign => Some(BlockEntity::Sign(SignBlockEntity::new())),
        BlockType::Hopper => Some(BlockEntity::Hopper(HopperBlockEntity::new())),
        BlockType::Dispenser => Some(BlockEntity::Dispenser(DispenserBlockEntity::new())),
        BlockType::Dropper => Some(BlockEntity::Dropper(DropperBlockEntity::new())),
        BlockType::Observer => Some(BlockEntity::Observer(ObserverBlockEntity::new())),
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
            revision: 0,
        });
        assert!(chest.matches_block_type(BlockType::Chest));
        assert!(chest.matches_block_type(BlockType::EndCityChest));
        assert!(!chest.matches_block_type(BlockType::Furnace));
        assert!(!chest.matches_block_type(BlockType::Dirt));

        let mut furnace_state = FurnaceBlockEntity::new();
        furnace_state.set_stack(0, Some(ItemStack::new(Item::IronOre, 1)));
        furnace_state.set_stack(2, Some(ItemStack::new(Item::IronIngot, 1)));
        let furnace = BlockEntity::Furnace(furnace_state);
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

    #[test]
    fn test_container_capability_and_sided_rules() {
        let furnace = BlockEntity::Furnace(FurnaceBlockEntity::new());
        let iron_ore = ItemStack::new(Item::IronOre, 1);
        let coal = ItemStack::new(Item::Coal, 1);

        // Furnace sided insertion
        assert!(furnace.can_insert_item(0, &iron_ore, Some(crate::redstone::Direction::Up)));
        assert!(!furnace.can_insert_item(0, &iron_ore, Some(crate::redstone::Direction::Down)));
        assert!(furnace.can_insert_item(1, &coal, Some(crate::redstone::Direction::North)));
        assert!(!furnace.can_insert_item(0, &iron_ore, Some(crate::redstone::Direction::North)));
        assert!(furnace.can_extract_item(0, None));
        assert!(!furnace.can_extract_item(0, Some(crate::redstone::Direction::Down)));
        assert!(furnace.can_extract_item(2, Some(crate::redstone::Direction::Down)));

        let hopper = BlockEntity::Hopper(HopperBlockEntity::new());
        assert_eq!(hopper.slot_count(), 5);
        assert!(hopper.can_insert_item(0, &iron_ore, None));
    }

    #[test]
    fn automation_transfer_preserves_itemstack_metadata() {
        use crate::brewing::{PotionData, PotionKind};
        use crate::enchantment::{Enchantment, ItemName};

        let mut stack = ItemStack::new(Item::SplashPotion, 2);
        stack.durability = 17;
        stack.enchantments.add_or_upgrade(Enchantment::Power(3));
        stack.potion = Some(PotionData {
            kind: PotionKind::Strength,
            level: 2,
            duration_seconds: 90,
            splash: true,
        });
        let mut custom_name = ItemName::default();
        custom_name.set("automation payload");
        stack.custom_name = custom_name;

        let mut source = BlockEntity::Hopper(HopperBlockEntity::new());
        source.set_stack(0, Some(stack));
        let extracted = source.try_extract_item(Some(crate::redstone::Direction::Down));
        assert_eq!(extracted, Some(ItemStack { count: 1, ..stack }));

        let mut target = BlockEntity::Chest(ChestBlockEntity::new());
        assert!(target.try_insert_item(Some(crate::redstone::Direction::Up), extracted.unwrap()));
        assert_eq!(target.get_stack(0), Some(&ItemStack { count: 1, ..stack }));
        assert_eq!(source.get_stack(0), Some(&ItemStack { count: 1, ..stack }));
    }

    #[test]
    fn dispenser_and_dropper_use_nine_slot_deterministic_payload_selection() {
        let payload = ItemStack::new(Item::Arrow, 3);
        let mut dispenser = BlockEntity::Dispenser(DispenserBlockEntity::new());
        dispenser.set_stack(2, Some(payload));
        assert_eq!(dispenser.slot_count(), 9);
        assert_eq!(dispenser.select_random_non_empty_slot(0), Some(2));
        assert_eq!(dispenser.select_random_non_empty_slot(17), Some(2));

        let mut dropper = BlockEntity::Dropper(DropperBlockEntity::new());
        assert!(dropper.try_insert_item(None, ItemStack::new(Item::Diamond, 1)));
        assert_eq!(dropper.slot_count(), 9);
        assert_eq!(
            dropper.get_stack(0),
            Some(&ItemStack::new(Item::Diamond, 1))
        );
    }

    #[test]
    fn test_comparator_fullness_signal_computation() {
        let mut manager = crate::chunk_manager::ChunkManager::new(8);
        manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));
        manager.set_block(0, 64, 0, BlockType::Chest);
        manager.set_block_entity(0, 64, 0, Some(BlockEntity::Chest(ChestBlockEntity::new())));
        assert_eq!(
            calculate_container_comparator_signal(&manager, (0, 64, 0)),
            0
        );
        if let Some(be) = manager.get_block_entity_mut(0, 64, 0) {
            be.set_stack(0, Some(ItemStack::new(Item::Redstone, 64))); // 1 full stack out of 27
        }
        let signal = calculate_container_comparator_signal(&manager, (0, 64, 0));
        assert_eq!(signal, 1);

        // Fill all 27 slots with 64 items
        if let Some(be) = manager.get_block_entity_mut(0, 64, 0) {
            for slot in 0..27 {
                be.set_stack(slot, Some(ItemStack::new(Item::Redstone, 64)));
            }
        }
        let signal_full = calculate_container_comparator_signal(&manager, (0, 64, 0));
        assert_eq!(signal_full, 15);

        // A double chest contributes both physical inventories to one
        // comparator signal, including the partner across the chunk-local z
        // edge selected by its facing/type state.
        manager.set_block(0, 64, 2, BlockType::Chest);
        manager.set_block(0, 64, 3, BlockType::Chest);
        manager.set_block_state(
            0,
            64,
            2,
            crate::world::BlockState {
                facing: crate::redstone::Direction::East,
                chest_type: crate::world::ChestType::Right,
                ..Default::default()
            }
            .encode(),
        );
        manager.set_block_entity(0, 64, 2, Some(BlockEntity::Chest(ChestBlockEntity::new())));
        let mut partner = ChestBlockEntity::new();
        partner.set_stack(0, Some(ItemStack::new(Item::Redstone, 64)));
        manager.set_block_entity(0, 64, 3, Some(BlockEntity::Chest(partner)));
        assert_eq!(
            calculate_container_comparator_signal(&manager, (0, 64, 2)),
            1
        );
    }

    #[test]
    fn observer_defaults_are_backward_compatible() {
        let observer = ObserverBlockEntity::new();
        assert_eq!(observer.facing, crate::redstone::Direction::North);
        assert!(!observer.baseline_initialized);
        assert_eq!(observer.pending_pulse, 0);

        let bytes = bincode::serialize(&observer).unwrap();
        let restored: ObserverBlockEntity = bincode::deserialize(&bytes).unwrap();
        assert_eq!(restored, observer);
    }
}
