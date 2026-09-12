//! Static per-discriminant block definition table (Plan 19).
//! Indexed by BlockType as usize. Values are byte-identical to the former match arms.

use crate::inventory::{ToolMaterial, ToolType};

use super::{
    BlockProperties, BlockState, BlockSupportStatus, BlockType, RenderType, SoundMaterial,
    BLOCK_STATE_OPEN_BIT,
};
use crate::redstone::Direction;

/// Number of BlockType variants (Air..=Observer).
pub const BLOCK_TYPE_COUNT: usize = BlockType::Observer as usize + 1;

/// Static definition for one BlockType discriminant.
#[derive(Debug, Clone, Copy)]
pub struct BlockDef {
    pub properties: BlockProperties,
    pub face_tex: [(u32, u32); 6],
    pub sound: Option<SoundMaterial>,
    pub preferred_tool: ToolType,
    pub min_harvest: Option<ToolMaterial>,
    pub is_cross_model: bool,
}

pub static BLOCK_TABLE: [BlockDef; BLOCK_TYPE_COUNT] = [
    // Air = 0
    BlockDef {
        properties: BlockProperties {
            name: "Air",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(0, 0), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)],
        sound: None,
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Grass = 1
    BlockDef {
        properties: BlockProperties {
            name: "Grass Block",
            hardness: 0.6f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(1, 0), (1, 0), (1, 0), (1, 0), (0, 0), (2, 0)],
        sound: Some(SoundMaterial::Grass),
        preferred_tool: ToolType::Shovel,
        min_harvest: None,
        is_cross_model: false,
    },
    // Dirt = 2
    BlockDef {
        properties: BlockProperties {
            name: "Dirt",
            hardness: 0.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(2, 0), (2, 0), (2, 0), (2, 0), (2, 0), (2, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Shovel,
        min_harvest: None,
        is_cross_model: false,
    },
    // Stone = 3
    BlockDef {
        properties: BlockProperties {
            name: "Stone",
            hardness: 1.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(3, 0), (3, 0), (3, 0), (3, 0), (3, 0), (3, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Wood),
        is_cross_model: false,
    },
    // Sand = 4
    BlockDef {
        properties: BlockProperties {
            name: "Sand",
            hardness: 0.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(4, 0), (4, 0), (4, 0), (4, 0), (4, 0), (4, 0)],
        sound: Some(SoundMaterial::Sand),
        preferred_tool: ToolType::Shovel,
        min_harvest: None,
        is_cross_model: false,
    },
    // Gravel = 5
    BlockDef {
        properties: BlockProperties {
            name: "Gravel",
            hardness: 0.6f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(5, 0), (5, 0), (5, 0), (5, 0), (5, 0), (5, 0)],
        sound: Some(SoundMaterial::Gravel),
        preferred_tool: ToolType::Shovel,
        min_harvest: None,
        is_cross_model: false,
    },
    // OakLog = 6
    BlockDef {
        properties: BlockProperties {
            name: "Oak Log",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(11, 1), (11, 1), (11, 1), (11, 1), (10, 1), (10, 1)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Axe,
        min_harvest: None,
        is_cross_model: false,
    },
    // OakPlanks = 7
    BlockDef {
        properties: BlockProperties {
            name: "Oak Planks",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(6, 0), (6, 0), (6, 0), (6, 0), (6, 0), (6, 0)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Axe,
        min_harvest: None,
        is_cross_model: false,
    },
    // OakLeaves = 8
    BlockDef {
        properties: BlockProperties {
            name: "Oak Leaves",
            hardness: 0.2f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(7, 0), (7, 0), (7, 0), (7, 0), (7, 0), (7, 0)],
        sound: Some(SoundMaterial::Grass),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Cobblestone = 9
    BlockDef {
        properties: BlockProperties {
            name: "Cobblestone",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(8, 0), (8, 0), (8, 0), (8, 0), (8, 0), (8, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Wood),
        is_cross_model: false,
    },
    // Bedrock = 10
    BlockDef {
        properties: BlockProperties {
            name: "Bedrock",
            hardness: -1.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(9, 0), (9, 0), (9, 0), (9, 0), (9, 0), (9, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Water = 11
    BlockDef {
        properties: BlockProperties {
            name: "Water",
            hardness: 100.0f32,
            render_type: RenderType::Translucent,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(10, 0), (10, 0), (10, 0), (10, 0), (10, 0), (10, 0)],
        sound: None,
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // CoalOre = 12
    BlockDef {
        properties: BlockProperties {
            name: "Coal Ore",
            hardness: 3.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(11, 0), (11, 0), (11, 0), (11, 0), (11, 0), (11, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Wood),
        is_cross_model: false,
    },
    // IronOre = 13
    BlockDef {
        properties: BlockProperties {
            name: "Iron Ore",
            hardness: 3.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(12, 0), (12, 0), (12, 0), (12, 0), (12, 0), (12, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Stone),
        is_cross_model: false,
    },
    // GoldOre = 14
    BlockDef {
        properties: BlockProperties {
            name: "Gold Ore",
            hardness: 3.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(13, 0), (13, 0), (13, 0), (13, 0), (13, 0), (13, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Iron),
        is_cross_model: false,
    },
    // DiamondOre = 15
    BlockDef {
        properties: BlockProperties {
            name: "Diamond Ore",
            hardness: 3.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(14, 0), (14, 0), (14, 0), (14, 0), (14, 0), (14, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Iron),
        is_cross_model: false,
    },
    // RedstoneOre = 16
    BlockDef {
        properties: BlockProperties {
            name: "Redstone Ore",
            hardness: 3.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(15, 0), (15, 0), (15, 0), (15, 0), (15, 0), (15, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Iron),
        is_cross_model: false,
    },
    // Glass = 17
    BlockDef {
        properties: BlockProperties {
            name: "Glass",
            hardness: 0.3f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (0, 1)],
        sound: Some(SoundMaterial::Glass),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Brick = 18
    BlockDef {
        properties: BlockProperties {
            name: "Brick",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(1, 1), (1, 1), (1, 1), (1, 1), (1, 1), (1, 1)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // StoneBrick = 19
    BlockDef {
        properties: BlockProperties {
            name: "Stone Brick",
            hardness: 1.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(2, 1), (2, 1), (2, 1), (2, 1), (2, 1), (2, 1)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Wood),
        is_cross_model: false,
    },
    // Snow = 20
    BlockDef {
        properties: BlockProperties {
            name: "Snow Block",
            hardness: 0.1f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(4, 1), (4, 1), (4, 1), (4, 1), (3, 1), (2, 0)],
        sound: Some(SoundMaterial::Snow),
        preferred_tool: ToolType::Shovel,
        min_harvest: None,
        is_cross_model: false,
    },
    // Ice = 21
    BlockDef {
        properties: BlockProperties {
            name: "Ice",
            hardness: 0.5f32,
            render_type: RenderType::Translucent,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(5, 1), (5, 1), (5, 1), (5, 1), (5, 1), (5, 1)],
        sound: Some(SoundMaterial::Ice),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Clay = 22
    BlockDef {
        properties: BlockProperties {
            name: "Clay",
            hardness: 0.6f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(6, 1), (6, 1), (6, 1), (6, 1), (6, 1), (6, 1)],
        sound: Some(SoundMaterial::Sand),
        preferred_tool: ToolType::Shovel,
        min_harvest: None,
        is_cross_model: false,
    },
    // Sandstone = 23
    BlockDef {
        properties: BlockProperties {
            name: "Sandstone",
            hardness: 0.8f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(8, 1), (8, 1), (8, 1), (8, 1), (7, 1), (7, 1)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Shovel,
        min_harvest: Some(ToolMaterial::Wood),
        is_cross_model: false,
    },
    // Obsidian = 24
    BlockDef {
        properties: BlockProperties {
            name: "Obsidian",
            hardness: 50.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(9, 1), (9, 1), (9, 1), (9, 1), (9, 1), (9, 1)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Diamond),
        is_cross_model: false,
    },
    // CraftingTable = 25
    BlockDef {
        properties: BlockProperties {
            name: "Crafting Table",
            hardness: 2.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(13, 1), (13, 1), (13, 1), (13, 1), (12, 1), (6, 0)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Axe,
        min_harvest: None,
        is_cross_model: false,
    },
    // Furnace = 26
    BlockDef {
        properties: BlockProperties {
            name: "Furnace",
            hardness: 3.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(14, 1), (3, 0), (3, 0), (3, 0), (3, 0), (3, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Wood),
        is_cross_model: false,
    },
    // Chest = 27
    BlockDef {
        properties: BlockProperties {
            name: "Chest",
            hardness: 2.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(15, 1), (15, 1), (15, 1), (15, 1), (15, 1), (15, 1)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Axe,
        min_harvest: None,
        is_cross_model: false,
    },
    // TNT = 28
    BlockDef {
        properties: BlockProperties {
            name: "TNT",
            hardness: 0.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(2, 2), (2, 2), (2, 2), (2, 2), (0, 2), (1, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Bookshelf = 29
    BlockDef {
        properties: BlockProperties {
            name: "Bookshelf",
            hardness: 1.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(3, 2), (3, 2), (3, 2), (3, 2), (6, 0), (6, 0)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Axe,
        min_harvest: None,
        is_cross_model: false,
    },
    // Torch = 30
    BlockDef {
        properties: BlockProperties {
            name: "Torch",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: false,
            light_emission: 14,
        },
        face_tex: [(4, 2), (4, 2), (4, 2), (4, 2), (4, 2), (4, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Lava = 31
    BlockDef {
        properties: BlockProperties {
            name: "Lava",
            hardness: 100.0f32,
            render_type: RenderType::Opaque,
            is_solid: false,
            is_passable: true,
            light_emission: 15,
        },
        face_tex: [(15, 2), (15, 2), (15, 2), (15, 2), (15, 2), (15, 2)],
        sound: None,
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // BirchLog = 32
    BlockDef {
        properties: BlockProperties {
            name: "Birch Log",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(1, 12), (1, 12), (1, 12), (1, 12), (0, 12), (0, 12)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Axe,
        min_harvest: None,
        is_cross_model: false,
    },
    // BirchPlanks = 33
    BlockDef {
        properties: BlockProperties {
            name: "Birch Planks",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(2, 12), (2, 12), (2, 12), (2, 12), (2, 12), (2, 12)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Axe,
        min_harvest: None,
        is_cross_model: false,
    },
    // BirchLeaves = 34
    BlockDef {
        properties: BlockProperties {
            name: "Birch Leaves",
            hardness: 0.2f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(3, 12), (3, 12), (3, 12), (3, 12), (3, 12), (3, 12)],
        sound: Some(SoundMaterial::Grass),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // SpruceLog = 35
    BlockDef {
        properties: BlockProperties {
            name: "Spruce Log",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(5, 12), (5, 12), (5, 12), (5, 12), (4, 12), (4, 12)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Axe,
        min_harvest: None,
        is_cross_model: false,
    },
    // SprucePlanks = 36
    BlockDef {
        properties: BlockProperties {
            name: "Spruce Planks",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(6, 12), (6, 12), (6, 12), (6, 12), (6, 12), (6, 12)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Axe,
        min_harvest: None,
        is_cross_model: false,
    },
    // SpruceLeaves = 37
    BlockDef {
        properties: BlockProperties {
            name: "Spruce Leaves",
            hardness: 0.2f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(7, 12), (7, 12), (7, 12), (7, 12), (7, 12), (7, 12)],
        sound: Some(SoundMaterial::Grass),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // TallGrass = 38
    BlockDef {
        properties: BlockProperties {
            name: "Tall Grass",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(8, 12), (8, 12), (8, 12), (8, 12), (8, 12), (8, 12)],
        sound: Some(SoundMaterial::Grass),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: true,
    },
    // Dandelion = 39
    BlockDef {
        properties: BlockProperties {
            name: "Dandelion",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(9, 12), (9, 12), (9, 12), (9, 12), (9, 12), (9, 12)],
        sound: Some(SoundMaterial::Grass),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: true,
    },
    // Poppy = 40
    BlockDef {
        properties: BlockProperties {
            name: "Poppy",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(10, 12), (10, 12), (10, 12), (10, 12), (10, 12), (10, 12)],
        sound: Some(SoundMaterial::Grass),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: true,
    },
    // Cactus = 41
    BlockDef {
        properties: BlockProperties {
            name: "Cactus",
            hardness: 0.4f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(11, 12), (11, 12), (11, 12), (11, 12), (11, 12), (11, 12)],
        sound: Some(SoundMaterial::Gravel),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // SugarCane = 42
    BlockDef {
        properties: BlockProperties {
            name: "Sugar Cane",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(12, 12), (12, 12), (12, 12), (12, 12), (12, 12), (12, 12)],
        sound: Some(SoundMaterial::Grass),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: true,
    },
    // Pumpkin = 43
    BlockDef {
        properties: BlockProperties {
            name: "Pumpkin",
            hardness: 1.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(13, 12), (13, 12), (13, 12), (13, 12), (13, 12), (13, 12)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Axe,
        min_harvest: None,
        is_cross_model: false,
    },
    // Melon = 44
    BlockDef {
        properties: BlockProperties {
            name: "Melon",
            hardness: 1.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(14, 12), (14, 12), (14, 12), (14, 12), (14, 12), (14, 12)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Axe,
        min_harvest: None,
        is_cross_model: false,
    },
    // EnchantingTable = 45
    BlockDef {
        properties: BlockProperties {
            name: "Enchanting Table",
            hardness: 5.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 7,
        },
        face_tex: [(0, 13), (0, 13), (0, 13), (0, 13), (0, 13), (0, 13)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Diamond),
        is_cross_model: false,
    },
    // BrewingStand = 46
    BlockDef {
        properties: BlockProperties {
            name: "Brewing Stand",
            hardness: 0.5f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 1,
        },
        face_tex: [(1, 13), (1, 13), (1, 13), (1, 13), (1, 13), (1, 13)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Stone),
        is_cross_model: false,
    },
    // Anvil = 47
    BlockDef {
        properties: BlockProperties {
            name: "Anvil",
            hardness: 5.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(2, 13), (2, 13), (2, 13), (2, 13), (2, 13), (2, 13)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Stone),
        is_cross_model: false,
    },
    // RedstoneWire = 48
    BlockDef {
        properties: BlockProperties {
            name: "Redstone Wire",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(5, 2), (5, 2), (5, 2), (5, 2), (5, 2), (5, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // RedstoneTorch = 49
    BlockDef {
        properties: BlockProperties {
            name: "Redstone Torch",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 7,
        },
        face_tex: [(6, 2), (6, 2), (6, 2), (6, 2), (6, 2), (6, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved50 = 50 (was RedstoneTorchOff)
    BlockDef {
        properties: BlockProperties {
            name: "Redstone Torch",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(6, 2), (6, 2), (6, 2), (6, 2), (6, 2), (6, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Repeater = 51
    BlockDef {
        properties: BlockProperties {
            name: "Redstone Repeater",
            hardness: 0.5f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(7, 2), (7, 2), (7, 2), (7, 2), (7, 2), (7, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved52 = 52 (was RepeaterPowered)
    BlockDef {
        properties: BlockProperties {
            name: "Redstone Repeater",
            hardness: 0.5f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(7, 2), (7, 2), (7, 2), (7, 2), (7, 2), (7, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Comparator = 53
    BlockDef {
        properties: BlockProperties {
            name: "Redstone Comparator",
            hardness: 0.5f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(8, 2), (8, 2), (8, 2), (8, 2), (8, 2), (8, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved54 = 54 (was ComparatorPowered)
    BlockDef {
        properties: BlockProperties {
            name: "Redstone Comparator",
            hardness: 0.5f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(8, 2), (8, 2), (8, 2), (8, 2), (8, 2), (8, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // StoneButton = 55
    BlockDef {
        properties: BlockProperties {
            name: "Stone Button",
            hardness: 0.5f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(9, 2), (9, 2), (9, 2), (9, 2), (9, 2), (9, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved56 = 56 (was StoneButtonPressed)
    BlockDef {
        properties: BlockProperties {
            name: "Stone Button",
            hardness: 0.5f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(9, 2), (9, 2), (9, 2), (9, 2), (9, 2), (9, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Lever = 57
    BlockDef {
        properties: BlockProperties {
            name: "Lever",
            hardness: 0.5f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(10, 2), (10, 2), (10, 2), (10, 2), (10, 2), (10, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved58 = 58 (was LeverOn)
    BlockDef {
        properties: BlockProperties {
            name: "Lever",
            hardness: 0.5f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(10, 2), (10, 2), (10, 2), (10, 2), (10, 2), (10, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // PressurePlate = 59
    BlockDef {
        properties: BlockProperties {
            name: "Stone Pressure Plate",
            hardness: 0.5f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(11, 2), (11, 2), (11, 2), (11, 2), (11, 2), (11, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved60 = 60 (was PressurePlatePowered)
    BlockDef {
        properties: BlockProperties {
            name: "Stone Pressure Plate",
            hardness: 0.5f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(11, 2), (11, 2), (11, 2), (11, 2), (11, 2), (11, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Piston = 61
    BlockDef {
        properties: BlockProperties {
            name: "Piston",
            hardness: 1.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(12, 2), (12, 2), (12, 2), (12, 2), (12, 2), (12, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved62 = 62 (was PistonExtended)
    BlockDef {
        properties: BlockProperties {
            name: "Piston",
            hardness: 1.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(12, 2), (12, 2), (12, 2), (12, 2), (12, 2), (12, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // StickyPiston = 63
    BlockDef {
        properties: BlockProperties {
            name: "Sticky Piston",
            hardness: 1.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(13, 2), (13, 2), (13, 2), (13, 2), (13, 2), (13, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved64 = 64 (was StickyPistonExtended)
    BlockDef {
        properties: BlockProperties {
            name: "Sticky Piston",
            hardness: 1.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(13, 2), (13, 2), (13, 2), (13, 2), (13, 2), (13, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // RedstoneLamp = 65
    BlockDef {
        properties: BlockProperties {
            name: "Redstone Lamp",
            hardness: 0.3f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(14, 2), (14, 2), (14, 2), (14, 2), (14, 2), (14, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved66 = 66 (was RedstoneLampLit)
    BlockDef {
        properties: BlockProperties {
            name: "Redstone Lamp",
            hardness: 0.3f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 15,
        },
        face_tex: [(8, 14), (8, 14), (8, 14), (8, 14), (8, 14), (8, 14)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // OakDoor = 67
    BlockDef {
        properties: BlockProperties {
            name: "Oak Door",
            hardness: 3.0f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(9, 14), (9, 14), (9, 14), (9, 14), (9, 14), (9, 14)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved68 = 68 (was OakDoorOpen)
    BlockDef {
        properties: BlockProperties {
            name: "Oak Door",
            hardness: 3.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(9, 14), (9, 14), (9, 14), (9, 14), (9, 14), (9, 14)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // OakTrapdoor = 69
    BlockDef {
        properties: BlockProperties {
            name: "Oak Trapdoor",
            hardness: 3.0f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(10, 14), (10, 14), (10, 14), (10, 14), (10, 14), (10, 14)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved70 = 70 (was OakTrapdoorOpen)
    BlockDef {
        properties: BlockProperties {
            name: "Oak Trapdoor",
            hardness: 3.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(10, 14), (10, 14), (10, 14), (10, 14), (10, 14), (10, 14)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Dispenser = 71
    BlockDef {
        properties: BlockProperties {
            name: "Dispenser",
            hardness: 3.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(11, 14), (11, 14), (11, 14), (11, 14), (11, 14), (11, 14)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Dropper = 72
    BlockDef {
        properties: BlockProperties {
            name: "Dropper",
            hardness: 3.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(12, 14), (12, 14), (12, 14), (12, 14), (12, 14), (12, 14)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // NoteBlock = 73
    BlockDef {
        properties: BlockProperties {
            name: "Note Block",
            hardness: 0.8f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(13, 14), (13, 14), (13, 14), (13, 14), (13, 14), (13, 14)],
        sound: Some(SoundMaterial::Wood),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Fire = 74
    BlockDef {
        properties: BlockProperties {
            name: "Fire",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 15,
        },
        face_tex: [(15, 12), (15, 12), (15, 12), (15, 12), (15, 12), (15, 12)],
        sound: None,
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // SnowLayer = 75
    BlockDef {
        properties: BlockProperties {
            name: "Snow Layer",
            hardness: 0.1f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(3, 1), (3, 1), (3, 1), (3, 1), (3, 1), (3, 1)],
        sound: Some(SoundMaterial::Snow),
        preferred_tool: ToolType::Shovel,
        min_harvest: None,
        is_cross_model: false,
    },
    // Netherrack = 76
    BlockDef {
        properties: BlockProperties {
            name: "Netherrack",
            hardness: 0.4f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(10, 15), (10, 15), (10, 15), (10, 15), (10, 15), (10, 15)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Stone),
        is_cross_model: false,
    },
    // SoulSand = 77
    BlockDef {
        properties: BlockProperties {
            name: "Soul Sand",
            hardness: 0.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(11, 15), (11, 15), (11, 15), (11, 15), (11, 15), (11, 15)],
        sound: Some(SoundMaterial::Sand),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Glowstone = 78
    BlockDef {
        properties: BlockProperties {
            name: "Glowstone",
            hardness: 0.3f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 15,
        },
        face_tex: [(12, 15), (12, 15), (12, 15), (12, 15), (12, 15), (12, 15)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Stone),
        is_cross_model: false,
    },
    // NetherPortal = 79
    BlockDef {
        properties: BlockProperties {
            name: "Nether Portal",
            hardness: -1.0f32,
            render_type: RenderType::Translucent,
            is_solid: false,
            is_passable: true,
            light_emission: 11,
        },
        face_tex: [(13, 15), (13, 15), (13, 15), (13, 15), (13, 15), (13, 15)],
        sound: None,
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // EndStone = 80
    BlockDef {
        properties: BlockProperties {
            name: "End Stone",
            hardness: 3.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(14, 15), (14, 15), (14, 15), (14, 15), (14, 15), (14, 15)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Stone),
        is_cross_model: false,
    },
    // EndPortalFrame = 81
    BlockDef {
        properties: BlockProperties {
            name: "End Portal Frame",
            hardness: -1.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(9, 4), (9, 4), (9, 4), (9, 4), (15, 15), (9, 4)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved82 = 82 (was EndPortalFrameFilled)
    BlockDef {
        properties: BlockProperties {
            name: "End Portal Frame",
            hardness: -1.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 2,
        },
        face_tex: [(9, 4), (9, 4), (9, 4), (9, 4), (6, 4), (9, 4)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: None,
        is_cross_model: false,
    },
    // EndPortal = 83
    BlockDef {
        properties: BlockProperties {
            name: "End Portal",
            hardness: -1.0f32,
            render_type: RenderType::Translucent,
            is_solid: false,
            is_passable: true,
            light_emission: 15,
        },
        face_tex: [(14, 10), (14, 10), (14, 10), (14, 10), (14, 10), (14, 10)],
        sound: None,
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Purpur = 84
    BlockDef {
        properties: BlockProperties {
            name: "Purpur Block",
            hardness: 1.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(15, 10), (15, 10), (15, 10), (15, 10), (15, 10), (15, 10)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Stone),
        is_cross_model: false,
    },
    // DragonEgg = 85
    BlockDef {
        properties: BlockProperties {
            name: "Dragon Egg",
            hardness: 3.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 1,
        },
        face_tex: [(14, 11), (14, 11), (14, 11), (14, 11), (14, 11), (14, 11)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: None,
        is_cross_model: false,
    },
    // WitherSkeletonSkull = 86
    BlockDef {
        properties: BlockProperties {
            name: "Wither Skeleton Skull",
            hardness: 1.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(15, 11), (15, 11), (15, 11), (15, 11), (15, 11), (15, 11)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // NetherBrick = 87
    BlockDef {
        properties: BlockProperties {
            name: "Nether Bricks",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(9, 10), (9, 10), (9, 10), (9, 10), (9, 10), (9, 10)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Stone),
        is_cross_model: false,
    },
    // EndCityChest = 88
    BlockDef {
        properties: BlockProperties {
            name: "End City Chest",
            hardness: 2.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 3,
        },
        face_tex: [(10, 10), (10, 10), (10, 10), (10, 10), (10, 10), (10, 10)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::Pickaxe,
        min_harvest: Some(ToolMaterial::Stone),
        is_cross_model: false,
    },
    // Bed = 89
    BlockDef {
        properties: BlockProperties {
            name: "Bed",
            hardness: 0.2f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(6, 0), (6, 0), (6, 0), (6, 0), (6, 0), (6, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Reserved90 = 90 (was FurnaceLit)
    BlockDef {
        properties: BlockProperties {
            name: "Furnace",
            hardness: 3.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 13,
        },
        face_tex: [(14, 1), (3, 0), (3, 0), (3, 0), (3, 0), (3, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Farmland = 91
    BlockDef {
        properties: BlockProperties {
            name: "Farmland",
            hardness: 0.6f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(2, 0), (2, 0), (2, 0), (2, 0), (6, 5), (2, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // WheatCrop = 92
    BlockDef {
        properties: BlockProperties {
            name: "Wheat Crop",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(8, 5), (8, 5), (8, 5), (8, 5), (8, 5), (8, 5)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: true,
    },
    // CarrotCrop = 93
    BlockDef {
        properties: BlockProperties {
            name: "Carrot Crop",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(0, 6), (0, 6), (0, 6), (0, 6), (0, 6), (0, 6)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: true,
    },
    // PotatoCrop = 94
    BlockDef {
        properties: BlockProperties {
            name: "Potato Crop",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(4, 6), (4, 6), (4, 6), (4, 6), (4, 6), (4, 6)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: true,
    },
    // OakSlab = 95
    BlockDef {
        properties: BlockProperties {
            name: "Oak Slab",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(6, 0), (6, 0), (6, 0), (6, 0), (6, 0), (6, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // CobblestoneSlab = 96
    BlockDef {
        properties: BlockProperties {
            name: "Cobblestone Slab",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(8, 0), (8, 0), (8, 0), (8, 0), (8, 0), (8, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // OakStair = 97
    BlockDef {
        properties: BlockProperties {
            name: "Oak Stairs",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(6, 0), (6, 0), (6, 0), (6, 0), (6, 0), (6, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // CobblestoneStair = 98
    BlockDef {
        properties: BlockProperties {
            name: "Cobblestone Stairs",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(8, 0), (8, 0), (8, 0), (8, 0), (8, 0), (8, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // OakFence = 99
    BlockDef {
        properties: BlockProperties {
            name: "Oak Fence",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(6, 0), (6, 0), (6, 0), (6, 0), (6, 0), (6, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // OakFenceGate = 100
    BlockDef {
        properties: BlockProperties {
            name: "Oak Fence Gate",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(6, 0), (6, 0), (6, 0), (6, 0), (6, 0), (6, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // CobblestoneWall = 101
    BlockDef {
        properties: BlockProperties {
            name: "Cobblestone Wall",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(8, 0), (8, 0), (8, 0), (8, 0), (8, 0), (8, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // GlassPane = 102
    BlockDef {
        properties: BlockProperties {
            name: "Glass Pane",
            hardness: 0.3f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (0, 1)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // OakLadder = 103
    BlockDef {
        properties: BlockProperties {
            name: "Ladder",
            hardness: 0.4f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(3, 5), (3, 5), (3, 5), (3, 5), (3, 5), (3, 5)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // OakSign = 104
    BlockDef {
        properties: BlockProperties {
            name: "Oak Sign",
            hardness: 1.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(6, 0), (6, 0), (6, 0), (6, 0), (6, 0), (6, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // OakSapling = 105
    BlockDef {
        properties: BlockProperties {
            name: "Sapling",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(4, 0), (4, 0), (4, 0), (4, 0), (4, 0), (4, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // BirchSapling = 106
    BlockDef {
        properties: BlockProperties {
            name: "Sapling",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(4, 0), (4, 0), (4, 0), (4, 0), (4, 0), (4, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // SpruceSapling = 107
    BlockDef {
        properties: BlockProperties {
            name: "Sapling",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(4, 0), (4, 0), (4, 0), (4, 0), (4, 0), (4, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Spawner = 108
    BlockDef {
        properties: BlockProperties {
            name: "Mob Spawner",
            hardness: 5.0f32,
            render_type: RenderType::Cutout,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(1, 4), (1, 4), (1, 4), (1, 4), (1, 4), (1, 4)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // MossyCobblestone = 109
    BlockDef {
        properties: BlockProperties {
            name: "Mossy Cobblestone",
            hardness: 2.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(4, 2), (4, 2), (4, 2), (4, 2), (4, 2), (4, 2)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // DirtPath = 110
    BlockDef {
        properties: BlockProperties {
            name: "Dirt Path",
            hardness: 0.6f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(2, 0), (2, 0), (2, 0), (2, 0), (6, 5), (2, 0)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // NetherWartCrop = 111
    BlockDef {
        properties: BlockProperties {
            name: "Nether Wart",
            hardness: 0.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(2, 6), (2, 6), (2, 6), (2, 6), (2, 6), (2, 6)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // EndStoneBrick = 112
    BlockDef {
        properties: BlockProperties {
            name: "End Stone Bricks",
            hardness: 3.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(15, 10), (15, 10), (15, 10), (15, 10), (15, 10), (15, 10)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // RespawnAnchor = 113
    BlockDef {
        properties: BlockProperties {
            name: "Respawn Anchor",
            hardness: 5.0f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 3,
        },
        face_tex: [(14, 11), (14, 11), (14, 11), (14, 11), (14, 11), (14, 11)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // EndGateway = 114
    BlockDef {
        properties: BlockProperties {
            name: "End Gateway",
            hardness: -1.0f32,
            render_type: RenderType::Translucent,
            is_solid: false,
            is_passable: true,
            light_emission: 15,
        },
        face_tex: [(14, 10), (14, 10), (14, 10), (14, 10), (14, 10), (14, 10)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Rail = 115
    BlockDef {
        properties: BlockProperties {
            name: "Rail",
            hardness: 0.7f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(0, 8), (0, 8), (0, 8), (0, 8), (0, 8), (0, 8)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // PoweredRail = 116
    BlockDef {
        properties: BlockProperties {
            name: "Powered Rail",
            hardness: 0.7f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(3, 8), (3, 8), (3, 8), (3, 8), (3, 8), (3, 8)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // DetectorRail = 117
    BlockDef {
        properties: BlockProperties {
            name: "Detector Rail",
            hardness: 0.7f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(3, 9), (3, 9), (3, 9), (3, 9), (3, 9), (3, 9)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // ActivatorRail = 118
    BlockDef {
        properties: BlockProperties {
            name: "Activator Rail",
            hardness: 0.7f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: true,
            light_emission: 0,
        },
        face_tex: [(3, 10), (3, 10), (3, 10), (3, 10), (3, 10), (3, 10)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Hopper = 119
    BlockDef {
        properties: BlockProperties {
            name: "Hopper",
            hardness: 3.0f32,
            render_type: RenderType::Cutout,
            is_solid: false,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(11, 15), (11, 15), (11, 15), (11, 15), (11, 15), (11, 15)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
    // Observer = 120
    BlockDef {
        properties: BlockProperties {
            name: "Observer",
            hardness: 3.5f32,
            render_type: RenderType::Opaque,
            is_solid: true,
            is_passable: false,
            light_emission: 0,
        },
        face_tex: [(11, 16), (11, 16), (11, 16), (11, 16), (11, 16), (11, 16)],
        sound: Some(SoundMaterial::Stone),
        preferred_tool: ToolType::None,
        min_harvest: None,
        is_cross_model: false,
    },
];

const _: () = assert!(BLOCK_TABLE.len() == BLOCK_TYPE_COUNT);

impl BlockType {
    pub fn from_u8(val: u8) -> Self {
        if val <= BlockType::Observer as u8 {
            let raw: Self = unsafe { std::mem::transmute(val) };
            raw.canonicalize()
        } else {
            BlockType::Air
        }
    }

    /// Wire encoding for multiplayer block sync.
    ///
    /// `BlockType` is `#[repr(u8)]` with explicit, stable discriminants, so the
    /// numeric value is part of the network protocol contract. Adding a new
    /// variant is allowed (append a new value), but never reuse an existing
    /// wire value for a different block: older clients would misdecode it.
    pub fn to_wire(&self) -> u32 {
        self.canonicalize() as u32
    }

    /// Inverse of `to_wire`. Reserved holes alias to their live base type.
    /// Returns `None` for values that do not map to a known discriminant.
    pub fn from_wire(val: u32) -> Option<Self> {
        if val > BlockType::Observer as u32 {
            return None;
        }
        let raw: Self = unsafe { std::mem::transmute(val as u8) };
        Some(raw.canonicalize())
    }

    /// Map a saved/wire `(block_id, state)` into the live pair.
    ///
    /// The 13 legacy powered/open/lit/extended/filled discriminants become the
    /// base type with `BLOCK_STATE_OPEN_BIT` set. Live base ids keep their state
    /// bytes unchanged (redstone torch lit = bit clear).
    pub fn migrate_saved(block_id: u8, state: u8) -> (Self, u8) {
        match block_id {
            50 => (Self::RedstoneTorch, state | BLOCK_STATE_OPEN_BIT),
            52 => (Self::Repeater, state | BLOCK_STATE_OPEN_BIT),
            54 => (Self::Comparator, state | BLOCK_STATE_OPEN_BIT),
            56 => (Self::StoneButton, state | BLOCK_STATE_OPEN_BIT),
            58 => (Self::Lever, state | BLOCK_STATE_OPEN_BIT),
            60 => (Self::PressurePlate, state | BLOCK_STATE_OPEN_BIT),
            62 => (Self::Piston, state | BLOCK_STATE_OPEN_BIT),
            64 => (Self::StickyPiston, state | BLOCK_STATE_OPEN_BIT),
            66 => (Self::RedstoneLamp, state | BLOCK_STATE_OPEN_BIT),
            68 => (Self::OakDoor, state | BLOCK_STATE_OPEN_BIT),
            70 => (Self::OakTrapdoor, state | BLOCK_STATE_OPEN_BIT),
            82 => (Self::EndPortalFrame, state | BLOCK_STATE_OPEN_BIT),
            90 => (Self::Furnace, state | BLOCK_STATE_OPEN_BIT),
            _ => (Self::from_u8(block_id), state),
        }
    }

    /// Collapse reserved holes to their live base type.
    pub const fn canonicalize(self) -> Self {
        match self {
            Self::Reserved50 => Self::RedstoneTorch,
            Self::Reserved52 => Self::Repeater,
            Self::Reserved54 => Self::Comparator,
            Self::Reserved56 => Self::StoneButton,
            Self::Reserved58 => Self::Lever,
            Self::Reserved60 => Self::PressurePlate,
            Self::Reserved62 => Self::Piston,
            Self::Reserved64 => Self::StickyPiston,
            Self::Reserved66 => Self::RedstoneLamp,
            Self::Reserved68 => Self::OakDoor,
            Self::Reserved70 => Self::OakTrapdoor,
            Self::Reserved82 => Self::EndPortalFrame,
            Self::Reserved90 => Self::Furnace,
            other => other,
        }
    }

    pub const fn is_reserved_hole(self) -> bool {
        matches!(
            self,
            Self::Reserved50
                | Self::Reserved52
                | Self::Reserved54
                | Self::Reserved56
                | Self::Reserved58
                | Self::Reserved60
                | Self::Reserved62
                | Self::Reserved64
                | Self::Reserved66
                | Self::Reserved68
                | Self::Reserved70
                | Self::Reserved82
                | Self::Reserved90
        )
    }

    /// State-aware light emission (lamp/furnace/torch/end-frame).
    pub fn light_emission_for(self, state: BlockState) -> u8 {
        match self.canonicalize() {
            Self::RedstoneTorch => {
                if state.is_open {
                    0
                } else {
                    7
                }
            }
            Self::RedstoneLamp => {
                if state.is_open {
                    15
                } else {
                    0
                }
            }
            Self::Furnace => {
                if state.is_open {
                    13
                } else {
                    0
                }
            }
            Self::EndPortalFrame => {
                if state.is_open {
                    2
                } else {
                    0
                }
            }
            other => other.def().properties.light_emission,
        }
    }

    /// State-aware face atlas tile (lit lamp / filled end-frame top).
    pub fn face_tex_for(self, state: BlockState, face_idx: usize) -> (u32, u32) {
        let face = face_idx.min(5);
        match self.canonicalize() {
            Self::RedstoneLamp if state.is_open => (8, 14),
            Self::EndPortalFrame if state.is_open && face == 4 => (6, 4),
            other => other.get_face_tex_index(face),
        }
    }

    pub fn is_solid_for(self, state: BlockState) -> bool {
        match self.canonicalize() {
            Self::OakDoor | Self::OakTrapdoor => !state.is_open,
            other => other.properties().is_solid,
        }
    }

    pub fn is_passable_for(self, state: BlockState) -> bool {
        match self.canonicalize() {
            Self::OakDoor | Self::OakTrapdoor => state.is_open,
            other => other.properties().is_passable,
        }
    }

    /// The intentionally small Plan27 waterlogging contract.  Other blocks
    /// retain their existing fluid semantics and must be rejected by the
    /// authority rather than silently accepting bit 7.
    pub const fn is_waterloggable(self) -> bool {
        matches!(self, BlockType::OakSlab | BlockType::CobblestoneSlab)
    }

    #[inline]
    pub fn def(self) -> &'static BlockDef {
        &BLOCK_TABLE[self.canonicalize() as usize]
    }

    #[inline]
    pub fn is_cross_model(self) -> bool {
        self.def().is_cross_model
    }

    pub fn can_stay_on(self, below: BlockType) -> bool {
        match self.canonicalize() {
            BlockType::WheatCrop | BlockType::CarrotCrop | BlockType::PotatoCrop => {
                below == BlockType::Farmland
            }
            BlockType::Dandelion | BlockType::Poppy | BlockType::TallGrass => {
                matches!(
                    below,
                    BlockType::Grass | BlockType::Dirt | BlockType::Farmland
                )
            }
            BlockType::SugarCane => {
                matches!(
                    below,
                    BlockType::Grass | BlockType::Dirt | BlockType::Sand | BlockType::SugarCane
                )
            }
            BlockType::Cactus => {
                matches!(below, BlockType::Sand | BlockType::Cactus)
            }
            BlockType::SnowLayer => below.properties().is_solid,
            BlockType::Torch
            | BlockType::RedstoneTorch
            | BlockType::RedstoneWire
            | BlockType::Repeater
            | BlockType::Comparator
            | BlockType::PressurePlate => below.properties().is_solid,
            _ => true,
        }
    }

    /// Validates support using loaded world context. `None` means the queried
    /// position belongs to a chunk whose data is not currently available.
    ///
    /// Existing blocks are only removed for `Unsupported`; `Unknown` preserves
    /// them until the missing neighbor loads. New player placements require
    /// `Supported`, so they never assume an unloaded neighbor contains water or
    /// empty space.
    pub fn support_status_at<F>(
        self,
        position: (i32, i32, i32),
        mut get_loaded_block: F,
    ) -> BlockSupportStatus
    where
        F: FnMut(i32, i32, i32) -> Option<BlockType>,
    {
        let (x, y, z) = position;

        match self {
            BlockType::SugarCane => {
                if y <= 0 {
                    return BlockSupportStatus::Unsupported;
                }
                let Some(below) = get_loaded_block(x, y - 1, z) else {
                    return BlockSupportStatus::Unknown;
                };
                if below == BlockType::SugarCane {
                    return BlockSupportStatus::Supported;
                }
                if !matches!(below, BlockType::Grass | BlockType::Dirt | BlockType::Sand) {
                    return BlockSupportStatus::Unsupported;
                }

                let mut has_unknown_neighbor = false;
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    match get_loaded_block(x + dx, y - 1, z + dz) {
                        Some(BlockType::Water) => return BlockSupportStatus::Supported,
                        Some(_) => {}
                        None => has_unknown_neighbor = true,
                    }
                }
                if has_unknown_neighbor {
                    BlockSupportStatus::Unknown
                } else {
                    BlockSupportStatus::Unsupported
                }
            }
            BlockType::Cactus => {
                if y <= 0 {
                    return BlockSupportStatus::Unsupported;
                }
                let Some(below) = get_loaded_block(x, y - 1, z) else {
                    return BlockSupportStatus::Unknown;
                };
                if !matches!(below, BlockType::Sand | BlockType::Cactus) {
                    return BlockSupportStatus::Unsupported;
                }

                let mut has_unknown_neighbor = false;
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    match get_loaded_block(x + dx, y, z + dz) {
                        Some(block) if block.properties().is_solid || block == BlockType::Lava => {
                            return BlockSupportStatus::Unsupported;
                        }
                        Some(_) => {}
                        None => has_unknown_neighbor = true,
                    }
                }
                if has_unknown_neighbor {
                    BlockSupportStatus::Unknown
                } else {
                    BlockSupportStatus::Supported
                }
            }
            BlockType::OakDoor => {
                if y <= 0 {
                    BlockSupportStatus::Unsupported
                } else {
                    match get_loaded_block(x, y - 1, z) {
                        Some(below) if below == BlockType::OakDoor || self.can_stay_on(below) => {
                            BlockSupportStatus::Supported
                        }
                        Some(_) => BlockSupportStatus::Unsupported,
                        None => BlockSupportStatus::Unknown,
                    }
                }
            }
            BlockType::OakLadder => {
                let mut has_unknown = false;
                for (dx, dz) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                    match get_loaded_block(x + dx, y, z + dz) {
                        Some(b) if b.properties().is_solid => return BlockSupportStatus::Supported,
                        Some(_) => {}
                        None => has_unknown = true,
                    }
                }
                if has_unknown {
                    BlockSupportStatus::Unknown
                } else {
                    BlockSupportStatus::Unsupported
                }
            }
            BlockType::OakSign => {
                if y <= 0 {
                    BlockSupportStatus::Unsupported
                } else {
                    let mut has_unknown = false;
                    if let Some(below) = get_loaded_block(x, y - 1, z) {
                        if below.properties().is_solid {
                            return BlockSupportStatus::Supported;
                        }
                    } else {
                        has_unknown = true;
                    }
                    for (dx, dz) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                        match get_loaded_block(x + dx, y, z + dz) {
                            Some(b) if b.properties().is_solid => {
                                return BlockSupportStatus::Supported
                            }
                            Some(_) => {}
                            None => has_unknown = true,
                        }
                    }
                    if has_unknown {
                        BlockSupportStatus::Unknown
                    } else {
                        BlockSupportStatus::Unsupported
                    }
                }
            }
            BlockType::Dandelion
            | BlockType::Poppy
            | BlockType::TallGrass
            | BlockType::SnowLayer
            | BlockType::Torch
            | BlockType::RedstoneTorch
            | BlockType::RedstoneWire
            | BlockType::Repeater
            | BlockType::Comparator
            | BlockType::PressurePlate => {
                if y <= 0 {
                    BlockSupportStatus::Unsupported
                } else {
                    match get_loaded_block(x, y - 1, z) {
                        Some(below) if self.can_stay_on(below) => BlockSupportStatus::Supported,
                        Some(_) => BlockSupportStatus::Unsupported,
                        None => BlockSupportStatus::Unknown,
                    }
                }
            }
            _ => BlockSupportStatus::Supported,
        }
    }

    #[inline]
    pub fn sound_material(self) -> Option<SoundMaterial> {
        self.def().sound
    }

    #[inline]
    pub fn properties(self) -> &'static BlockProperties {
        &self.def().properties
    }

    /// Whether this block is a full, opaque cube that casts vertex ambient occlusion.
    pub fn is_ao_occluder(self) -> bool {
        let properties = self.properties();
        properties.is_solid && properties.render_type == RenderType::Opaque
    }

    #[inline]
    pub fn get_face_tex_index(self, face_idx: usize) -> (u32, u32) {
        self.def().face_tex[face_idx.min(5)]
    }

    #[inline]
    pub fn preferred_tool(self) -> ToolType {
        self.def().preferred_tool
    }

    #[inline]
    pub fn min_harvest_material(self) -> Option<ToolMaterial> {
        self.def().min_harvest
    }

}

