//! Small, GPU-independent helpers shared by the authoritative mining path and
//! the legacy presentation reward adapter.

use crate::enchantment::Enchantment;
use crate::inventory::{GameMode, Item, ItemStack};
use crate::world::BlockType;

#[derive(Debug, Clone, PartialEq)]
pub struct BlockBreakRewards {
    pub drops: Vec<ItemStack>,
    pub xp: u32,
    pub exhaustion: f32,
    pub tool_damaged: bool,
}

/// Compute the existing narrow block-drop contract without touching renderer
/// state. This deliberately follows the established block/tool helper
/// methods instead of attempting to reproduce the entire vanilla table.
pub fn calculate_block_break_rewards(
    old_block: BlockType,
    old_state: u8,
    pos: (i32, i32, i32),
    held_stack: Option<&ItemStack>,
    game_mode: GameMode,
) -> BlockBreakRewards {
    if matches!(game_mode, GameMode::Creative | GameMode::Spectator) {
        return BlockBreakRewards {
            drops: Vec::new(),
            xp: 0,
            exhaustion: 0.0,
            tool_damaged: false,
        };
    }

    let eligible = old_block.min_harvest_material().map_or(true, |minimum| {
        held_stack
            .and_then(|stack| stack.item.tool_properties())
            .is_some_and(|tool| {
                tool.tool_type == old_block.preferred_tool() && tool.material >= minimum
            })
    });
    let mut drops = Vec::new();
    if eligible {
        let silk_touch = held_stack
            .map(|stack| stack.enchantments.level_of(Enchantment::SilkTouch) > 0)
            .unwrap_or(false);
        let fortune = held_stack
            .map(|stack| stack.enchantments.level_of(Enchantment::Fortune(1)) as u32)
            .unwrap_or(0);
        if silk_touch {
            drops.push(ItemStack::new(Item::from_block(old_block), 1));
        } else {
            let base_drop = match old_block {
                BlockType::CoalOre => Item::Coal,
                BlockType::DiamondOre => Item::Diamond,
                BlockType::RedstoneOre => Item::Redstone,
                _ => Item::from_block(old_block),
            };
            let fortune_eligible = matches!(
                old_block,
                BlockType::CoalOre | BlockType::DiamondOre | BlockType::RedstoneOre
            );
            let bonus = if fortune_eligible && fortune > 0 {
                ((pos.0 as u32)
                    .wrapping_mul(31)
                    .wrapping_add((pos.1 as u32).wrapping_mul(17))
                    .wrapping_add((pos.2 as u32).wrapping_mul(13))
                    % (fortune + 1))
                    + fortune / 2
            } else {
                0
            };
            drops.extend((0..=bonus).map(|_| ItemStack::new(base_drop, 1)));
        }
        // Keep a deterministic crop/decoration subset from the existing
        // helper's contract; unsupported tables intentionally drop nothing.
        match old_block {
            BlockType::TallGrass if drops.is_empty() => {
                let seed = (pos.0 as u32)
                    .wrapping_mul(31)
                    .wrapping_add((pos.1 as u32).wrapping_mul(17))
                    .wrapping_add(pos.2 as u32);
                if seed % 8 == 0 {
                    drops.push(ItemStack::new(Item::Seeds, 1));
                }
            }
            BlockType::WheatCrop | BlockType::CarrotCrop | BlockType::PotatoCrop => {
                let age = old_state & 0b111;
                if age == 7 {
                    let item = match old_block {
                        BlockType::WheatCrop => Item::Wheat,
                        BlockType::CarrotCrop => Item::Carrot,
                        BlockType::PotatoCrop => Item::Potato,
                        _ => Item::Air,
                    };
                    if item != Item::Air {
                        drops.push(ItemStack::new(item, 1));
                    }
                } else {
                    let item = match old_block {
                        BlockType::WheatCrop => Item::Seeds,
                        BlockType::CarrotCrop => Item::Carrot,
                        BlockType::PotatoCrop => Item::Potato,
                        _ => Item::Air,
                    };
                    if item != Item::Air {
                        drops.push(ItemStack::new(item, 1));
                    }
                }
            }
            _ => {}
        }
    }

    let xp = match old_block {
        BlockType::DiamondOre => 5,
        BlockType::CoalOre | BlockType::IronOre | BlockType::GoldOre | BlockType::RedstoneOre => 2,
        _ => 0,
    };
    BlockBreakRewards {
        drops,
        xp,
        exhaustion: 0.005,
        tool_damaged: held_stack.is_some_and(|stack| stack.item.tool_properties().is_some()),
    }
}

/// Deterministic fixed-tick mining duration. Creative is handled by the
/// authority before calling this helper.
pub fn mining_time_seconds(block: BlockType, held_stack: Option<&ItemStack>) -> f32 {
    let hardness = block.properties().hardness;
    if hardness < 0.0 {
        return f32::MAX;
    }
    let preferred = block.preferred_tool();
    let mut speed = 1.0;
    let mut matching = false;
    if let Some(stack) = held_stack {
        if let Some(properties) = stack.item.tool_properties() {
            if properties.tool_type == preferred && preferred != crate::inventory::ToolType::None {
                speed = properties.mining_speed;
                matching = true;
            }
        }
    }
    let base = if matching || preferred == crate::inventory::ToolType::None {
        hardness * 1.5
    } else {
        hardness * 5.0
    };
    let enchantment_multiplier = held_stack
        .map(|stack| crate::enchantment::mining_speed_multiplier(&stack.enchantments))
        .unwrap_or(1.0);
    base / (speed * enchantment_multiplier).max(f32::EPSILON)
}
