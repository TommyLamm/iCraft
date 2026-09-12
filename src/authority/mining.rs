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

    let old_def = old_block.def();
    let eligible = old_def.min_harvest.map_or(true, |minimum| {
        held_stack
            .and_then(|stack| stack.item.tool_properties())
            .is_some_and(|tool| {
                tool.tool_type == old_def.preferred_tool && tool.material >= minimum
            })
    });
    let silk_touch = held_stack
        .map(|stack| stack.enchantments.level_of(Enchantment::SilkTouch) > 0)
        .unwrap_or(false);
    let mut drops = Vec::new();
    if eligible {
        let fortune = held_stack
            .map(|stack| stack.enchantments.level_of(Enchantment::Fortune(1)) as u32)
            .unwrap_or(0);
        if silk_touch {
            drops.push(ItemStack::new(Item::from_block(old_block), 1));
        } else {
            let specialized_crop = matches!(
                old_block,
                BlockType::WheatCrop
                    | BlockType::CarrotCrop
                    | BlockType::PotatoCrop
                    | BlockType::TallGrass
            );
            if !specialized_crop {
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
        }
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
                let item = match (old_block, old_state & 0b111) {
                    (BlockType::WheatCrop, 7) => Item::Wheat,
                    (BlockType::WheatCrop, _) => Item::Seeds,
                    (BlockType::CarrotCrop, _) => Item::Carrot,
                    (BlockType::PotatoCrop, _) => Item::Potato,
                    _ => Item::Air,
                };
                drops.push(ItemStack::new(item, 1));
            }
            _ => {}
        }
    }

    let xp = if eligible && !silk_touch {
        match old_block {
            BlockType::DiamondOre => 5,
            BlockType::CoalOre
            | BlockType::IronOre
            | BlockType::GoldOre
            | BlockType::RedstoneOre => 2,
            _ => 0,
        }
    } else {
        0
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
    let def = block.def();
    let hardness = def.properties.hardness;
    if hardness < 0.0 {
        return f32::MAX;
    }
    let preferred = def.preferred_tool;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculate_block_break_rewards_harvest_and_drops() {
        let pos = (10, 60, 10);

        // Stone with bare hand in Survival -> not eligible to harvest (no drops)
        let rewards =
            calculate_block_break_rewards(BlockType::Stone, 0, pos, None, GameMode::Survival);
        assert!(rewards.drops.is_empty());
        assert_eq!(rewards.xp, 0);

        // Stone with Pickaxe -> eligible, drops Stone
        let pick = ItemStack::new(Item::StonePickaxe, 1);
        let rewards = calculate_block_break_rewards(
            BlockType::Stone,
            0,
            pos,
            Some(&pick),
            GameMode::Survival,
        );
        assert_eq!(rewards.drops.len(), 1);
        assert_eq!(rewards.drops[0].item, Item::Stone);

        // DiamondOre with IronPickaxe -> drops Diamond + 5 XP
        let iron_pick = ItemStack::new(Item::IronPickaxe, 1);
        let rewards = calculate_block_break_rewards(
            BlockType::DiamondOre,
            0,
            pos,
            Some(&iron_pick),
            GameMode::Survival,
        );
        assert_eq!(rewards.drops[0].item, Item::Diamond);
        assert_eq!(rewards.xp, 5);

        // DiamondOre with SilkTouch -> drops DiamondOre block, no XP
        let mut silk_pick = ItemStack::new(Item::IronPickaxe, 1);
        silk_pick
            .enchantments
            .add_or_upgrade(crate::enchantment::Enchantment::SilkTouch);
        let rewards = calculate_block_break_rewards(
            BlockType::DiamondOre,
            0,
            pos,
            Some(&silk_pick),
            GameMode::Survival,
        );
        assert_eq!(rewards.drops[0].item, Item::DiamondOre);
        assert_eq!(rewards.xp, 0);

        // Creative mode -> zero drops
        let rewards = calculate_block_break_rewards(
            BlockType::Stone,
            0,
            pos,
            Some(&pick),
            GameMode::Creative,
        );
        assert!(rewards.drops.is_empty());
    }

    #[test]
    fn calculate_block_break_rewards_mature_and_immature_crops() {
        let pos = (10, 60, 10);

        // Mature Wheat (age 7) -> Wheat
        let mature_wheat =
            calculate_block_break_rewards(BlockType::WheatCrop, 7, pos, None, GameMode::Survival);
        assert_eq!(mature_wheat.drops.len(), 1);
        assert_eq!(mature_wheat.drops[0].item, Item::Wheat);

        // Immature Wheat (age 3) -> Seeds
        let immature_wheat =
            calculate_block_break_rewards(BlockType::WheatCrop, 3, pos, None, GameMode::Survival);
        assert_eq!(immature_wheat.drops.len(), 1);
        assert_eq!(immature_wheat.drops[0].item, Item::Seeds);

        // Immature Carrot (age 2) -> Carrot
        let immature_carrot =
            calculate_block_break_rewards(BlockType::CarrotCrop, 2, pos, None, GameMode::Survival);
        assert_eq!(immature_carrot.drops.len(), 1);
        assert_eq!(immature_carrot.drops[0].item, Item::Carrot);
    }

    #[test]
    fn mining_time_calculation() {
        let hand_dirt = mining_time_seconds(BlockType::Dirt, None);
        assert!(hand_dirt > 0.0);

        let shovel = ItemStack::new(Item::StoneShovel, 1);
        let shovel_dirt = mining_time_seconds(BlockType::Dirt, Some(&shovel));
        assert!(shovel_dirt < hand_dirt);

        let bedrock = mining_time_seconds(BlockType::Bedrock, None);
        assert_eq!(bedrock, f32::MAX);
    }

    #[test]
    fn debug_ore_xp_and_crop_drops() {
        let pos = (10, 60, 10);
        let bare_iron =
            calculate_block_break_rewards(BlockType::IronOre, 0, pos, None, GameMode::Survival);
        let immature =
            calculate_block_break_rewards(BlockType::WheatCrop, 3, pos, None, GameMode::Survival);
        let tall =
            calculate_block_break_rewards(BlockType::TallGrass, 0, pos, None, GameMode::Survival);
        assert_eq!(bare_iron.xp, 0);
        assert!(bare_iron.drops.is_empty());
        assert_eq!(immature.drops.len(), 1);
        assert_eq!(immature.drops[0].item, Item::Seeds);
        assert!(tall.drops.iter().all(|d| d.item != Item::TallGrass));
    }
}
