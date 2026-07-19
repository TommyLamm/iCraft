use crate::world::BlockType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    Creative,
    Survival,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    Air,
    // Blocks
    Grass, Dirt, Stone, Sand, Gravel, OakLog, OakPlanks, OakLeaves, Cobblestone, Bedrock, Water,
    CoalOre, IronOre, GoldOre, DiamondOre, RedstoneOre, Glass, Brick, StoneBrick, Snow, Ice, Clay,
    Sandstone, Obsidian, CraftingTable, Furnace, Chest, TNT, Bookshelf, Torch, Lava,
    
    // Tools
    StoneSword, StonePickaxe, StoneAxe, StoneShovel,
    IronSword, IronPickaxe, IronAxe, IronShovel,
    DiamondSword, DiamondPickaxe, DiamondAxe, DiamondShovel,
    
    // Resources
    Stick, Coal, IronIngot, GoldIngot, Diamond, Redstone,

    // Food
    Apple, Bread,
    
    // Mob Drops
    RottenFlesh,
    Bone,
    Bow,
    Gunpowder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolType {
    None,
    Pickaxe,
    Axe,
    Shovel,
    Sword,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolMaterial {
    Wood,
    Stone,
    Iron,
    Gold,
    Diamond,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolProperties {
    pub tool_type: ToolType,
    pub material: ToolMaterial,
    pub mining_speed: f32,
    pub durability: u32,
    pub damage: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemStack {
    pub item: Item,
    pub count: u32,
    pub durability: u32,
}

impl ItemStack {
    pub fn new(item: Item, count: u32) -> Self {
        let durability = item.tool_properties().map(|t| t.durability).unwrap_or(0);
        Self { item, count, durability }
    }
}

pub struct ItemProperties {
    pub name: &'static str,
    pub max_stack: u32,
    pub is_block: bool,
    pub block_type: Option<BlockType>,
    pub tex_coords: (u32, u32), // (col, row) in texture atlas
}

impl Item {
    pub fn tool_properties(self) -> Option<ToolProperties> {
        match self {
            Item::StoneSword => Some(ToolProperties { tool_type: ToolType::Sword, material: ToolMaterial::Stone, mining_speed: 4.0, durability: 131, damage: 5.0 }),
            Item::StonePickaxe => Some(ToolProperties { tool_type: ToolType::Pickaxe, material: ToolMaterial::Stone, mining_speed: 4.0, durability: 131, damage: 3.0 }),
            Item::StoneAxe => Some(ToolProperties { tool_type: ToolType::Axe, material: ToolMaterial::Stone, mining_speed: 4.0, durability: 131, damage: 4.0 }),
            Item::StoneShovel => Some(ToolProperties { tool_type: ToolType::Shovel, material: ToolMaterial::Stone, mining_speed: 4.0, durability: 131, damage: 2.0 }),

            Item::IronSword => Some(ToolProperties { tool_type: ToolType::Sword, material: ToolMaterial::Iron, mining_speed: 6.0, durability: 250, damage: 6.0 }),
            Item::IronPickaxe => Some(ToolProperties { tool_type: ToolType::Pickaxe, material: ToolMaterial::Iron, mining_speed: 6.0, durability: 250, damage: 4.0 }),
            Item::IronAxe => Some(ToolProperties { tool_type: ToolType::Axe, material: ToolMaterial::Iron, mining_speed: 6.0, durability: 250, damage: 5.0 }),
            Item::IronShovel => Some(ToolProperties { tool_type: ToolType::Shovel, material: ToolMaterial::Iron, mining_speed: 6.0, durability: 250, damage: 3.0 }),

            Item::DiamondSword => Some(ToolProperties { tool_type: ToolType::Sword, material: ToolMaterial::Diamond, mining_speed: 8.0, durability: 1561, damage: 7.0 }),
            Item::DiamondPickaxe => Some(ToolProperties { tool_type: ToolType::Pickaxe, material: ToolMaterial::Diamond, mining_speed: 8.0, durability: 1561, damage: 5.0 }),
            Item::DiamondAxe => Some(ToolProperties { tool_type: ToolType::Axe, material: ToolMaterial::Diamond, mining_speed: 8.0, durability: 1561, damage: 6.0 }),
            Item::DiamondShovel => Some(ToolProperties { tool_type: ToolType::Shovel, material: ToolMaterial::Diamond, mining_speed: 8.0, durability: 1561, damage: 4.0 }),

            _ => None,
        }
    }

    pub fn properties(self) -> ItemProperties {
        match self {
            Item::Air => ItemProperties { name: "Air", max_stack: 64, is_block: false, block_type: None, tex_coords: (0, 0) },
            Item::Grass => ItemProperties { name: "Grass Block", max_stack: 64, is_block: true, block_type: Some(BlockType::Grass), tex_coords: (1, 0) },
            Item::Dirt => ItemProperties { name: "Dirt", max_stack: 64, is_block: true, block_type: Some(BlockType::Dirt), tex_coords: (2, 0) },
            Item::Stone => ItemProperties { name: "Stone", max_stack: 64, is_block: true, block_type: Some(BlockType::Stone), tex_coords: (3, 0) },
            Item::Sand => ItemProperties { name: "Sand", max_stack: 64, is_block: true, block_type: Some(BlockType::Sand), tex_coords: (4, 0) },
            Item::Gravel => ItemProperties { name: "Gravel", max_stack: 64, is_block: true, block_type: Some(BlockType::Gravel), tex_coords: (5, 0) },
            Item::OakLog => ItemProperties { name: "Oak Log", max_stack: 64, is_block: true, block_type: Some(BlockType::OakLog), tex_coords: (11, 1) },
            Item::OakPlanks => ItemProperties { name: "Oak Planks", max_stack: 64, is_block: true, block_type: Some(BlockType::OakPlanks), tex_coords: (6, 0) },
            Item::OakLeaves => ItemProperties { name: "Oak Leaves", max_stack: 64, is_block: true, block_type: Some(BlockType::OakLeaves), tex_coords: (7, 0) },
            Item::Cobblestone => ItemProperties { name: "Cobblestone", max_stack: 64, is_block: true, block_type: Some(BlockType::Cobblestone), tex_coords: (8, 0) },
            Item::Bedrock => ItemProperties { name: "Bedrock", max_stack: 64, is_block: true, block_type: Some(BlockType::Bedrock), tex_coords: (9, 0) },
            Item::Water => ItemProperties { name: "Water", max_stack: 64, is_block: true, block_type: Some(BlockType::Water), tex_coords: (10, 0) },
            Item::CoalOre => ItemProperties { name: "Coal Ore", max_stack: 64, is_block: true, block_type: Some(BlockType::CoalOre), tex_coords: (11, 0) },
            Item::IronOre => ItemProperties { name: "Iron Ore", max_stack: 64, is_block: true, block_type: Some(BlockType::IronOre), tex_coords: (12, 0) },
            Item::GoldOre => ItemProperties { name: "Gold Ore", max_stack: 64, is_block: true, block_type: Some(BlockType::GoldOre), tex_coords: (13, 0) },
            Item::DiamondOre => ItemProperties { name: "Diamond Ore", max_stack: 64, is_block: true, block_type: Some(BlockType::DiamondOre), tex_coords: (14, 0) },
            Item::RedstoneOre => ItemProperties { name: "Redstone Ore", max_stack: 64, is_block: true, block_type: Some(BlockType::RedstoneOre), tex_coords: (15, 0) },
            Item::Glass => ItemProperties { name: "Glass", max_stack: 64, is_block: true, block_type: Some(BlockType::Glass), tex_coords: (0, 1) },
            Item::Brick => ItemProperties { name: "Brick Block", max_stack: 64, is_block: true, block_type: Some(BlockType::Brick), tex_coords: (1, 1) },
            Item::StoneBrick => ItemProperties { name: "Stone Brick", max_stack: 64, is_block: true, block_type: Some(BlockType::StoneBrick), tex_coords: (2, 1) },
            Item::Snow => ItemProperties { name: "Snow Block", max_stack: 64, is_block: true, block_type: Some(BlockType::Snow), tex_coords: (4, 1) },
            Item::Ice => ItemProperties { name: "Ice", max_stack: 64, is_block: true, block_type: Some(BlockType::Ice), tex_coords: (5, 1) },
            Item::Clay => ItemProperties { name: "Clay Block", max_stack: 64, is_block: true, block_type: Some(BlockType::Clay), tex_coords: (6, 1) },
            Item::Sandstone => ItemProperties { name: "Sandstone", max_stack: 64, is_block: true, block_type: Some(BlockType::Sandstone), tex_coords: (8, 1) },
            Item::Obsidian => ItemProperties { name: "Obsidian", max_stack: 64, is_block: true, block_type: Some(BlockType::Obsidian), tex_coords: (9, 1) },
            Item::CraftingTable => ItemProperties { name: "Crafting Table", max_stack: 64, is_block: true, block_type: Some(BlockType::CraftingTable), tex_coords: (13, 1) },
            Item::Furnace => ItemProperties { name: "Furnace", max_stack: 64, is_block: true, block_type: Some(BlockType::Furnace), tex_coords: (14, 1) },
            Item::Chest => ItemProperties { name: "Chest", max_stack: 64, is_block: true, block_type: Some(BlockType::Chest), tex_coords: (15, 1) },
            Item::TNT => ItemProperties { name: "TNT", max_stack: 64, is_block: true, block_type: Some(BlockType::TNT), tex_coords: (2, 2) },
            Item::Bookshelf => ItemProperties { name: "Bookshelf", max_stack: 64, is_block: true, block_type: Some(BlockType::Bookshelf), tex_coords: (3, 2) },
            Item::Torch => ItemProperties { name: "Torch", max_stack: 64, is_block: true, block_type: Some(BlockType::Torch), tex_coords: (4, 2) },
            Item::Lava => ItemProperties { name: "Lava", max_stack: 64, is_block: true, block_type: Some(BlockType::Lava), tex_coords: (15, 2) },
            
            // Tools (row 4-7)
            Item::StoneSword => ItemProperties { name: "Stone Sword", max_stack: 1, is_block: false, block_type: None, tex_coords: (0, 4) },
            Item::IronSword => ItemProperties { name: "Iron Sword", max_stack: 1, is_block: false, block_type: None, tex_coords: (1, 4) },
            Item::DiamondSword => ItemProperties { name: "Diamond Sword", max_stack: 1, is_block: false, block_type: None, tex_coords: (2, 4) },
            Item::StonePickaxe => ItemProperties { name: "Stone Pickaxe", max_stack: 1, is_block: false, block_type: None, tex_coords: (0, 5) },
            Item::IronPickaxe => ItemProperties { name: "Iron Pickaxe", max_stack: 1, is_block: false, block_type: None, tex_coords: (1, 5) },
            Item::DiamondPickaxe => ItemProperties { name: "Diamond Pickaxe", max_stack: 1, is_block: false, block_type: None, tex_coords: (2, 5) },
            Item::StoneAxe => ItemProperties { name: "Stone Axe", max_stack: 1, is_block: false, block_type: None, tex_coords: (0, 6) },
            Item::IronAxe => ItemProperties { name: "Iron Axe", max_stack: 1, is_block: false, block_type: None, tex_coords: (1, 6) },
            Item::DiamondAxe => ItemProperties { name: "Diamond Axe", max_stack: 1, is_block: false, block_type: None, tex_coords: (2, 6) },
            Item::StoneShovel => ItemProperties { name: "Stone Shovel", max_stack: 1, is_block: false, block_type: None, tex_coords: (0, 7) },
            Item::IronShovel => ItemProperties { name: "Iron Shovel", max_stack: 1, is_block: false, block_type: None, tex_coords: (1, 7) },
            Item::DiamondShovel => ItemProperties { name: "Diamond Shovel", max_stack: 1, is_block: false, block_type: None, tex_coords: (2, 7) },
            
            // Resources (row 3)
            Item::Stick => ItemProperties { name: "Stick", max_stack: 64, is_block: false, block_type: None, tex_coords: (0, 3) },
            Item::Coal => ItemProperties { name: "Coal", max_stack: 64, is_block: false, block_type: None, tex_coords: (1, 3) },
            Item::IronIngot => ItemProperties { name: "Iron Ingot", max_stack: 64, is_block: false, block_type: None, tex_coords: (2, 3) },
            Item::GoldIngot => ItemProperties { name: "Gold Ingot", max_stack: 64, is_block: false, block_type: None, tex_coords: (3, 3) },
            Item::Diamond => ItemProperties { name: "Diamond", max_stack: 64, is_block: false, block_type: None, tex_coords: (4, 3) },
            Item::Redstone => ItemProperties { name: "Redstone Dust", max_stack: 64, is_block: false, block_type: None, tex_coords: (5, 3) },
            Item::Apple => ItemProperties { name: "Apple", max_stack: 64, is_block: false, block_type: None, tex_coords: (6, 3) },
            Item::Bread => ItemProperties { name: "Bread", max_stack: 64, is_block: false, block_type: None, tex_coords: (7, 3) },
            
            // Mob Drops on Row 3, Cols 8..11
            Item::RottenFlesh => ItemProperties { name: "Rotten Flesh", max_stack: 64, is_block: false, block_type: None, tex_coords: (8, 3) },
            Item::Bone => ItemProperties { name: "Bone", max_stack: 64, is_block: false, block_type: None, tex_coords: (9, 3) },
            Item::Bow => ItemProperties { name: "Bow", max_stack: 1, is_block: false, block_type: None, tex_coords: (10, 3) },
            Item::Gunpowder => ItemProperties { name: "Gunpowder", max_stack: 64, is_block: false, block_type: None, tex_coords: (11, 3) },
        }
    }

    pub fn from_block(b: BlockType) -> Self {
        match b {
            BlockType::Air => Item::Air,
            BlockType::Grass => Item::Grass,
            BlockType::Dirt => Item::Dirt,
            BlockType::Stone => Item::Stone,
            BlockType::Sand => Item::Sand,
            BlockType::Gravel => Item::Gravel,
            BlockType::OakLog => Item::OakLog,
            BlockType::OakPlanks => Item::OakPlanks,
            BlockType::OakLeaves => Item::OakLeaves,
            BlockType::Cobblestone => Item::Cobblestone,
            BlockType::Bedrock => Item::Bedrock,
            BlockType::Water => Item::Water,
            BlockType::CoalOre => Item::CoalOre,
            BlockType::IronOre => Item::IronOre,
            BlockType::GoldOre => Item::GoldOre,
            BlockType::DiamondOre => Item::DiamondOre,
            BlockType::RedstoneOre => Item::RedstoneOre,
            BlockType::Glass => Item::Glass,
            BlockType::Brick => Item::Brick,
            BlockType::StoneBrick => Item::StoneBrick,
            BlockType::Snow => Item::Snow,
            BlockType::Ice => Item::Ice,
            BlockType::Clay => Item::Clay,
            BlockType::Sandstone => Item::Sandstone,
            BlockType::Obsidian => Item::Obsidian,
            BlockType::CraftingTable => Item::CraftingTable,
            BlockType::Furnace => Item::Furnace,
            BlockType::Chest => Item::Chest,
            BlockType::TNT => Item::TNT,
            BlockType::Bookshelf => Item::Bookshelf,
            BlockType::Torch => Item::Torch,
            BlockType::Lava => Item::Lava,
        }
    }
}

pub struct Inventory {
    pub hotbar: [Option<ItemStack>; 9],
    pub main: [Option<ItemStack>; 27],
    pub armor: [Option<ItemStack>; 4],
    pub craft_input: Vec<Option<ItemStack>>, // 4 slots for 2x2, 9 slots for 3x3
    pub craft_output: Option<ItemStack>,
    pub dragged: Option<ItemStack>,
    pub selected: usize, // Selected hotbar slot: 0..8
    pub is_open: bool,
    pub is_table_open: bool,
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            hotbar: [None; 9],
            main: [None; 27],
            armor: [None; 4],
            craft_input: vec![None; 4],
            craft_output: None,
            dragged: None,
            selected: 0,
            is_open: false,
            is_table_open: false,
        }
    }

    pub fn new_creative() -> Self {
        let mut inv = Self::new();
        let creative_items = [
            Item::Grass, Item::Dirt, Item::Stone, Item::OakLog, Item::OakPlanks,
            Item::Glass, Item::Cobblestone, Item::Water, Item::Torch,
        ];
        for (i, &item) in creative_items.iter().enumerate() {
            inv.hotbar[i] = Some(ItemStack::new(item, 64));
        }
        
        let extra_items = [
            Item::StoneSword, Item::IronPickaxe, Item::DiamondPickaxe, Item::DiamondAxe, Item::DiamondShovel,
            Item::Apple, Item::Bread, Item::Coal, Item::IronIngot, Item::Diamond, Item::Redstone,
            Item::CraftingTable, Item::Furnace, Item::Chest, Item::TNT, Item::Bookshelf, Item::Sand, Item::Lava,
        ];
        for (i, &item) in extra_items.iter().enumerate() {
            inv.main[i] = Some(ItemStack::new(item, 64));
        }
        inv
    }

    pub fn clear(&mut self) {
        self.hotbar = [None; 9];
        self.main = [None; 27];
        self.armor = [None; 4];
        self.craft_input.fill(None);
        self.craft_output = None;
        self.dragged = None;
    }

    pub fn get_selected_block(&self) -> Option<BlockType> {
        self.hotbar[self.selected].and_then(|stack| stack.item.properties().block_type)
    }

    pub fn add_item(&mut self, item: Item) -> bool {
        if item == Item::Air { return false; }
        let max_stack = item.properties().max_stack;
        
        // 1. Try to add to existing stack in hotbar
        for slot in self.hotbar.iter_mut() {
            if let Some(stack) = slot {
                if stack.item == item && stack.count < max_stack {
                    stack.count += 1;
                    return true;
                }
            }
        }
        // 2. Try to add to existing stack in main backpack
        for slot in self.main.iter_mut() {
            if let Some(stack) = slot {
                if stack.item == item && stack.count < max_stack {
                    stack.count += 1;
                    return true;
                }
            }
        }
        // 3. Try to add to empty slot in hotbar
        for slot in self.hotbar.iter_mut() {
            if slot.is_none() {
                *slot = Some(ItemStack::new(item, 1));
                return true;
            }
        }
        // 4. Try to add to empty slot in main backpack
        for slot in self.main.iter_mut() {
            if slot.is_none() {
                *slot = Some(ItemStack::new(item, 1));
                return true;
            }
        }
        false
    }

    pub fn use_selected_item(&mut self, is_creative: bool) {
        if is_creative { return; }
        if let Some(stack) = &mut self.hotbar[self.selected] {
            if stack.count > 1 {
                stack.count -= 1;
            } else {
                self.hotbar[self.selected] = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inventory_creative_init() {
        let inv = Inventory::new_creative();
        assert_eq!(inv.selected, 0);
        assert_eq!(inv.get_selected_block(), Some(BlockType::Grass));
        assert_eq!(inv.hotbar[0].unwrap().count, 64);
    }

    #[test]
    fn test_inventory_add_item() {
        let mut inv = Inventory::new();
        assert!(inv.add_item(Item::Stone));
        assert_eq!(inv.hotbar[0].unwrap().item, Item::Stone);
        assert_eq!(inv.hotbar[0].unwrap().count, 1);

        assert!(inv.add_item(Item::Stone));
        assert_eq!(inv.hotbar[0].unwrap().count, 2);
    }

    #[test]
    fn test_item_properties() {
        let pick = ItemStack::new(Item::StonePickaxe, 1);
        assert_eq!(pick.durability, 131);
        let grass = ItemStack::new(Item::Grass, 64);
        assert_eq!(grass.durability, 0);
    }

    #[test]
    fn test_new_mob_items() {
        let flesh = Item::RottenFlesh;
        let prop = flesh.properties();
        assert_eq!(prop.name, "Rotten Flesh");
        assert_eq!(prop.tex_coords, (8, 3));
    }
}
